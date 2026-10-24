use serde::{Deserialize, Deserializer};
use std::fmt;
use std::{fs, path::PathBuf};

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
pub struct Iocc {
    address: HexU8,
    verbose: bool,
    i2c_speed_hz: u32,
    reset_on_open: bool,
    timeout: u64,
}

// Template written on first run; includes the sections used by plugins
const DEFAULT_CONFIG: &str = include_str!("../config.yaml");

impl Default for Iocc {
    fn default() -> Self {
        Iocc {
            address: HexU8(0x20),
            verbose: false,
            i2c_speed_hz: 400_000,
            reset_on_open: false,
            timeout: 10,
        }
    }
}

/**
 * Returns the system config path (Linux: ~/.config/iocc/config.yaml,
 * macOS: ~/Library/Application Support/iocc/config.yaml,
 * Windows: %APPDATA%\iocc\config.yaml)
 */
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("iocc").join("config.yaml"))
}

/**
 * Reads and parses configuration file (config.yaml) into Iocc structure
 * The configuration file is shared among all plugins and is created with
 * default values if it does not exist
 */
pub fn read_config() -> Iocc {
    let Some(path) = config_path() else {
        eprintln!("Warning: system config directory not found; using defaults");
        return Iocc::default();
    };

    if !path.exists() {
        let created = match path.parent() {
            Some(dir) => fs::create_dir_all(dir),
            None => Ok(()),
        }
        .and_then(|_| fs::write(&path, DEFAULT_CONFIG));
        if let Err(e) = created {
            eprintln!("Warning: could not create {}: {e}", path.display());
        }
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: cannot read {}: {e}; using defaults", path.display());
            return Iocc::default();
        }
    };

    match serde_yaml::from_str::<RootConfig>(&content) {
        Ok(cfg) => cfg.iocc,
        Err(e) => {
            eprintln!("Warning: invalid {}: {e}; using defaults", path.display());
            Iocc::default()
        }
    }
}

/**
 * getters for Iocc structure
 */
impl Iocc {
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }
    #[allow(dead_code)]
    pub fn get_address(&self) -> u8 {
        self.address.0
    }

    pub fn get_speed(&self) -> u32 {
        self.i2c_speed_hz
    }

    pub fn is_reset_on_open(&self) -> bool {
        self.reset_on_open
    }

    pub fn get_timeout(&self) -> u64 {
        self.timeout
    }
}
