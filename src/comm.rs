use embedded_hal::blocking::i2c::Read;
use std::env;
use std::fs;
use std::path::Path;
use std::{process::exit, thread::sleep, time::Duration};

pub fn start_device() -> Result<mcp2221::Handle, mcp2221::Error> {
    let mut config = mcp2221::Config::default();
    config.i2c_speed_hz = 400_000;
    config.reset_on_open = false;
    // For talking to a peripheral we might want a higher timeout, but for
    // scanning the bus, a short timeout is good since it allows us to scan all
    // addresses more quickly.
    config.timeout = Duration::from_millis(10);
    let dev = match mcp2221::Handle::open_first(&config) {
        Ok(x) => x,
        Err(err) => {
            return Err(err);
        }
    };
    Ok(dev)
}

// fn read_register(dev: &mut mcp2221::Handle, address: u8, reg: Register, port: Port) -> u8 {
//     let mut read: [u8; 1] = [0; 1];
//     let mut result: [u8; 1] = [0; 1];
//     read[0] = reg as u8 | port as u8;
//     let _ = dev.write_read_address(address, &mut read, &mut result);

//     result[0]
// }

// fn write_register(dev: &mut mcp2221::Handle, address: u8, port: Port, reg: Register, value: u8) {
//     let mut result: [u8; 2] = [0; 2];
//     result[0] = reg as u8 | port as u8;
//     result[1] = value;
//     match dev.write_read_address(address, &mut result, &mut [0u8]) {
//         Ok(()) => (),
//         Err(x) => println!("Error: {}", x),
//     }
// }

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
