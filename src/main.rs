use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use iocc_bus::Bus;

use std::process::exit;
mod comm;
use comm::*;

mod config;
use config::*;

mod plugin;
use plugin::*;

mod tui;
mod udev;

#[derive(Parser)]
#[command(name = "iocc", version, about = "IOExtender Control Center, MCP2221 communication control via plugins", long_about = None)]
struct Cli {
    /// Use the device broker of a running `iocc tui` instead of opening the device
    #[arg(long, global = true)]
    via_tui: bool,
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
    /// Open the interactive terminal interface (plugins can add their own panels)
    Tui,
    /// Scan Bus for connected devices (probes each address with a 1 byte read)
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
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
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
        Some(Commands::Tui) => {
            if cli.via_tui {
                eprintln!("Error: --via-tui cannot be used with `tui`");
                exit(2);
            }
            if let Err(err) = tui::run(&config) {
                eprintln!("Error: {err}");
                exit(1);
            }
        }
        Some(Commands::Status) if cli.via_tui => {
            let mut bus = connect_broker();
            match bus.status() {
                Ok(info) => println!("Status: Device Functional.\n{info}"),
                Err(err) => {
                    eprintln!("Error: {err}");
                    exit(1);
                }
            }
        }
        Some(Commands::Status) => {
            if let Err(err) = device_status(&mut open_device()) {
                eprintln!("Error: {err}");
                exit(1);
            }
        }
        Some(Commands::Scan) if cli.via_tui => {
            let mut bus = connect_broker();
            match bus.scan() {
                Ok(scan) => print!("{}", scan_report(&scan.info, &scan.devices)),
                Err(err) => {
                    eprintln!("Error: {err}");
                    exit(1);
                }
            }
        }
        Some(Commands::Scan) => {
            if let Err(err) = scan_bus(&mut open_device()) {
                eprintln!("Error: {err}");
                exit(1);
            }
        }
        Some(Commands::External(args)) => {
            if cli.via_tui {
                eprintln!(
                    "Error: --via-tui only applies to status/scan/read/write; \
                     plugins use the broker through IOCC_SOCKET"
                );
                exit(2);
            }
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
            if cli.via_tui {
                let mut bus = connect_broker();
                if let Err(err) = bus.write(*address, *register, data) {
                    eprintln!("Error: {err}");
                    exit(1);
                }
            } else if let Err(err) = write_device(&mut open_device(), *address, *register, data) {
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
            let result = if cli.via_tui {
                connect_broker()
                    .read(*address, *register, *expected as usize)
                    .map_err(|e| e.to_string())
            } else {
                read_device(&mut open_device(), *address, *register, *expected as usize)
                    .map_err(|e| e.to_string())
            };
            match result {
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

/**
 * Opens the broker of a running TUI, exiting with a clear message when it is
 * unreachable.
 */
fn connect_broker() -> Bus {
    match Bus::connect() {
        Ok(bus) => bus,
        Err(err) => {
            eprintln!("Error: {err}");
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_u8_hex("0x10"), Ok(0x10));
        assert_eq!(parse_u8_hex("0X1f"), Ok(0x1f));
        assert_eq!(parse_u8_hex("FF"), Ok(0xFF));
        assert!(parse_u8_hex("0x0x10").is_err());
        assert!(parse_u8_hex("100").is_err());
    }

    #[test]
    fn i2c_address_range() {
        assert_eq!(parse_i2c_address("7F"), Ok(0x7F));
        assert!(parse_i2c_address("80").is_err());
    }

    #[test]
    fn broker_flag_is_opt_in() {
        let matches = build_cli(false)
            .try_get_matches_from(["iocc", "scan", "--via-tui"])
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        assert!(cli.via_tui);
        assert!(matches!(cli.command, Some(Commands::Scan)));

        let matches = build_cli(false)
            .try_get_matches_from(["iocc", "scan"])
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        assert!(!cli.via_tui);
    }

    #[test]
    fn scan_report_keeps_the_grid() {
        let report = scan_report("device info", &[0x20, 0x4A]);
        assert!(report.starts_with("device info\n"));
        assert_eq!(report.lines().count(), 9);
        assert!(report.contains("0x20") && report.contains("0x4a"));
    }
}
