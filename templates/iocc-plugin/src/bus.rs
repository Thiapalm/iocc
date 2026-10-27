//! I2C transport used by the plugin.
//!
//! `iocc` can hand the MCP2221 to the plugin in two ways:
//!
//! - **direct**: on the command line (`iocc mydev read`) the plugin opens the
//!   USB device itself, exactly like any other `iocc` subcommand;
//! - **broker**: views and actions declared as `"device": "shared"` receive
//!   `IOCC_SOCKET` and leave the chip to the running TUI. The same socket lets
//!   other plugins use the bus while this one keeps sampling.
//!
//! The [`iocc_bus`] crate implements the broker protocol, so this file only
//! chooses the transport and hides the difference behind `read`/`write`.

use std::time::Duration;

use embedded_hal::blocking::i2c::Write;

/// Errors from the broker, the USB device or a validation check
pub type Error = Box<dyn std::error::Error>;
pub type Result<T> = std::result::Result<T, Error>;

pub enum Bus {
    /// Through the TUI broker (`IOCC_SOCKET`)
    #[cfg_attr(not(unix), allow(dead_code))]
    Broker(iocc_bus::Bus),
    /// Direct access to the MCP2221
    Local(mcp2221::Handle),
}

impl Bus {
    /// Opens the broker when the TUI exported `IOCC_SOCKET`, otherwise opens the
    /// first MCP2221 found.
    pub fn open(speed: u32, reset_on_open: bool, timeout_ms: u64) -> Result<Bus> {
        #[cfg(unix)]
        if let Some(path) = std::env::var_os("IOCC_SOCKET").filter(|path| !path.is_empty()) {
            // Do not fall back to the USB device here: the TUI owns it and the
            // open would fail with "resource busy". A missing socket is an error.
            return Ok(Bus::Broker(iocc_bus::Bus::connect_path(path)?));
        }

        let mut config = mcp2221::Config::default();
        config.i2c_speed_hz = speed;
        config.reset_on_open = reset_on_open;
        config.timeout = Duration::from_millis(timeout_ms);
        Ok(Bus::Local(mcp2221::Handle::open_first(&config)?))
    }

    /// Human readable transport name, shown by the status view
    pub fn transport(&self) -> &'static str {
        match self {
            Bus::Broker(_) => "broker (shared with the TUI)",
            Bus::Local(_) => "direct (MCP2221 opened by the plugin)",
        }
    }

    /// Reads `count` bytes after writing `register`
    pub fn read(&mut self, address: u8, register: u8, count: usize) -> Result<Vec<u8>> {
        match self {
            Bus::Broker(bus) => Ok(bus.read(address, register, count)?),
            Bus::Local(device) => {
                let mut data = vec![0; count];
                device.write_read_address(address, &[register], &mut data)?;
                Ok(data)
            }
        }
    }

    /// Writes `data` after `register`
    pub fn write(&mut self, address: u8, register: u8, data: &[u8]) -> Result<()> {
        match self {
            Bus::Broker(bus) => Ok(bus.write(address, register, data)?),
            Bus::Local(device) => {
                let mut payload = Vec::with_capacity(data.len() + 1);
                payload.push(register);
                payload.extend_from_slice(data);
                device.write(address, &payload)?;
                Ok(())
            }
        }
    }

    /// Reads a big endian `u16`. Change the byte order here (or in
    /// `src/device.rs`) when your chip is little endian.
    pub fn read_u16(&mut self, address: u8, register: u8) -> Result<u16> {
        let data = self.read(address, register, 2)?;
        let [high, low] = data.as_slice() else {
            return Err(format!(
                "register 0x{register:02X} returned {} bytes, expected 2",
                data.len()
            )
            .into());
        };
        Ok(u16::from_be_bytes([*high, *low]))
    }

    pub fn write_u16(&mut self, address: u8, register: u8, value: u16) -> Result<()> {
        self.write(address, register, &value.to_be_bytes())
    }

    /// Checks the bus and returns the MCP2221 information
    pub fn status(&mut self) -> Result<String> {
        match self {
            Bus::Broker(bus) => Ok(bus.status()?),
            Bus::Local(device) => {
                device.check_bus()?;
                Ok(device.get_device_info()?.to_string())
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_missing_broker_reports_the_socket_path() {
        // SAFETY: this is the only test that touches IOCC_SOCKET
        unsafe { std::env::set_var("IOCC_SOCKET", "/tmp/iocc-template-missing.sock") };
        let error = Bus::open(400_000, false, 10).err().unwrap().to_string();
        unsafe { std::env::remove_var("IOCC_SOCKET") };
        assert!(error.contains("iocc-template-missing.sock"), "{error}");
    }
}
