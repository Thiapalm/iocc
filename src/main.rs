use clap::{CommandFactory, Parser, Subcommand};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs;

use std::path::PathBuf;
use std::process::{Command, Stdio, exit};

mod comm;
use comm::*;

#[derive(Parser)]
#[command(name = "iocc", version, about = "IOExtender Control Center, MCP2221 communication control via plugins", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show local Status
    Status,
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

#[derive(Debug)]
pub struct HexU8(pub u8);

impl<'de> Deserialize<'de> for HexU8 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let value =
            u8::from_str_radix(s.trim_start_matches("0x"), 16).map_err(serde::de::Error::custom)?;
        Ok(HexU8(value))
    }
}

impl fmt::Display for HexU8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X}", self.0)
    }
}
#[derive(Debug, Deserialize)]
struct RootConfig {
    iocc: Iocc,
}
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Iocc {
    address: HexU8,
    verbose: bool,
    i2c_speed_hz: u32,
    reset_on_open: bool,
    timeout: u64,
}

fn read_config() -> Iocc {
    let config_content = fs::read_to_string("config.yaml").unwrap_or_default();
    let parsed: Result<RootConfig, _> = serde_yaml::from_str(&config_content);

    match parsed {
        Ok(cfg) => cfg.iocc,
        Err(_) => Iocc {
            address: HexU8(0x20),
            verbose: false,
            i2c_speed_hz: 400_000,
            reset_on_open: false,
            timeout: 10,
        },
    }
}

fn run_plugin(plugin_name: &str, args: &[String], config: &Iocc) {
    //let plugin_bin = format!("iocc-{}", plugin_name);
    let plugin_bin = {
        let base = format!("iocc-{}", plugin_name);
        if cfg!(windows) {
            format!("{}.exe", base)
        } else {
            base
        }
    };

    let paths = vec![
        env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf())),
        Some(env::current_dir().unwrap()),
        env::var("PATH").ok().map(|_| PathBuf::from("")),
    ];

    for path_opt in paths.into_iter().flatten() {
        let full_path = path_opt.join(&plugin_bin);
        if full_path.exists() && full_path.is_file() {
            if config.verbose {
                println!("[host] Executando plugin: {}", full_path.display());
            }
            let mut child = Command::new(full_path)
                .args(args)
                .stdout(Stdio::inherit())
                .spawn()
                .expect("Erro ao executar plugin");

            let _ = child.wait();
            return;
        }
    }

    if let Ok(paths) = env::var("PATH") {
        for path in env::split_paths(&paths) {
            let full_path = path.join(&plugin_bin);
            if full_path.exists() && full_path.is_file() {
                if config.verbose {
                    println!("[host] Executando plugin (PATH): {}", full_path.display());
                }
                let mut child = Command::new(full_path)
                    .args(args)
                    .stdout(Stdio::inherit())
                    .spawn()
                    .expect("Erro ao executar plugin");

                let _ = child.wait();
                return;
            }
        }
    }

    eprintln!("Plugin '{}' não encontrado.", plugin_bin);
}

fn list_plugins() -> Vec<String> {
    let mut found = BTreeSet::new();

    let candidate_dirs = vec![
        env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf())),
        Some(env::current_dir().unwrap()),
    ];

    for path_opt in candidate_dirs.into_iter().flatten() {
        if let Ok(entries) = fs::read_dir(path_opt) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(file_name) = path.file_name().and_then(OsStr::to_str) {
                        if file_name.starts_with("iocc-") {
                            let base_name = file_name
                                .strip_suffix(".exe")
                                .unwrap_or(file_name)
                                .replacen("iocc-", "", 1);
                            found.insert(base_name);
                        }
                    }
                }
            }
        }
    }

    if let Ok(paths) = env::var("PATH") {
        for path in env::split_paths(&paths) {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(file_name) = path.file_name().and_then(OsStr::to_str) {
                            if file_name.starts_with("iocc-") {
                                let base_name = file_name
                                    .strip_suffix(".exe")
                                    .unwrap_or(file_name)
                                    .replacen("iocc-", "", 1);
                                found.insert(base_name);
                            }
                        }
                    }
                }
            }
        }
    }

    found.into_iter().collect()
}

fn main() {
    let raw_args: Vec<String> = std::env::args().collect();

    // Detecta se o primeiro argumento é um plugin conhecido
    if raw_args.len() >= 2 {
        let command = &raw_args[1];
        let plugins = list_plugins();
        if !plugins.contains(command) {
            // Se o comando for 'help', exibir ajuda e os plugins detectados
            if raw_args.iter().any(|arg| arg == "--help" || arg == "-h") {
                let mut cmd = Cli::command();
                cmd.print_help().unwrap();

                // Aqui será a listagem dos plugins
                let plugins = list_plugins();
                if !plugins.is_empty() {
                    println!("\nDetected Plugins:");
                    for plugin in plugins {
                        println!("  {}", plugin);
                    }
                } else {
                    println!("\nNo Plugin detected.");
                }

                return; // Retornar para não prosseguir com a execução normal
            }
        }
    }

    let cli = Cli::parse();
    let config = read_config();
    if config.verbose {
        println!(
            "[host] Speed: {} Reset: {} timeout: {}",
            config.i2c_speed_hz, config.reset_on_open, config.timeout
        );
    }

    let mut i2cdev = match start_device(config.i2c_speed_hz, config.reset_on_open, config.timeout) {
        Ok(x) => x,
        Err(err) => {
            println!("Error: {err}, check USB connection");
            exit(1);
        }
    };

    match &cli.command {
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
            if config.verbose {
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

fn parse_u8_hex(s: &str) -> Result<u8, std::num::ParseIntError> {
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    u8::from_str_radix(s, 16)
}
