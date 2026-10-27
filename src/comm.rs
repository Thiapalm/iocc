use embedded_hal::blocking::i2c::{Read, Write};
#[cfg(unix)]
use mcp2221::{Direction, PinState};
use std::time::Duration;

/**
 * Initializes the I2C connection towards a given device.
 * Inputs:
 *   speed - Bus speed in Hz (ex. 400000 Hz)
 *   reset - Reset the chip when starting communication or not
 *   timeout - communication timeout in milliseconds (ex. 10ms)
 *
 * Output:
 *   Error code if there was a problem od a mcp2221 Handle
 */
pub fn start_device(
    speed: u32,
    reset: bool,
    timeout: u64,
) -> Result<mcp2221::Handle, mcp2221::Error> {
    let mut config = mcp2221::Config::default();
    config.i2c_speed_hz = speed;
    config.reset_on_open = reset;
    config.timeout = Duration::from_millis(timeout);
    mcp2221::Handle::open_first(&config)
}

/**
 * Probes every non reserved address (0x03-0x77) with a 1 byte read and returns
 * the addresses that answered.
 */
pub fn scan_addresses(dev: &mut mcp2221::Handle) -> Vec<u8> {
    (0x03..=0x77u8)
        .filter(|&address| dev.read(address, &mut [0u8]).is_ok())
        .collect()
}

/**
 * Checks the bus and returns the device information.
 * Inputs:
 *   dev - mcp2221 handle
 *
 * Output:
 *   The device information or the error if the bus or the device cannot be queried
 */
pub fn check_device(dev: &mut mcp2221::Handle) -> Result<String, mcp2221::Error> {
    dev.check_bus()?;
    Ok(dev.get_device_info()?.to_string())
}

/**
 * Prints the device status and information
 */
pub fn device_status(dev: &mut mcp2221::Handle) -> Result<(), mcp2221::Error> {
    println!("Status: Device Functional.\n{}", check_device(dev)?);
    Ok(())
}

/**
 * Formats the device information and the address grid shown by a bus scan.
 * Shared by the native `scan` command and by `--via-tui`.
 */
pub fn scan_report(info: &str, found: &[u8]) -> String {
    let mut out = format!("{info}\n");
    for base_address in (0..=127u8).step_by(16) {
        for offset in 0..=15 {
            let address = base_address + offset;
            if !(0x03..=0x77).contains(&address) {
                out.push_str("    ");
            } else if found.contains(&address) {
                out.push_str(&format!("0x{address:02x}"));
            } else {
                out.push_str(" -- ");
            }
        }
        out.push('\n');
    }
    out
}

/**
 * Scans the Bus checking for connected devices.
 * Inputs:
 *   dev - mcp2221 handle
 *
 * Output:
 *   No output value, but a list of found addresses will be sent to stdout. Reserved
 *   addresses (0x00-0x02 and 0x78-0x7F) are not probed. Each address is probed with a
 *   1 byte read, which some devices (EEPROMs, ADCs) may treat as an operation.
 *   Error if the bus or the device cannot be queried
 */
pub fn scan_bus(dev: &mut mcp2221::Handle) -> Result<(), mcp2221::Error> {
    let info = check_device(dev)?;
    let found = scan_addresses(dev);
    print!("{}", scan_report(&info, &found));
    Ok(())
}

/**
 * Reads one of the four MCP2221 GPIO pins.
 * Inputs:
 *   dev - mcp2221 handle
 *   pin - GPIO index (0-3)
 *
 * Output:
 *   true for a high level, false for low, or an error when the pin is not
 *   configured as GPIO.
 */
#[cfg(unix)]
pub fn gpio_read(dev: &mut mcp2221::Handle, pin: usize) -> Result<bool, String> {
    if pin > 3 {
        return Err(format!("GPIO pin must be between 0 and 3, got {pin}"));
    }
    let pins = dev.get_pin_values().map_err(|e| e.to_string())?;
    match [pins.gp0, pins.gp1, pins.gp2, pins.gp3][pin] {
        PinState::High => Ok(true),
        PinState::Low => Ok(false),
        state => Err(format!("GPIO {pin} is not configured as GPIO ({state:?})")),
    }
}

/**
 * Drives one of the four MCP2221 GPIO pins, keeping the configuration of the
 * other pins.
 * Inputs:
 *   dev - mcp2221 handle
 *   pin - GPIO index (0-3)
 *   value - true for high, false for low
 */
#[cfg(unix)]
pub fn gpio_write(dev: &mut mcp2221::Handle, pin: usize, value: bool) -> Result<(), String> {
    if pin > 3 {
        return Err(format!("GPIO pin must be between 0 and 3, got {pin}"));
    }
    let mut config = dev.get_gpio_config().map_err(|e| e.to_string())?;
    config.set_direction(pin, Direction::Output);
    config.set_value(pin, value);
    dev.configure_gpio(&config).map_err(|e| e.to_string())
}

/**
 * Write data to a given device.
 * Inputs:
 *   dev - mcp2221 handle
 *   address - Device address in 7bit format
 *   register - Register that will receive the data
 *   data - Data to write in u8 format
 *
 * Output:
 *   Error code if there was a problem while writing
 */
pub fn write_device(
    dev: &mut mcp2221::Handle,
    address: u8,
    register: u8,
    data: &[u8],
) -> Result<(), mcp2221::Error> {
    let mut payload = Vec::with_capacity(data.len() + 1);
    payload.push(register);
    payload.extend_from_slice(data);
    dev.write(address, &payload)
}

/**
 * Read data from a given device.
 * Inputs:
 *   dev - mcp2221 handle
 *   address - Device address in 7bit format
 *   register - Register that will receive the data
 *   expected - number of bytes to read
 *
 * Output:
 *   Error code if there was a problem while writing or a vector of u8 containing the data
 */
pub fn read_device(
    dev: &mut mcp2221::Handle,
    address: u8,
    register: u8,
    expected: usize,
) -> Result<Vec<u8>, mcp2221::Error> {
    let mut read = vec![0; expected];
    dev.write_read_address(address, &[register], &mut read)?;
    Ok(read)
}
