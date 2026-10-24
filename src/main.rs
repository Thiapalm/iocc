use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

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
        /// Slave Address, 7bit format (Hexadecimal 0x10, 7F, etc.)
        #[arg(short, long, value_parser = parse_i2c_address)]
        address: u8,
        /// Slave Register to write to (Hexadecimal 0x10, FF, etc.)
        #[arg(short, long, value_parser = parse_u8_hex)]
        register: u8,
        /// Data to write (hexadecimal space separated if more than one and up to 60 bytes)
        #[arg(short, long, value_parser = parse_u8_hex, num_args = 1..=60, required = true)]
        data: Vec<u8>,
    },
    /// Read from device
    Read {
        /// Slave Address (7bit format)
        #[arg(short, long, value_parser = parse_i2c_address)]
        address: u8,
        /// Slave Register to write to
        #[arg(short, long, value_parser = parse_u8_hex)]
        register: u8,
        /// Number of bytes to read (1 to 65535)
        #[arg(short, long, value_parser = clap::value_parser!(u16).range(1..))]
        expected: u16,
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

/**
 * Parses a 7bit I2C address (0x00 to 0x7F), used as clap value_parser
 */
fn parse_i2c_address(s: &str) -> Result<u8, String> {
    let address = parse_u8_hex(s).map_err(|e| e.to_string())?;
    if address > 0x7F {
        return Err(format!("{address:#04X} is not a 7bit address (max 0x7F)"));
    }
    Ok(address)
}

/**
 * Builds the clap command, appending the detected plugins to the help text.
 * Plugins are only searched when help is requested.
 */
fn build_cli(with_plugins: bool) -> clap::Command {
    let cmd = Cli::command();
    if with_plugins {
        cmd.after_help(plugins_help())
    } else {
        cmd
    }
}

fn main() {
    let help_requested = std::env::args().any(|a| matches!(a.as_str(), "-h" | "--help" | "help"));
    let matches = build_cli(help_requested).get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

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

    // Device is opened only by commands that need it, never for plugins (they open it themselves)
    let open_device = || match start_device(
        config.get_speed(),
        config.is_reset_on_open(),
        config.get_timeout(),
    ) {
        Ok(dev) => dev,
        Err(err) => {
            eprintln!("Error: {err}, check USB connection");
            exit(1);
        }
    };

    //process received command
    match &cli.command {
        Some(Commands::SetupUdev) => unreachable!("handled before device start"),
        Some(Commands::Status) => {
            device_status(&mut open_device());
        }
        Some(Commands::Scan) => {
            scan_bus(&mut open_device());
        }
        Some(Commands::External(args)) => {
            if let Some(cmd) = args.first() {
                exit(run_plugin(cmd, &args[1..], &config));
            } else {
                eprintln!("Unidentified external command.");
                exit(1);
            }
        }
        Some(Commands::Write {
            address,
            register,
            data,
        }) => {
            if let Err(err) = write_device(&mut open_device(), *address, *register, data) {
                eprintln!("Error: {err}");
                exit(1);
            }
        }
        Some(Commands::Read {
            address,
            register,
            expected,
        }) => {
            if config.is_verbose() {
                println!("[host] Reading device: {}", *address);
            }
            match read_device(&mut open_device(), *address, *register, *expected as usize) {
                Ok(data) => {
                    for byte in data.iter() {
                        print!("{:#04X} ", byte);
                    }
                    println!();
                }
                Err(err) => {
                    eprintln!("Error: {err}");
                    exit(1);
                }
            }
        }
        None => {
            build_cli(true).print_help().unwrap();
        }
    }
}
