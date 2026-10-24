use clap::{CommandFactory, Parser, Subcommand};

use std::process::exit;
mod comm;
use comm::*;

mod config;
use config::*;

mod plugin;
use plugin::*;

mod udev;

#[derive(Parser)]
#[command(name = "iocc", version, about = "IOExtender Control Center, MCP2221 communication control via plugins", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/**
 * Clap command enum list
 */
#[derive(Subcommand)]
enum Commands {
    /// Show local Status
    Status,
    /// Install the Linux udev rule for USB access without root (run with sudo)
    SetupUdev,
    /// Scan Bus for connected devices
    Scan,
    /// Write to device
    Write {
        /// Slave Address (Hexadecimal 0x10, FF, etc.)
        #[arg(short, long, value_parser = parse_u8_hex)]
        address: u8,
        /// Slave Register to write to (Hexadecimal 0x10, FF, etc.)
        #[arg(short, long, value_parser = parse_u8_hex)]
        register: u8,
        /// Data to write (hexadecimal space separated if more than one and up to 60 bytes)
        #[arg(short, long, value_parser = parse_u8_hex, num_args = 1.., required = true)]
        data: Vec<u8>,
    },
    /// Read from device
    Read {
        /// Slave Address (7bit format)
        #[arg(short, long, value_parser = parse_u8_hex)]
        address: u8,
        /// Slave Register to write to
        #[arg(short, long, value_parser = parse_u8_hex)]
        register: u8,
        /// Number of bytes to read
        #[arg(short, long)]
        expected: usize,
    },
    /// Runs Plugin
    #[command(external_subcommand)]
    External(Vec<String>),
}

/**
 * Parses command received Hex data into u8, used as clap value_parser
 */
fn parse_u8_hex(s: &str) -> Result<u8, std::num::ParseIntError> {
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    u8::from_str_radix(s, 16)
}

fn main() {
    // Collects raw arguments received and verifies if command 'help' is given, if so, searches
    // for available plugins and appends the list into clap help
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() >= 2 {
        let command = &raw_args[1];
        let plugins = list_plugins();
        if !plugins.contains(command) {
            // if 'help', print clap help and
            if raw_args.iter().any(|arg| arg == "--help" || arg == "-h") {
                let mut cmd = Cli::command();
                cmd.print_help().unwrap();

                // Append any found plugin
                let plugins = list_plugins();
                if !plugins.is_empty() {
                    println!("\nDetected Plugins:");
                    for plugin in plugins {
                        println!("  {}", plugin);
                    }
                } else {
                    println!("\nNo Plugin detected.");
                }

                return; // Terminate the execution
            }
        }
    }

    // clap parsing
    let cli = Cli::parse();

    // Handled before config/device access, since it must work without USB permissions
    if let Some(Commands::SetupUdev) = &cli.command {
        if let Err(err) = udev::setup_udev() {
            eprintln!("Error: {err}");
            exit(1);
        }
        return;
    }

    // config file parsing
    let config = read_config();
    if config.is_verbose() {
        println!(
            "[host] Speed: {} Reset: {} timeout: {}",
            config.get_speed(),
            config.is_reset_on_open(),
            config.get_timeout()
        );
    }

    // starts device
    let mut i2cdev = match start_device(
        config.get_speed(),
        config.is_reset_on_open(),
        config.get_timeout(),
    ) {
        Ok(x) => x,
        Err(err) => {
            println!("Error: {err}, check USB connection");
            exit(1);
        }
    };

    //process received command
    match &cli.command {
        Some(Commands::SetupUdev) => unreachable!("handled before device start"),
        Some(Commands::Status) => {
            println!("Status: Device Functional.");
        }
        Some(Commands::Scan) => {
            scan_bus(&mut i2cdev);
        }
        Some(Commands::External(args)) => {
            if !args.is_empty() {
                let cmd = &args[0];
                run_plugin(cmd, &args[1..], &config);
            } else {
                eprintln!("Unindentified external command.");
            }
        }
        Some(Commands::Write {
            address,
            register,
            data,
        }) => match write_device(&mut i2cdev, *address, *register, &mut data.clone()) {
            Ok(()) => (),
            Err(x) => println!("Error: {}", x),
        },
        Some(Commands::Read {
            address,
            register,
            expected,
        }) => {
            if config.is_verbose() {
                println!("[host] Reading device: {}", *address);
            }
            let data = read_device(&mut i2cdev, *address, *register, *expected);
            match data {
                Ok(data) => {
                    for byte in data.iter() {
                        print!("{:#04X} ", byte);
                    }
                }
                Err(x) => println!("Error: {}", x),
            }
        }
        None => {
            let mut cmd = Cli::command();
            cmd.print_help().unwrap();

            // Listar plugins quando nenhum comando específico for fornecido
            let plugins = list_plugins();
            if !plugins.is_empty() {
                println!("\nDetected Plugins:");
                for plugin in plugins {
                    println!("  {}", plugin);
                }
            } else {
                println!("\nNo Plugin detected.");
            }
        }
    }
}
