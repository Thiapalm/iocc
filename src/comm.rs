use embedded_hal::blocking::i2c::{Read, Write};
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
 * Checks the bus and returns the device information.
 * Inputs:
 *   dev - mcp2221 handle
 *
 * Output:
 *   The device information or the error if the bus or the device cannot be queried
 */
fn check_device(dev: &mut mcp2221::Handle) -> Result<String, mcp2221::Error> {
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
    println!("{}", check_device(dev)?);

    for base_address in (0..=127u8).step_by(16) {
        for offset in 0..=15 {
            let address = base_address + offset;
            if !(0x03..=0x77).contains(&address) {
                print!("    ");
                continue;
            }
            match dev.read(address, &mut [0u8]) {
                Ok(_) => print!("0x{:02x}", address),
                Err(_) => print!(" -- "),
            }
        }
        println!();
    }
    Ok(())
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
