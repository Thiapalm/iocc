use super::manifest::{MANIFEST_FLAG, Manifest};
use crate::comm::{check_device, read_device, scan_addresses, start_device, write_device};
use crate::plugin::{find_plugin, list_plugins, plugin_command_with};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
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
        /// Run with IOCC_SOCKET (device stays open) instead of exclusive access
        shared: bool,
    },
    StreamStart {
        tag: String,
        name: String,
        args: Vec<String>,
    },
    StreamStop {
        tag: String,
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
    /// One line of a streaming view
    StreamLine {
        tag: String,
        line: String,
    },
    /// Streaming command ended (or was killed)
    StreamEnd {
        tag: String,
        code: Option<i32>,
        stderr: String,
        /// True when the TUI stopped it on purpose (paused for an exclusive plugin)
        stopped: bool,
    },
}

/**
 * The MCP2221 handle shared between the TUI thread and the broker. Only one
 * process can talk to the chip, so every transaction goes through this mutex.
 */
pub struct SharedDevice {
    dev: Mutex<Option<mcp2221::Handle>>,
    params: DeviceParams,
    /// Set while an exclusive (non broker aware) plugin owns the device
    exclusive: AtomicBool,
    tx: Sender<Event>,
}

impl SharedDevice {
    #[cfg(all(test, unix))]
    pub fn new_for_tests(tx: Sender<Event>) -> SharedDevice {
        SharedDevice::new(
            DeviceParams {
                speed: 400_000,
                reset: false,
                timeout: 10,
            },
            tx,
        )
    }

    fn new(params: DeviceParams, tx: Sender<Event>) -> SharedDevice {
        SharedDevice {
            dev: Mutex::new(None),
            params,
            exclusive: AtomicBool::new(false),
            tx,
        }
    }

    /**
     * Runs a transaction, opening the device on demand. Errors drop the handle so
     * the next request reconnects.
     */
    pub fn with<T>(
        &self,
        op: impl FnOnce(&mut mcp2221::Handle) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.exclusive.load(Ordering::SeqCst) {
            return Err("device in exclusive use by a plugin".into());
        }
        let mut guard = self
            .dev
            .lock()
            .map_err(|_| "device lock poisoned".to_string())?;
        if guard.is_none() {
            let handle = start_device(self.params.speed, self.params.reset, self.params.timeout)
                .map_err(|e| format!("{e}, check USB connection"))?;
            *guard = Some(handle);
        }
        let handle = guard.as_mut().expect("device opened above");
        let result = op(handle);
        if result.is_err() {
            *guard = None;
        }
        let _ = self.tx.send(Event::Connection(result.is_ok()));
        result
    }

    fn release(&self) {
        if let Ok(mut guard) = self.dev.lock() {
            *guard = None;
        }
    }

    fn set_exclusive(&self, exclusive: bool) {
        self.exclusive.store(exclusive, Ordering::SeqCst);
    }
}

pub struct Backend {
    pub tx: Sender<Req>,
    pub rx: Receiver<Event>,
    /// Broker socket handed to plugins through IOCC_SOCKET (None when unavailable)
    pub socket: Option<PathBuf>,
}

/**
 * Starts the thread that owns the MCP2221 and the local device broker. Every
 * blocking operation (USB or plugin execution) happens there, so the UI never
 * freezes.
 */
pub fn spawn(params: DeviceParams) -> Backend {
    spawn_with(params, None)
}

/**
 * Starts the backend with an explicit broker socket path. Tests pass a unique
 * path so several `App`s can run in parallel; production uses the default path.
 */
fn spawn_with(params: DeviceParams, socket: Option<PathBuf>) -> Backend {
    let (req_tx, req_rx) = channel::<Req>();
    let (ev_tx, ev_rx) = channel::<Event>();
    let shared = Arc::new(SharedDevice::new(params, ev_tx.clone()));
    let socket = match socket {
        Some(path) => start_broker_at(&shared, path),
        None => start_broker(&shared),
    };
    let plugin_socket = socket.clone();
    thread::spawn(move || backend_loop(req_rx, shared, ev_tx, plugin_socket));
    Backend {
        tx: req_tx,
        rx: ev_rx,
        socket,
    }
}

