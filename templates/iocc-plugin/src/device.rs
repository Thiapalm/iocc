//! Register map of the example device.
//!
//! Replace the constants and the scaling below with your chip. The rest of the
//! template only calls these functions, so the CLI, the manifest and the
//! configuration keep working after the adaptation.
//!
//! The imaginary device here has 16 bit big endian registers:
//!
//! | Register | Name        | Meaning                         |
//! |----------|-------------|---------------------------------|
//! | `0x00`   | `CHIP_ID`   | fixed `0x4D59`                  |
//! | `0x01`   | `CONFIG`    | bits 1:0 select the gain        |
//! | `0x02`   | `VOLTAGE`   | 1 mV per LSB                    |
//! | `0x03`   | `CURRENT`   | 1 mA per LSB, signed            |
//! | `0x04`   | `TEMPERATURE` | 1/128 °C per LSB, signed      |
//! | `0x05`   | `THRESHOLD` | 1 mV per LSB, alert level       |

use crate::bus::{Bus, Result};
use crate::config::Settings;

pub const CHIP_ID: u16 = 0x4D59;
pub const REG_CHIP_ID: u8 = 0x00;
pub const REG_CONFIG: u8 = 0x01;
pub const REG_VOLTAGE: u8 = 0x02;
pub const REG_CURRENT: u8 = 0x03;
pub const REG_TEMPERATURE: u8 = 0x04;
pub const REG_THRESHOLD: u8 = 0x05;

/// Gains offered by the `Set gain` action
pub const GAINS: [u8; 4] = [1, 2, 4, 8];

/// One sample in SI units
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    pub voltage: f64,
    pub current: f64,
    pub temperature: f64,
}

/// Checks the chip ID, so a wrong address fails with a clear message instead of
/// returning random readings.
pub fn identify(bus: &mut Bus, address: u8) -> Result<u16> {
    let id = bus.read_u16(address, REG_CHIP_ID)?;
    if id != CHIP_ID {
        return Err(format!(
            "unexpected chip id 0x{id:04X} at {address:#04X} (expected 0x{CHIP_ID:04X})"
        )
        .into());
    }
    Ok(id)
}

/// Reads the configuration register
pub fn config(bus: &mut Bus, address: u8) -> Result<u16> {
    bus.read_u16(address, REG_CONFIG)
}

/// Decodes the gain from the configuration register
pub fn gain_from_config(config: u16) -> u8 {
    let index = (config & 0x0003).min(GAINS.len() as u16 - 1) as usize;
    GAINS[index]
}

/// Reads voltage, current and temperature in one pass
pub fn reading(bus: &mut Bus, address: u8) -> Result<Reading> {
    let voltage = bus.read_u16(address, REG_VOLTAGE)? as f64 / 1000.0;
    let current = bus.read_u16(address, REG_CURRENT)? as i16 as f64 / 1000.0;
    let temperature = bus.read_u16(address, REG_TEMPERATURE)? as i16 as f64 / 128.0;
    Ok(Reading {
        voltage,
        current,
        temperature,
    })
}

/// Applies the saved settings when a command starts the device. A value changed
/// with `--no-save` is therefore temporary: the next run restores the file.
pub fn apply(bus: &mut Bus, address: u8, settings: &Settings) -> Result<()> {
    set_gain(bus, address, settings.gain)?;
    set_threshold(bus, address, settings.threshold)
}

/// Sets the gain, keeping the other configuration bits
pub fn set_gain(bus: &mut Bus, address: u8, gain: u8) -> Result<()> {
    let register = gain_register(gain)?;
    let config = bus.read_u16(address, REG_CONFIG)?;
    bus.write_u16(address, REG_CONFIG, (config & !0x0003) | register)
}

fn gain_register(gain: u8) -> Result<u16> {
    GAINS
        .iter()
        .position(|value| *value == gain)
        .map(|index| index as u16)
        .ok_or_else(|| format!("gain must be one of {GAINS:?}, got {gain}").into())
}

/// Sets the alert threshold in volts
pub fn set_threshold(bus: &mut Bus, address: u8, threshold: f64) -> Result<()> {
    if !threshold.is_finite() || !(0.0..=65.535).contains(&threshold) {
        return Err(format!("threshold must be between 0 and 65.535 V, got {threshold}").into());
    }
    bus.write_u16(address, REG_THRESHOLD, (threshold * 1000.0).round() as u16)
}

/// Reads the alert threshold in volts
pub fn threshold(bus: &mut Bus, address: u8) -> Result<f64> {
    Ok(bus.read_u16(address, REG_THRESHOLD)? as f64 / 1000.0)
}

/// Restores the power-on values of the configuration and threshold registers
pub fn reset(bus: &mut Bus, address: u8) -> Result<()> {
    bus.write_u16(address, REG_CONFIG, 0x0000)?;
    bus.write_u16(address, REG_THRESHOLD, 2000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_round_trips_through_the_config_register() {
        for gain in GAINS {
            let register = gain_register(gain).unwrap();
            assert_eq!(gain_from_config(0xFF00 | register), gain);
        }
    }

    #[test]
    fn unknown_gain_is_rejected() {
        let error = gain_register(3).unwrap_err().to_string();
        assert!(error.contains("gain must be one of"), "{error}");
    }
}
