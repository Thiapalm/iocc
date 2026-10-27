//! Client for the device broker exposed by a running `iocc tui`.
//!
//! The TUI owns the MCP2221 and listens on a Unix socket (the path is also
//! exported to plugins as `IOCC_SOCKET`). Plugins use this crate to talk to the
//! same chip while the TUI keeps it open, without reimplementing the JSON
//! protocol.
//!
//! ```no_run
//! let mut bus = iocc_bus::Bus::connect()?;
//! let scan = bus.scan()?;
//! println!("{} {:?}", scan.info, scan.devices);
//! # Ok::<(), iocc_bus::Error>(())
//! ```

use serde::Deserialize;
#[cfg(unix)]
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Protocol version answered by `ping`
pub const PROTOCOL: u32 = 1;

/// Broker path: `IOCC_SOCKET`, then `$XDG_RUNTIME_DIR/iocc.sock`
pub fn socket_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("IOCC_SOCKET").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|p| !p.is_empty())
        .map(|dir| PathBuf::from(dir).join("iocc.sock"))
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// The broker rejected the request (bus or device error)
    Device(String),
    /// No `IOCC_SOCKET` and no `XDG_RUNTIME_DIR`
    NoSocket,
    /// The broker is only implemented on Unix
    Unsupported,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "broker I/O: {e}"),
            Error::Json(e) => write!(f, "invalid broker response: {e}"),
            Error::Device(e) => f.write_str(e),
            Error::NoSocket => {
                f.write_str("no broker socket: start `iocc tui` or set IOCC_SOCKET/XDG_RUNTIME_DIR")
            }
            Error::Unsupported => f.write_str("the device broker is only available on Unix"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

/// Answer of the `ping` operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ping {
    pub protocol: u32,
    pub version: String,
}

/// Answer of the `scan` operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub info: String,
    pub devices: Vec<u8>,
}

