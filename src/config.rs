use serde::{Deserialize, Deserializer};
use std::fmt;
use std::fs;

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

/**
 * Reads and parses configuration file (config.yaml) into Iocc structure
 * The configuratin file is shared among all plugins
 */
pub fn read_config() -> Iocc {
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
