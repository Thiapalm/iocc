use embedded_hal::blocking::i2c::Read;
use std::{process::exit, time::Duration};

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
    let dev = match mcp2221::Handle::open_first(&config) {
        Ok(x) => x,
        Err(err) => {
            return Err(err);
        }
    };
    Ok(dev)
}

/**
 * Scans the Bus checking for connected devices.
 * Inputs:
 *   dev - mcp2221 handle
 *
 * Output:
 *   No output, but a list of found addressess will be sent to stdout
 */
pub fn scan_bus(dev: &mut mcp2221::Handle) {
    match dev.check_bus() {
        Ok(()) => (),
        Err(x) => {
            println!("Error: {x}");
            exit(1);
        }
    }

    println!("{}", dev.get_device_info().unwrap());

    for base_address in (0..=127).step_by(16) {
        for offset in 0..=15 {
            let address = base_address + offset;
            match dev.read(address, &mut [0u8]) {
                Ok(_) => print!("0x{:02x}", address),
                Err(_) => print!(" -- "),
            }
        }
        println!();
    }
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
    data: &mut Vec<u8>,
) -> Result<(), mcp2221::Error> {
    let mut my_data = vec![register];
    my_data.append(data);
    dev.write_read_address(address, &mut my_data, &mut [0u8])
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
    //println!("addr: {:#02x} reg {:#02x}", addr, register);
    let mut read = vec![0; expected];
    let mut reg: [u8; 1] = [0; 1];
    reg[0] = register;
    match dev.write_read_address(address, &mut reg, &mut read) {
        Ok(()) => Ok(read),
        Err(x) => Err(x),
    }
}