/**
 * Backend for tests: a broker on a unique path, so parallel tests never rebind
 * or remove each other's socket.
 */
#[cfg(test)]
pub fn spawn_for_tests(params: DeviceParams) -> Backend {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "iocc-test-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    spawn_with(params, Some(path))
}

fn backend_loop(
    rx: Receiver<Req>,
    shared: Arc<SharedDevice>,
    tx: Sender<Event>,
    socket: Option<PathBuf>,
) {
    let mut streams: HashMap<String, Arc<Mutex<Child>>> = HashMap::new();
    while let Ok(req) = rx.recv() {
        handle(req, &shared, &tx, &mut streams, socket.as_deref());
    }
    // The UI is gone: do not leave streaming plugins behind
    for (_, child) in streams {
        if let Ok(mut child) = child.lock() {
            let _ = child.kill();
        }
    }
}

fn handle(
    req: Req,
    shared: &Arc<SharedDevice>,
    tx: &Sender<Event>,
    streams: &mut HashMap<String, Arc<Mutex<Child>>>,
    socket: Option<&Path>,
) {
    match req {
        Req::Status => {
            let result = shared.with(|d| check_device(d).map_err(|e| e.to_string()));
            let _ = tx.send(Event::Status(result));
        }
        Req::Scan => {
            let result = shared.with(|d| {
                check_device(d).map_err(|e| e.to_string())?;
                Ok(scan_addresses(d))
            });
            let _ = tx.send(Event::Scan(result));
        }
        Req::Read { addr, reg, count } => {
            let result =
                shared.with(|d| read_device(d, addr, reg, count).map_err(|e| e.to_string()));
            let _ = tx.send(Event::Read { addr, reg, result });
        }
        Req::Write { addr, reg, data } => {
            let result =
                shared.with(|d| write_device(d, addr, reg, &data).map_err(|e| e.to_string()));
            let _ = tx.send(Event::Written {
                addr,
                reg,
                data,
                result,
            });
        }
        Req::LoadPlugins => {
            let _ = tx.send(Event::Plugins(load_plugins(socket)));
        }
        Req::Plugin {
            tag,
            name,
            args,
            shared: is_shared,
        } => {
            let plugin_socket = if is_shared { socket } else { None };
            if !is_shared {
                // Exclusive: the plugin opens the chip itself, so no stream can keep using it
                stop_streams(streams, tx);
                shared.set_exclusive(true);
                shared.release();
            }
            let result = match find_plugin(&name) {
                Some(path) => run_captured(&path, &args, plugin_socket, PLUGIN_TIMEOUT),
                None => Err(format!("plugin iocc-{name} not found")),
            };
            if !is_shared {
                shared.set_exclusive(false);
            }
            let _ = tx.send(Event::Plugin {
                tag,
                name,
                args,
                result,
            });
        }
        Req::StreamStart { tag, name, args } => {
            if streams.contains_key(&tag) {
                return;
            }
            let Some(path) = find_plugin(&name) else {
                let _ = tx.send(Event::StreamEnd {
                    tag,
                    code: None,
                    stderr: format!("plugin iocc-{name} not found"),
                    stopped: false,
                });
                return;
            };
            match start_stream(tag.clone(), &path, &args, socket, tx) {
                Ok(child) => {
                    streams.insert(tag, child);
                }
                Err(e) => {
                    let _ = tx.send(Event::StreamEnd {
                        tag,
                        code: None,
                        stderr: e,
                        stopped: false,
                    });
                }
            }
        }
        Req::StreamStop { tag } => {
            if let Some(child) = streams.remove(&tag)
                && let Ok(mut child) = child.lock()
            {
                let _ = child.kill();
            }
        }
    }
}

