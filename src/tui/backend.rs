use super::manifest::{MANIFEST_FLAG, Manifest};
use crate::comm::{check_device, read_device, scan_addresses, start_device, write_device};
use crate::plugin::{find_plugin, list_plugins, plugin_command};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

const MANIFEST_TIMEOUT: Duration = Duration::from_secs(3);
const PLUGIN_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
pub struct DeviceParams {
    pub speed: u32,
    pub reset: bool,
    pub timeout: u64,
}

pub enum Req {
    Status,
    Scan,
    Read {
        addr: u8,
        reg: u8,
        count: usize,
    },
    Write {
        addr: u8,
        reg: u8,
        data: Vec<u8>,
    },
    LoadPlugins,
    Plugin {
        tag: String,
        name: String,
        args: Vec<String>,
    },
}

pub struct PluginOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct PluginEntry {
    pub name: String,
    pub path: PathBuf,
    /// True when the plugin answered the manifest request
    pub described: bool,
    pub manifest: Manifest,
}

pub enum Event {
    Connection(bool),
    Status(Result<String, String>),
    Scan(Result<Vec<u8>, String>),
    Read {
        addr: u8,
        reg: u8,
        result: Result<Vec<u8>, String>,
    },
    Written {
        addr: u8,
        reg: u8,
        data: Vec<u8>,
        result: Result<(), String>,
    },
    Plugins(Vec<PluginEntry>),
    Plugin {
        tag: String,
        name: String,
        args: Vec<String>,
        result: Result<PluginOutput, String>,
    },
}

/**
 * Starts the thread that owns the MCP2221. Every blocking operation (USB or plugin
 * execution) happens there, so the UI never freezes. The device is released before
 * any plugin runs because the chip only accepts one process at a time.
 */
pub fn spawn(params: DeviceParams) -> (Sender<Req>, Receiver<Event>) {
    let (req_tx, req_rx) = channel::<Req>();
    let (ev_tx, ev_rx) = channel::<Event>();
    thread::spawn(move || {
        let mut dev: Option<mcp2221::Handle> = None;
        while let Ok(req) = req_rx.recv() {
            handle(req, &mut dev, params, &ev_tx);
        }
    });
    (req_tx, ev_rx)
}

fn open(
    dev: &mut Option<mcp2221::Handle>,
    params: DeviceParams,
) -> Result<&mut mcp2221::Handle, String> {
    if dev.is_none() {
        let handle = start_device(params.speed, params.reset, params.timeout)
            .map_err(|e| format!("{e}, check USB connection"))?;
        *dev = Some(handle);
    }
    Ok(dev.as_mut().expect("device opened above"))
}

/**
 * Runs a device operation, reporting the connection state and dropping the handle on
 * failure so the next request reconnects
 */
fn with_device<T>(
    dev: &mut Option<mcp2221::Handle>,
    params: DeviceParams,
    tx: &Sender<Event>,
    op: impl FnOnce(&mut mcp2221::Handle) -> Result<T, String>,
) -> Result<T, String> {
    let result = open(dev, params).and_then(op);
    if result.is_err() {
        *dev = None;
    }
    let _ = tx.send(Event::Connection(result.is_ok()));
    result
}

fn handle(req: Req, dev: &mut Option<mcp2221::Handle>, params: DeviceParams, tx: &Sender<Event>) {
    match req {
        Req::Status => {
            let result = with_device(dev, params, tx, |d| {
                check_device(d).map_err(|e| e.to_string())
            });
            let _ = tx.send(Event::Status(result));
        }
        Req::Scan => {
            let result = with_device(dev, params, tx, |d| {
                check_device(d).map_err(|e| e.to_string())?;
                Ok(scan_addresses(d))
            });
            let _ = tx.send(Event::Scan(result));
        }
        Req::Read { addr, reg, count } => {
            let result = with_device(dev, params, tx, |d| {
                read_device(d, addr, reg, count).map_err(|e| e.to_string())
            });
            let _ = tx.send(Event::Read { addr, reg, result });
        }
        Req::Write { addr, reg, data } => {
            let result = with_device(dev, params, tx, |d| {
                write_device(d, addr, reg, &data).map_err(|e| e.to_string())
            });
            let _ = tx.send(Event::Written {
                addr,
                reg,
                data,
                result,
            });
        }
        Req::LoadPlugins => {
            *dev = None;
            let _ = tx.send(Event::Plugins(load_plugins()));
        }
        Req::Plugin { tag, name, args } => {
            *dev = None;
            let result = match find_plugin(&name) {
                Some(path) => run_captured(&path, &args, PLUGIN_TIMEOUT),
                None => Err(format!("plugin iocc-{name} not found")),
            };
            let _ = tx.send(Event::Plugin {
                tag,
                name,
                args,
                result,
            });
        }
    }
}

fn load_plugins() -> Vec<PluginEntry> {
    list_plugins()
        .into_iter()
        .filter_map(|name| {
            let path = find_plugin(&name)?;
            let described = run_captured(&path, &[MANIFEST_FLAG.to_string()], MANIFEST_TIMEOUT)
                .ok()
                .filter(|out| out.code == 0)
                .and_then(|out| Manifest::parse(&out.stdout));
            Some(PluginEntry {
                name,
                path,
                described: described.is_some(),
                manifest: described.unwrap_or_else(Manifest::generic),
            })
        })
        .collect()
}

/**
 * Runs a plugin capturing stdout/stderr, killing it after the timeout
 */
fn run_captured(path: &Path, args: &[String], timeout: Duration) -> Result<PluginOutput, String> {
    let mut command = plugin_command(path, args);
    command
        .env("IOCC_TUI", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not run {}: {e}", path.display()))?;

    fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
        thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    }
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok(PluginOutput {
        code: status.code().unwrap_or(1),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn captures_output_and_times_out() {
        let dir = std::env::temp_dir().join(format!("iocc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("iocc-t");
        std::fs::write(&script, "#!/bin/sh\necho out\necho err >&2\nexit 3\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = run_captured(&script, &[], Duration::from_secs(5)).unwrap();
        assert_eq!(
            (out.code, out.stdout.trim(), out.stderr.trim()),
            (3, "out", "err")
        );

        std::fs::write(&script, "#!/bin/sh\nsleep 5\n").unwrap();
        assert!(run_captured(&script, &[], Duration::from_millis(100)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