/// One JSON object per line, matching the broker protocol
#[derive(Debug, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub ok: bool,
    pub id: Option<u64>,
    pub error: Option<String>,
    pub info: Option<String>,
    #[serde(default)]
    pub data: Vec<u8>,
    #[serde(default)]
    pub devices: Vec<u8>,
    pub value: Option<bool>,
    pub protocol: Option<u32>,
    pub version: Option<String>,
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::os::unix::net::UnixStream;

    /// Connection to a running TUI broker. Requests are serialized: send one
    /// line and read one line back before the next request.
    pub struct Bus {
        stream: UnixStream,
        reader: BufReader<UnixStream>,
        next_id: u64,
    }

    impl Bus {
        /// Connects to the broker found through [`socket_path`]
        pub fn connect() -> Result<Bus, Error> {
            let path = socket_path().ok_or(Error::NoSocket)?;
            Bus::connect_path(path)
        }

        /// Connects to an explicit socket path
        pub fn connect_path(path: impl AsRef<Path>) -> Result<Bus, Error> {
            let path = path.as_ref();
            let stream = UnixStream::connect(path).map_err(|e| {
                Error::Io(std::io::Error::new(
                    e.kind(),
                    format!("cannot connect to {}: {e}", path.display()),
                ))
            })?;
            let reader = BufReader::new(stream.try_clone()?);
            Ok(Bus {
                stream,
                reader,
                next_id: 0,
            })
        }

        fn call(&mut self, op: &str, fields: Value) -> Result<Response, Error> {
            self.next_id += 1;
            let mut object = serde_json::Map::new();
            object.insert("id".into(), self.next_id.into());
            object.insert("op".into(), op.into());
            if let Value::Object(extra) = fields {
                object.extend(extra);
            }
            let mut line = serde_json::to_string(&Value::Object(object))?;
            line.push('\n');
            self.stream.write_all(line.as_bytes())?;
            self.stream.flush()?;

            let mut reply = String::new();
            if self.reader.read_line(&mut reply)? == 0 {
                return Err(Error::Io(std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "broker closed the connection",
                )));
            }
            let response: Response = serde_json::from_str(reply.trim())?;
            if !response.ok {
                return Err(Error::Device(
                    response
                        .error
                        .unwrap_or_else(|| "broker rejected the request".into()),
                ));
            }
            Ok(response)
        }

        /// Checks that the peer speaks this protocol
        pub fn ping(&mut self) -> Result<Ping, Error> {
            let r = self.call("ping", json!({}))?;
            Ok(Ping {
                protocol: r.protocol.unwrap_or(0),
                version: r.version.unwrap_or_default(),
            })
        }

        /// Checks the bus and returns the MCP2221 information
        pub fn status(&mut self) -> Result<String, Error> {
            let r = self.call("status", json!({}))?;
            r.info
                .ok_or_else(|| Error::Device("broker did not return the device info".into()))
        }

        /// Probes the bus; also returns the device info
        pub fn scan(&mut self) -> Result<Scan, Error> {
            let r = self.call("scan", json!({}))?;
            Ok(Scan {
                info: r.info.unwrap_or_default(),
                devices: r.devices,
            })
        }

        /// Information about one I2C device (the broker runs a bus check)
        pub fn info(&mut self, address: u8) -> Result<String, Error> {
            let r = self.call("info", json!({"address": address}))?;
            r.info
                .ok_or_else(|| Error::Device("broker did not return the device info".into()))
        }

        /// Reads `count` bytes after writing `register`
        pub fn read(&mut self, address: u8, register: u8, count: usize) -> Result<Vec<u8>, Error> {
            if !(1..=256).contains(&count) {
                return Err(Error::Device(format!(
                    "count must be between 1 and 256, got {count}"
                )));
            }
            let r = self.call(
                "read",
                json!({"address": address, "register": register, "count": count}),
            )?;
            Ok(r.data)
        }

        /// Writes `data` after `register`
        pub fn write(&mut self, address: u8, register: u8, data: &[u8]) -> Result<(), Error> {
            self.call(
                "write",
                json!({"address": address, "register": register, "data": data}),
            )?;
            Ok(())
        }

        /// Reads one of the four GPIO pins (0-3)
        pub fn gpio_read(&mut self, pin: u8) -> Result<bool, Error> {
            let r = self.call("gpio_read", json!({"pin": pin}))?;
            r.value
                .ok_or_else(|| Error::Device("broker did not return the pin value".into()))
        }

        /// Drives one of the four GPIO pins (0-3) as an output
        pub fn gpio_write(&mut self, pin: u8, value: bool) -> Result<(), Error> {
            self.call("gpio_write", json!({"pin": pin, "value": value}))?;
            Ok(())
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;

    /// Stub kept for API compatibility: the broker only exists on Unix
    pub struct Bus {
        _private: (),
    }

    impl Bus {
        pub fn connect() -> Result<Bus, Error> {
            Err(Error::Unsupported)
        }

        pub fn connect_path(_path: impl AsRef<Path>) -> Result<Bus, Error> {
            Err(Error::Unsupported)
        }

        pub fn ping(&mut self) -> Result<Ping, Error> {
            Err(Error::Unsupported)
        }

        pub fn status(&mut self) -> Result<String, Error> {
            Err(Error::Unsupported)
        }

        pub fn scan(&mut self) -> Result<Scan, Error> {
            Err(Error::Unsupported)
        }

        pub fn info(&mut self, _address: u8) -> Result<String, Error> {
            Err(Error::Unsupported)
        }

        pub fn read(
            &mut self,
            _address: u8,
            _register: u8,
            _count: usize,
        ) -> Result<Vec<u8>, Error> {
            Err(Error::Unsupported)
        }

        pub fn write(&mut self, _address: u8, _register: u8, _data: &[u8]) -> Result<(), Error> {
            Err(Error::Unsupported)
        }

        pub fn gpio_read(&mut self, _pin: u8) -> Result<bool, Error> {
            Err(Error::Unsupported)
        }

        pub fn gpio_write(&mut self, _pin: u8, _value: bool) -> Result<(), Error> {
            Err(Error::Unsupported)
        }
    }
}

pub use imp::Bus;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_can_be_overridden() {
        // SAFETY: this is the only test that changes these variables
        unsafe { std::env::set_var("IOCC_SOCKET", "/tmp/iocc-bus-test.sock") };
        assert_eq!(
            socket_path().unwrap(),
            PathBuf::from("/tmp/iocc-bus-test.sock")
        );
        unsafe { std::env::remove_var("IOCC_SOCKET") };
    }

    #[cfg(unix)]
    #[test]
    fn round_trips_requests() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let path = std::env::temp_dir().join(format!("iocc-bus-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            for _ in 0..5 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(line.trim()).unwrap();
                let id = request["id"].clone();
                let reply = match request["op"].as_str().unwrap() {
                    "ping" => {
                        json!({"id": id, "ok": true, "protocol": PROTOCOL, "version": "test"})
                    }
                    "read" => json!({"id": id, "ok": true, "data": [1, 2]}),
                    "write" | "gpio_write" => json!({"id": id, "ok": true}),
                    "gpio_read" => json!({"id": id, "ok": true, "value": true}),
                    _ => json!({"id": id, "ok": false, "error": "nope"}),
                };
                writeln!(writer, "{reply}").unwrap();
            }
        });

        let mut bus = Bus::connect_path(&path).unwrap();
        let ping = bus.ping().unwrap();
        assert_eq!(
            ping,
            Ping {
                protocol: PROTOCOL,
                version: "test".into()
            }
        );
        assert_eq!(bus.read(0x4A, 2, 2).unwrap(), [1, 2]);
        bus.write(0x4A, 2, &[3]).unwrap();
        assert!(bus.gpio_read(0).unwrap());
        bus.gpio_write(0, true).unwrap();
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
