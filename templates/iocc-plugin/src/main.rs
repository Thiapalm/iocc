//! Starter iocc plugin.
//!
//! The executable is named `iocc-mydev`, so `iocc mydev <command>` runs it.
//! `iocc tui` also finds it and asks for the manifest without recompiling iocc.
//! Replace `mydev` in this crate with your plugin name (or start from
//! `templates/new-plugin.sh`, which does it for you).

use clap::{CommandFactory, Parser, Subcommand};
use std::io::Write;
use std::process::exit;
use std::time::Duration;

mod bus;
use bus::Bus;
mod config;
use config::Config;
mod device;
mod manifest;

#[derive(Parser)]
#[command(version, about = "Example iocc plugin (replace this with your device)", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check the chip and show the decoded settings
    Status {
        /// Print a JSON table (used by the iocc TUI)
        #[arg(long)]
        json: bool,
    },
    /// Read voltage, current and temperature
    Read {
        /// Print JSON metrics (used by the iocc TUI)
        #[arg(long)]
        json: bool,
    },
    /// Keep printing readings until interrupted (used by the TUI live panel)
    Watch {
        /// Milliseconds between samples
        #[arg(long, default_value_t = 200)]
        interval_ms: u64,
        /// Print one JSON metrics object per line
        #[arg(long)]
        json: bool,
    },
    /// Change a setting and save it in config.yaml
    Set {
        /// Setting name: gain or threshold
        key: String,
        /// New value (gain: 1, 2, 4 or 8; threshold: volts)
        value: String,
        /// Do not write the value to config.yaml (temporary change)
        #[arg(long)]
        no_save: bool,
    },
    /// Reset the device registers
    Reset,
    /// Explain what to try in the TUI (plain text view)
    Guide,
}

fn main() {
    // Answered before anything else: the TUI asks for it without opening the
    // device and without IOCC_HOST (the flag is handled here, not by iocc).
    if std::env::args().any(|arg| arg == manifest::MANIFEST_FLAG) {
        println!("{}", manifest::json(&config::read_quiet().settings));
        return;
    }

    // Plugins are not standalone: iocc sets IOCC_HOST when it starts them.
    if std::env::var_os("IOCC_HOST").is_none() {
        eprintln!("iocc-mydev is a plugin and must be run through iocc: iocc mydev <command>");
        exit(1);
    }

    let cli = Cli::parse();
    let config = config::read();
    if config.verbose {
        println!("[mydev] Configuration: {config:#?}");
    }
    if let Err(error) = run(&cli, &config) {
        eprintln!("Error: {error}");
        exit(1);
    }
}

fn run(cli: &Cli, config: &Config) -> bus::Result<()> {
    let Some(command) = &cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    // The guide is plain text and needs neither the device nor the broker.
    if matches!(command, Commands::Guide) {
        print_guide();
        return Ok(());
    }

    let mut bus = open(config)?;
    match command {
        Commands::Status { json } => status(&mut bus, config, *json),
        Commands::Read { json } => read(&mut bus, config, *json),
        Commands::Watch { interval_ms, json } => watch(&mut bus, config, *interval_ms, *json),
        Commands::Set {
            key,
            value,
            no_save,
        } => set(&mut bus, config, key, value, *no_save),
        Commands::Reset => reset(&mut bus, config),
        Commands::Guide => unreachable!("handled before opening the device"),
    }
}

fn open(config: &Config) -> bus::Result<Bus> {
    Bus::open(config.i2c_speed_hz, config.reset_on_open, config.timeout)
}

fn status(bus: &mut Bus, config: &Config, json: bool) -> bus::Result<()> {
    let address = config.address.0;
    let chip = device::identify(bus, address)?;
    let register = device::config(bus, address)?;
    let gain = device::gain_from_config(register);
    let threshold = device::threshold(bus, address)?;
    let info = bus.status()?;

    if json {
        println!(
            "{}",
            manifest::status_table(
                &config.address.to_string(),
                bus.transport(),
                chip,
                gain,
                threshold,
                &info,
            )
        );
        return Ok(());
    }

    println!("Device    : {}", config.address);
    println!("Transport : {}", bus.transport());
    println!("Chip ID   : 0x{chip:04X}");
    println!("Gain      : x{gain}");
    println!("Threshold : {threshold:.3} V");
    println!("Bus       : {}", info.replace('\n', " "));
    Ok(())
}

fn read(bus: &mut Bus, config: &Config, json: bool) -> bus::Result<()> {
    let address = config.address.0;
    device::identify(bus, address)?;
    device::apply(bus, address, &config.settings)?;
    let reading = device::reading(bus, address)?;
    if json {
        println!("{}", manifest::metrics(&reading));
    } else {
        println!("Voltage     : {:.3} V", reading.voltage);
        println!("Current     : {:.3} A", reading.current);
        println!("Temperature : {:.2} °C", reading.temperature);
    }
    Ok(())
}

fn watch(bus: &mut Bus, config: &Config, interval_ms: u64, json: bool) -> bus::Result<()> {
    let address = config.address.0;
    device::identify(bus, address)?;
    device::apply(bus, address, &config.settings)?;
    let interval = Duration::from_millis(interval_ms.max(10));
    loop {
        let reading = device::reading(bus, address)?;
        if json {
            println!("{}", manifest::metrics(&reading));
        } else {
            println!(
                "{:.3} V  {:.3} A  {:.2} °C",
                reading.voltage, reading.current, reading.temperature
            );
        }
        // Streaming views read one sample per stdout line, so flush every line.
        std::io::stdout().flush()?;
        std::thread::sleep(interval);
    }
}

fn set(bus: &mut Bus, config: &Config, key: &str, value: &str, no_save: bool) -> bus::Result<()> {
    let address = config.address.0;
    device::identify(bus, address)?;
    let saved = match key {
        "gain" => {
            let gain: u8 = value
                .parse()
                .map_err(|_| format!("invalid gain '{value}': use 1, 2, 4 or 8"))?;
            device::set_gain(bus, address, gain)?;
            println!("Gain set to x{gain}");
            gain.to_string()
        }
        "threshold" => {
            let threshold: f64 = value
                .parse()
                .map_err(|_| format!("invalid threshold '{value}': use volts, e.g. 2.5"))?;
            device::set_threshold(bus, address, threshold)?;
            println!("Threshold set to {threshold:.3} V");
            threshold.to_string()
        }
        other => {
            return Err(format!("unknown setting '{other}': use gain or threshold").into());
        }
    };

    if !no_save {
        let path = config::save_setting(key, &saved)?;
        println!("Saved {key}={saved} in {}", path.display());
    }
    Ok(())
}

fn reset(bus: &mut Bus, config: &Config) -> bus::Result<()> {
    let address = config.address.0;
    device::identify(bus, address)?;
    device::reset(bus, address)?;
    println!("Device registers restored to their power-on values");
    Ok(())
}

fn print_guide() {
    println!("Starter plugin guide");
    println!();
    println!("This panel is a plain text view: whatever the command prints is");
    println!("shown here. The other views use the JSON formats in README.md.");
    println!();
    println!("1. Pin *Live readings* with p to keep it in the Watches column.");
    println!("2. Select *Voltage graph*, press p and then m to cycle the series.");
    println!("3. Run *Set gain* and reopen it: the default is the saved value.");
    println!("4. Run *Reset device* (exclusive) and watch the panels pause.");
    println!();
    println!("Edit src/{{main,bus,config,device,manifest}}.rs and replace the");
    println!("imaginary register map with your chip.");
}
