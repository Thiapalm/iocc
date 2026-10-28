use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread;

/// Lines kept above the visible screen, reachable with PageUp/PageDown
const SCROLLBACK: usize = 2000;

/**
 * Interactive shell running in a pseudo terminal. The reader thread only moves
 * bytes; the `vt100` parser turns them into a screen that the TUI draws cell by
 * cell. Keeping the parser here means a program like `vim` or `top` can run
 * inside the tab without the TUI knowing about its escape sequences.
 */
pub struct TerminalSession {
    parser: vt100::Parser,
    writer: Option<Box<dyn Write + Send>>,
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    output: Receiver<Vec<u8>>,
    /// Message shown instead of the prompt once the shell is gone
    exited: Option<String>,
    shell: String,
    rows: u16,
    cols: u16,
}

impl TerminalSession {
    /**
     * Starts the user shell (`$SHELL`, falling back to `/bin/sh` or `cmd.exe`)
     * with the environment the TUI was launched with. When the TUI has a broker
     * socket it is exported as `IOCC_SOCKET`, so commands typed in the terminal
     * share the MCP2221 with the TUI instead of fighting for it.
     */
    pub fn spawn(rows: u16, cols: u16, socket: Option<&Path>) -> Result<Self, String> {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.trim().is_empty())
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    "cmd.exe".to_string()
                } else {
                    "/bin/sh".to_string()
                }
            });
        let mut command = CommandBuilder::new(&shell);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("IOCC_TERMINAL", "1");
        match socket {
            Some(path) => command.env("IOCC_SOCKET", path),
            None => command.env_remove("IOCC_SOCKET"),
        };
        Self::spawn_command(command, rows, cols, shell)
    }

    fn spawn_command(
        command: CommandBuilder,
        rows: u16,
        cols: u16,
        shell: String,
    ) -> Result<Self, String> {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let pair = native_pty_system()
            .openpty(pty_size(rows, cols))
            .map_err(|err| format!("cannot open a pseudo terminal: {err}"))?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| format!("cannot start {shell}: {err}"))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| format!("cannot read from the terminal: {err}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| format!("cannot write to the terminal: {err}"))?;

        // Bounded on purpose: a program that floods the terminal blocks on the
        // pty instead of growing this channel without limit.
        let (tx, output) = sync_channel(256);
        if let Err(err) = thread::Builder::new()
            .name("iocc-terminal".into())
            .spawn(move || {
                let mut buffer = [0u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            if tx.send(buffer[..read].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
        {
            let _ = child.kill();
            return Err(format!("cannot start the terminal reader: {err}"));
        }

        Ok(TerminalSession {
            parser: vt100::Parser::new(nonzero(rows), nonzero(cols), SCROLLBACK),
            writer: Some(writer),
            master: Some(pair.master),
            child: Some(child),
            output,
            exited: None,
            shell,
            rows,
            cols,
        })
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn shell(&self) -> &str {
        &self.shell
    }

    pub fn exited(&self) -> Option<&str> {
        self.exited.as_deref()
    }

    /**
     * Sends bytes to the shell. A write failure means the other side is gone;
     * the next `pump` would find the same, so report it right away.
     */
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let Some(writer) = &mut self.writer else {
            return Err("the shell is not running".into());
        };
        writer
            .write_all(bytes)
            .and_then(|()| writer.flush())
            .map_err(|err| err.to_string())
    }

    pub fn mark_exited(&mut self, message: String) {
        self.writer = None;
        self.exited = Some(message);
    }

    /**
     * Applies new bytes and checks whether the child is still alive. Returns the
     * message once, when the exit is first noticed.
     */
    pub fn pump(&mut self) -> Option<String> {
        self.drain();
        if self.exited.is_some() {
            return None;
        }
        let Some(child) = &mut self.child else {
            return None;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                let message = match status.signal() {
                    Some(signal) => format!("shell exited ({signal})"),
                    None => format!("shell exited (code {})", status.exit_code()),
                };
                self.child = None;
                self.mark_exited(message.clone());
                self.drain();
                Some(message)
            }
            Ok(None) => None,
            Err(err) => {
                let message = format!("terminal: {err}");
                self.child = None;
                self.mark_exited(message.clone());
                Some(message)
            }
        }
    }

    fn drain(&mut self) {
        while let Ok(bytes) = self.output.try_recv() {
            self.parser.process(&bytes);
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        if (rows, cols) == (self.rows, self.cols) {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        if let Some(master) = &self.master {
            let _ = master.resize(pty_size(rows, cols));
        }
        self.parser.set_size(nonzero(rows), nonzero(cols));
    }

    /**
     * Moves the scrollback window; negative values go back in history and
     * positive values return to the live screen.
     */
    pub fn scroll(&mut self, lines: isize) {
        let current = self.parser.screen().scrollback() as isize;
        let capacity = self.parser.screen().scrollback_len() as isize;
        let target = (current + lines).clamp(0, capacity) as usize;
        self.parser.screen_mut().set_scrollback(target);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.parser.screen_mut().set_scrollback(0);
    }

    #[cfg(test)]
    pub(crate) fn spawn_for_test(
        program: &str,
        args: &[&str],
        rows: u16,
        cols: u16,
    ) -> Result<Self, String> {
        let mut command = CommandBuilder::new(program);
        command.args(args);
        Self::spawn_command(command, rows, cols, program.to_string())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
        // Dropping the writer and the master closes the pty, which also ends the
        // reader thread blocked on read().
        self.writer = None;
        self.master = None;
    }
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn nonzero(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).unwrap_or(NonZeroU16::MIN)
}

/**
 * Translates a TUI key into the bytes a terminal emulator would send. Keys that
 * the TUI keeps for itself (F12, focus handling) are filtered by the caller.
 * `application_cursor` makes the arrows and Home/End use the SS3 sequences that
 * programs such as `vim` expect after enabling DECCKM.
 */
pub fn key_bytes(key: &KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let mut bytes = match key.code {
        KeyCode::Char(character) => match ctrl.then(|| control_byte(character)).flatten() {
            Some(byte) => vec![byte],
            None => character.to_string().into_bytes(),
        },
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab if shift => b"\x1b[Z".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Home if application_cursor => b"\x1bOH".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End if application_cursor => b"\x1bOF".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Up => cursor_key(b'A', application_cursor),
        KeyCode::Down => cursor_key(b'B', application_cursor),
        KeyCode::Right => cursor_key(b'C', application_cursor),
        KeyCode::Left => cursor_key(b'D', application_cursor),
        KeyCode::F(number) => function_key(number)?,
        _ => return None,
    };
    if alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn cursor_key(direction: u8, application_cursor: bool) -> Vec<u8> {
    if application_cursor {
        vec![0x1b, b'O', direction]
    } else {
        vec![0x1b, b'[', direction]
    }
}

fn function_key(number: u8) -> Option<Vec<u8>> {
    let bytes = match number {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => return None,
    };
    Some(bytes)
}

/**
 * Control characters produced by Ctrl+<key>, following the ASCII control table
 */
fn control_byte(character: char) -> Option<u8> {
    let lower = character.to_ascii_lowercase();
    match lower {
        'a'..='z' => Some(lower as u8 - b'a' + 1),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        ' ' | '@' => Some(0),
        '?' => Some(0x7f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn control_and_alt_keys_match_terminal_sequences() {
        assert_eq!(
            key_bytes(
                &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                false
            ),
            Some(vec![0x03])
        );
        assert_eq!(
            key_bytes(
                &KeyEvent::new(KeyCode::Char('D'), KeyModifiers::CONTROL),
                false
            ),
            Some(vec![0x04])
        );
        assert_eq!(
            key_bytes(
                &KeyEvent::new(KeyCode::Char('?'), KeyModifiers::CONTROL),
                false
            ),
            Some(vec![0x7f])
        );
        assert_eq!(
            key_bytes(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT), false),
            Some(vec![0x1b, b'x'])
        );
        assert_eq!(
            key_bytes(&key(KeyCode::Char('é')), false),
            Some("é".as_bytes().to_vec())
        );
    }

    #[test]
    fn arrows_follow_the_application_cursor_mode() {
        assert_eq!(
            key_bytes(&key(KeyCode::Up), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(key_bytes(&key(KeyCode::Up), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            key_bytes(&key(KeyCode::Home), false),
            Some(b"\x1b[H".to_vec())
        );
        assert_eq!(
            key_bytes(&key(KeyCode::Home), true),
            Some(b"\x1bOH".to_vec())
        );
    }

    #[test]
    fn function_keys_use_xterm_sequences() {
        assert_eq!(
            key_bytes(&key(KeyCode::F(1)), false),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            key_bytes(&key(KeyCode::F(12)), false),
            Some(b"\x1b[24~".to_vec())
        );
        assert_eq!(key_bytes(&key(KeyCode::F(13)), false), None);
    }

    #[cfg(unix)]
    #[test]
    fn runs_a_command_and_reads_its_output() {
        let mut session = TerminalSession::spawn_for_test(
            "/bin/sh",
            &["-c", "printf 'terminal output\\n'"],
            24,
            80,
        )
        .unwrap();
        let start = Instant::now();
        while session.exited().is_none() && start.elapsed() < Duration::from_secs(10) {
            session.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        // One more pump drains anything the reader sent after the exit
        session.pump();
        assert!(session.exited().is_some(), "the shell did not exit");
        assert!(
            session.screen().contents().contains("terminal output"),
            "{:?}",
            session.screen().contents()
        );
    }
}
