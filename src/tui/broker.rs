use super::backend::SharedDevice;
use crate::comm::{check_device, read_device, write_device};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

#[derive(Deserialize)]
struct Request {
    id: Option<u64>,
    op: String,
    address: Option<u8>,
    register: Option<u8>,
    count: Option<usize>,
    data: Option<Vec<u8>>,
}

/**
 * Socket the TUI listens on for plugins that share the device. Plugins receive
 * the path through IOCC_SOCKET. An explicit IOCC_SOCKET wins, which also lets a
 * client be pointed at a running TUI.
 */
pub fn socket_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("IOCC_SOCKET").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(dir).join("iocc.sock"));
    }
    // No runtime dir: a per process file in the temp dir, reachable only through IOCC_SOCKET
    Some(std::env::temp_dir().join(format!("iocc-{}.sock", std::process::id())))
}

/**
 * Starts accepting broker clients. Returns the socket path on success.
 */
pub fn start(shared: &Arc<SharedDevice>) -> Option<PathBuf> {
    let path = socket_path()?;
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).ok()?;
    // Only the current user may drive the bus through this socket
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

    let shared = Arc::clone(shared);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = Arc::clone(&shared);
            thread::spawn(move || serve(stream, &shared));
        }
    });
    Some(path)
}

fn serve(stream: UnixStream, shared: &SharedDevice) {
    let reader = match stream.try_clone() {
        Ok(s) => BufReader::new(s),
        Err(_) => return,
    };
    let mut writer = stream;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let response = handle(shared, &line);
        if writeln!(writer, "{response}").is_err() {
            break;
        }
    }
}

fn handle(shared: &SharedDevice, line: &str) -> String {
    let request: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => return serde_json::json!({"ok": false, "error": e.to_string()}).to_string(),
    };
    let id = request.id;
    let add = |mut value: serde_json::Value| {
        if let Some(id) = id {
            value["id"] = id.into();
        }
        value.to_string()
    };

    let address = match request.address {
        Some(a) if a <= 0x7F => a,
        _ => return add(serde_json::json!({"ok": false, "error": "missing or invalid address"})),
    };

    let result: Result<serde_json::Value, String> = match request.op.as_str() {
        "info" => shared
            .with(|dev| check_device(dev).map_err(|e| e.to_string()))
            .map(|info| serde_json::json!({"ok": true, "info": info})),
        "read" => {
            let reg = request.register.unwrap_or(0);
            let count = request.count.unwrap_or(1).clamp(1, 256);
            shared
                .with(|dev| read_device(dev, address, reg, count).map_err(|e| e.to_string()))
                .map(|data| serde_json::json!({"ok": true, "data": data}))
        }
        "write" => {
            let reg = request.register.unwrap_or(0);
            let data = request.data.unwrap_or_default();
            shared
                .with(|dev| write_device(dev, address, reg, &data).map_err(|e| e.to_string()))
                .map(|()| serde_json::json!({"ok": true}))
        }
        other => Err(format!("unknown op '{other}', use info, read or write")),
    };

    match result {
        Ok(mut value) => {
            if let Some(id) = id {
                value["id"] = id.into();
            }
            value.to_string()
        }
        Err(e) => add(serde_json::json!({"ok": false, "error": e})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn invalid_requests_are_rejected_without_touching_the_device() {
        let (tx, _rx) = channel();
        let shared = Arc::new(SharedDevice::new_for_tests(tx));
        let out = handle(&shared, "not json");
        assert!(out.contains("error"));
        let out: serde_json::Value =
            serde_json::from_str(&handle(&shared, r#"{"id":7,"op":"nope","address":32}"#)).unwrap();
        assert_eq!(out["ok"], false);
        assert_eq!(out["id"], 7);
        let out: serde_json::Value =
            serde_json::from_str(&handle(&shared, r#"{"id":8,"op":"read"}"#)).unwrap();
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("address"));
    }

    #[test]
    fn socket_path_can_be_overridden() {
        // SAFETY: no other test reads IOCC_SOCKET
        unsafe { std::env::set_var("IOCC_SOCKET", "/tmp/x.sock") };
        assert_eq!(socket_path().unwrap(), PathBuf::from("/tmp/x.sock"));
        unsafe { std::env::remove_var("IOCC_SOCKET") };
    }
}