fn stop_streams(streams: &mut HashMap<String, Arc<Mutex<Child>>>, tx: &Sender<Event>) {
    for (tag, child) in streams.drain() {
        if let Ok(mut child) = child.lock() {
            let _ = child.kill();
        }
        let _ = tx.send(Event::StreamEnd {
            tag,
            code: None,
            stderr: String::new(),
            stopped: true,
        });
    }
}

/**
 * Broker stubs for platforms without Unix sockets
 */
#[cfg(not(unix))]
mod broker {
    use super::SharedDevice;
    use std::path::PathBuf;
    use std::sync::Arc;

    pub fn start(_shared: &Arc<SharedDevice>) -> Option<PathBuf> {
        None
    }

    pub fn start_at(_shared: &Arc<SharedDevice>, _path: PathBuf) -> Option<PathBuf> {
        None
    }
}

#[cfg(unix)]
use super::broker;

fn start_broker(shared: &Arc<SharedDevice>) -> Option<PathBuf> {
    broker::start(shared)
}

fn start_broker_at(shared: &Arc<SharedDevice>, path: PathBuf) -> Option<PathBuf> {
    broker::start_at(shared, path)
}

fn load_plugins(socket: Option<&Path>) -> Vec<PluginEntry> {
    list_plugins()
        .into_iter()
        .filter_map(|name| {
            let path = find_plugin(&name)?;
            let described = run_captured(
                &path,
                &[MANIFEST_FLAG.to_string()],
                socket,
                MANIFEST_TIMEOUT,
            )
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
fn run_captured(
    path: &Path,
    args: &[String],
    socket: Option<&Path>,
    timeout: Duration,
) -> Result<PluginOutput, String> {
    let mut command = plugin_command_with(path, args, socket);
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

/**
 * Starts a long running plugin whose stdout lines are forwarded to the UI
 */
fn start_stream(
    tag: String,
    path: &Path,
    args: &[String],
    socket: Option<&Path>,
    tx: &Sender<Event>,
) -> Result<Arc<Mutex<Child>>, String> {
    let mut command = plugin_command_with(path, args, socket);
    command
        .env("IOCC_TUI", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not run {}: {e}", path.display()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));

    let tx = tx.clone();
    let watched = Arc::clone(&child);
    thread::spawn(move || {
        let stderr_thread = thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = stderr {
                let _ = p.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        });
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        let _ = tx.send(Event::StreamLine {
                            tag: tag.clone(),
                            line,
                        });
                    }
                    Err(_) => break,
                }
            }
        }
        let (code, stderr) = match watched.lock() {
            Ok(mut child) => (
                child.wait().ok().and_then(|s| s.code()),
                stderr_thread.join().unwrap_or_default(),
            ),
            Err(_) => (None, String::new()),
        };
        let _ = tx.send(Event::StreamEnd {
            tag,
            code,
            stderr,
            stopped: false,
        });
    });
    Ok(child)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn captures_output_and_times_out() {
        let dir = std::env::temp_dir().join(format!("iocc-test-{}", std::process::id()));
        let path = script(
            &dir,
            "iocc-t",
            "#!/bin/sh\necho out\necho err >&2\nexit 3\n",
        );
        let out = run_captured(&path, &[], None, Duration::from_secs(5)).unwrap();
        assert_eq!(
            (out.code, out.stdout.trim(), out.stderr.trim()),
            (3, "out", "err")
        );

        std::fs::write(&path, "#!/bin/sh\nsleep 5\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(run_captured(&path, &[], None, Duration::from_millis(100)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stream_forwards_lines_and_end() {
        let dir = std::env::temp_dir().join(format!("iocc-stream-{}", std::process::id()));
        let path = script(&dir, "iocc-s", "#!/bin/sh\necho one\necho two\nexit 2\n");
        let (tx, rx) = channel();
        let child = start_stream("t".into(), &path, &[], None, &tx).unwrap();
        let mut lines = Vec::new();
        let mut end = None;
        for _ in 0..10 {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                Event::StreamLine { line, .. } => lines.push(line),
                Event::StreamEnd { code, .. } => {
                    end = code;
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(lines, ["one", "two"]);
        assert_eq!(end, Some(2));
        child.lock().unwrap().wait().ok();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
