use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use iocc_bus::Bus;

use std::path::{Path, PathBuf};
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
    /// Require the device broker of a running `iocc tui` (alias: --via-tui)
    ///
    /// By default a live broker is detected and used automatically; this flag
    /// turns that into an error when no broker is running.
    #[arg(long, global = true, visible_alias = "via-tui")]
    broker: bool,
    /// Open the device directly, even when a broker is available
    #[arg(long, global = true, conflicts_with = "broker")]
    direct: bool,
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
    /// Manage the shared config.yaml (used by iocc and its plugins)
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },
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
 * Config subcommands
 */
#[derive(Subcommand)]
enum ConfigCommands {
    /// Merge the sections of a YAML file (e.g. a plugin config example) into the shared config.yaml
    Import {
        /// YAML file with the sections to add
        file: PathBuf,
        /// Replace sections that already exist instead of skipping them
        #[arg(long)]
        force: bool,
    },
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

    // Config editing must also work without USB permissions
    if let Some(Commands::Config { action }) = &cli.command {
        match action {
            ConfigCommands::Import { file, force } => {
                if let Err(err) = import_sections(file, *force) {
                    eprintln!("Error: {err}");
                    exit(1);
                }
            }
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

    if cli.broker && matches!(cli.command, Some(Commands::Tui)) {
        eprintln!("Error: --broker cannot be used with `tui`");
        exit(2);
    }

    // A live broker is used whenever there is one, so the command line and a
    // running TUI share the MCP2221. `--direct` keeps the exclusive access and
    // `--broker` requires a broker instead of falling back to the device.
    let socket = use_broker(&cli);
    if config.is_verbose()
        && let Some(path) = &socket
    {
        println!("[host] Using the device broker at {}", path.display());
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
        Some(Commands::Config { .. }) => unreachable!("handled before device start"),
        Some(Commands::Tui) => {
            if let Err(err) = tui::run(&config) {
                eprintln!("Error: {err}");
                exit(1);
            }
        }
        Some(Commands::Status) => {
            if let Some(path) = &socket {
                match connect_broker(path).status() {
                    Ok(info) => println!("Status: Device Functional.\n{info}"),
                    Err(err) => {
                        eprintln!("Error: {err}");
                        exit(1);
                    }
                }
            } else if let Err(err) = device_status(&mut open_device()) {
                eprintln!("Error: {err}");
                exit(1);
            }
        }
        Some(Commands::Scan) => {
            if let Some(path) = &socket {
                match connect_broker(path).scan() {
                    Ok(scan) => print!("{}", scan_report(&scan.info, &scan.devices)),
                    Err(err) => {
                        eprintln!("Error: {err}");
                        exit(1);
                    }
                }
            } else if let Err(err) = scan_bus(&mut open_device()) {
                eprintln!("Error: {err}");
                exit(1);
            }
        }
        Some(Commands::External(args)) => {
            if let Some(cmd) = args.first() {
                exit(run_plugin_with_socket(
                    cmd,
                    &args[1..],
                    &config,
                    socket.as_deref(),
                ));
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
            if let Some(path) = &socket {
                if let Err(err) = connect_broker(path).write(*address, *register, data) {
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
            let result = if let Some(path) = &socket {
                connect_broker(path)
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
 * Imports the top level sections of `file` into the shared config.yaml and
 * reports what changed. Plugin installers call `iocc config import config.yaml`
 * so their settings are available on the first run.
 */
fn import_sections(file: &Path, force: bool) -> Result<(), String> {
    let report = import_config(file, force)?;
    let path = config_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "config.yaml".to_string());

    if report.added.is_empty() && report.replaced.is_empty() {
        println!(
            "{path} is already up to date; nothing to import from {}",
            file.display()
        );
        return Ok(());
    }
    if !report.added.is_empty() {
        println!("Added to {path}: {}", report.added.join(", "));
    }
    if !report.replaced.is_empty() {
        println!("Replaced in {path}: {}", report.replaced.join(", "));
    }
    if !report.skipped.is_empty() {
        println!("Kept in {path}: {}", report.skipped.join(", "));
    }
    Ok(())
}

/**
 * Broker to use for this invocation. `--direct` disables the detection,
 * `--broker` requires a broker (exiting when none answers) and the default is
 * to use a running TUI when one is found, falling back to direct access.
 */
fn use_broker(cli: &Cli) -> Option<PathBuf> {
    if cli.direct {
        return None;
    }
    let socket = live_broker();
    if cli.broker && socket.is_none() {
        let where_ = iocc_bus::socket_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "IOCC_SOCKET or $XDG_RUNTIME_DIR/iocc.sock".to_string());
        eprintln!("Error: --broker was given but no iocc broker answered at {where_}");
        exit(1);
    }
    if socket.is_none()
        && let Some(path) = std::env::var_os("IOCC_SOCKET").filter(|path| !path.is_empty())
    {
        eprintln!(
            "Warning: IOCC_SOCKET={} does not answer; opening the device directly",
            Path::new(&path).display()
        );
    }
    socket
}

/**
 * Socket of a broker that answers `ping`, so a stale or unrelated
 * `IOCC_SOCKET` is ignored instead of breaking the command.
 */
fn live_broker() -> Option<PathBuf> {
    let path = iocc_bus::socket_path()?;
    probe_broker(&path).then_some(path)
}

/// Whether something at `path` speaks the broker protocol
fn probe_broker(path: &Path) -> bool {
    Bus::connect_path(path)
        .and_then(|mut bus| bus.ping())
        .is_ok()
}

/**
 * Opens the broker at `path`, exiting with a clear message when it closed
 * between the detection and the command.
 */
fn connect_broker(path: &Path) -> Bus {
    match Bus::connect_path(path) {
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
    fn broker_flags_parse_and_conflict() {
        // The broker is automatic, so neither flag is required
        let matches = build_cli(false)
            .try_get_matches_from(["iocc", "scan"])
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        assert!(!cli.broker);
        assert!(!cli.direct);
        assert!(matches!(cli.command, Some(Commands::Scan)));

        // --broker and the --via-tui compatibility alias
        for flag in ["--broker", "--via-tui"] {
            let matches = build_cli(false)
                .try_get_matches_from(["iocc", "scan", flag])
                .unwrap();
            let cli = Cli::from_arg_matches(&matches).unwrap();
            assert!(cli.broker, "{flag}");
            assert!(!cli.direct, "{flag}");
        }

        let matches = build_cli(false)
            .try_get_matches_from(["iocc", "scan", "--direct"])
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        assert!(cli.direct);
        assert!(!cli.broker);

        assert!(
            build_cli(false)
                .try_get_matches_from(["iocc", "scan", "--direct", "--broker"])
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn broker_probe_requires_a_ping_answer() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let path = std::env::temp_dir().join(format!("iocc-probe-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
            let reply = serde_json::json!({
                "id": request["id"],
                "ok": true,
                "protocol": 1,
                "version": "test",
            });
            writeln!(stream, "{reply}").unwrap();
        });

        assert!(probe_broker(&path));
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(!probe_broker(Path::new("/tmp/iocc-missing-probe.sock")));
    }

    #[test]
    fn config_import_takes_a_file_and_an_optional_force() {
        let matches = build_cli(false)
            .try_get_matches_from(["iocc", "config", "import", "plugin/config.yaml"])
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        match cli.command {
            Some(Commands::Config {
                action: ConfigCommands::Import { file, force },
            }) => {
                assert_eq!(file, PathBuf::from("plugin/config.yaml"));
                assert!(!force);
            }
            _ => panic!("expected config import"),
        }

        let matches = build_cli(false)
            .try_get_matches_from(["iocc", "config", "import", "x.yaml", "--force"])
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                action: ConfigCommands::Import { force: true, .. }
            })
        ));
    }

    #[test]
    fn scan_report_keeps_the_grid() {
        let report = scan_report("device info", &[0x20, 0x4A]);
        assert!(report.starts_with("device info\n"));
        assert_eq!(report.lines().count(), 9);
        assert!(report.contains("0x20") && report.contains("0x4a"));
    }
}
