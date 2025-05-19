use embedded_hal::blocking::i2c::Read;
use std::{process::exit, time::Duration};

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
