//! Configuration stored in the shared `config.yaml`.
//!
//! `iocc` exports `IOCC_CONFIG` with the path of its config file. This plugin
//! reads and writes only its own top level section (`mydev:`); every other
//! section, comment and indentation is left untouched.

use serde::{Deserialize, Deserializer};
use std::fmt;
use std::fs;
use std::path::PathBuf;

/// Top level section read by this plugin. `iocc` itself ignores it.
pub const SECTION: &str = "mydev";

/// Block inside [`SECTION`] whose keys are written back when a setting changes.
pub const SETTINGS: &str = "settings";

/// 7 bit I2C address, written as `0x4A` in `config.yaml`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Address(pub u8);

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        /// YAML 1.2 reads `0x4A` as the integer 74, while `4A` stays a string.
        /// Accept both so the usual `config.yaml` spelling keeps working.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Text(String),
            Number(u8),
        }

        match Value::deserialize(deserializer)? {
            Value::Text(text) => parse_address(&text)
                .map(Address)
                .map_err(serde::de::Error::custom),
            Value::Number(value) => Ok(Address(value)),
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X}", self.0)
    }
}

/// Accepts `0x4A` and `4A`, the same spelling used by `config.yaml`
fn parse_address(text: &str) -> std::result::Result<u8, String> {
    let text = text.trim();
    let hex = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u8::from_str_radix(hex, 16).map_err(|error| format!("invalid I2C address '{text}': {error}"))
}

/// Values the plugin writes back through the TUI actions
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Amplifier gain: 1, 2, 4 or 8
    pub gain: u8,
    /// Alert threshold in volts
    pub threshold: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            gain: 1,
            threshold: 2.0,
        }
    }
}

/// The `mydev:` section. Missing fields use these defaults, so a partial (or
/// absent) section keeps working.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub address: Address,
    pub verbose: bool,
    pub i2c_speed_hz: u32,
    pub reset_on_open: bool,
    /// Timeout in milliseconds
    pub timeout: u64,
    pub settings: Settings,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            address: Address(0x4A),
            verbose: false,
            i2c_speed_hz: 400_000,
            reset_on_open: false,
            timeout: 10,
            settings: Settings::default(),
        }
    }
}

/// Reads the section, warning on unreadable or invalid files
pub fn read() -> Config {
    read_with(true)
}

/// Same as [`read`], but silent when the file or section is missing. Used by
/// the manifest request, where a warning would only be noise.
pub fn read_quiet() -> Config {
    read_with(false)
}

fn read_with(warn: bool) -> Config {
    let Some(path) = std::env::var_os("IOCC_CONFIG") else {
        if warn {
            eprintln!("Warning: IOCC_CONFIG not set; using the default configuration");
        }
        return Config::default();
    };
    let path = PathBuf::from(path);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            if warn {
                eprintln!(
                    "Warning: cannot read {}: {error}; using the default configuration",
                    path.display()
                );
            }
            return Config::default();
        }
    };
    let root: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(root) => root,
        Err(error) => {
            if warn {
                eprintln!(
                    "Warning: invalid {}: {error}; using the default configuration",
                    path.display()
                );
            }
            return Config::default();
        }
    };
    let Some(section) = root.get(SECTION) else {
        if warn {
            eprintln!(
                "Warning: no {SECTION}: section in {}; using the default configuration",
                path.display()
            );
        }
        return Config::default();
    };
    match serde_yaml::from_value::<Config>(section.clone()) {
        Ok(config) => config,
        Err(error) => {
            if warn {
                eprintln!("Warning: invalid {SECTION} configuration: {error}; using the defaults");
            }
            Config::default()
        }
    }
}

/// Persists one `mydev.settings.<key>` entry in the shared `config.yaml` and
/// returns the file that was written.
///
/// Only that line is rewritten: comments, indentation and every other section
/// are kept byte for byte. A key that does not exist yet is appended inside the
/// `settings` block.
pub fn save_setting(key: &str, value: &str) -> std::result::Result<PathBuf, String> {
    let Some(path) = std::env::var_os("IOCC_CONFIG") else {
        return Err("IOCC_CONFIG not set, cannot save the configuration".into());
    };
    let path = PathBuf::from(path);
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let updated = replace_entry(&text, &[SECTION, SETTINGS], key, value)?;
    fs::write(&path, updated)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(path)
}

/// Rewrites `key: value` inside a nested block such as `mydev.settings`.
fn replace_entry(
    text: &str,
    path: &[&str],
    key: &str,
    value: &str,
) -> std::result::Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let (header, block_indent, end) = find_block(&lines, path)?;

    // Indentation of the direct children of the block (normally block + 2).
    // Inferred from the file so hand formatted 4-space files keep working.
    let child_indent = (header + 1..end)
        .find_map(|index| {
            let trimmed = lines[index].trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let indent = lines[index].len() - trimmed.len();
            (indent > block_indent).then_some(indent)
        })
        .unwrap_or(block_indent + 2);

    let prefix = format!("{key}:");
    let mut insert_at = header + 1;
    let mut found = None;
    for (offset, line) in lines[header + 1..end].iter().enumerate() {
        let index = header + 1 + offset;
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - trimmed.len();
        if indent < child_indent {
            continue;
        }
        if !trimmed.starts_with('#') {
            insert_at = index + 1;
        }
        if indent == child_indent
            && let Some(rest) = trimmed.strip_prefix(&prefix)
        {
            // Keep the inline comment as it was (e.g. `# in volts`).
            let comment = rest
                .find('#')
                .map(|at| format!(" {}", &rest[at..]))
                .unwrap_or_default();
            found = Some((index, comment));
            break;
        }
    }

    if let Some((index, comment)) = found {
        let indent = lines[index].len() - lines[index].trim_start().len();
        lines[index] = format!("{}{key}: {value}{comment}", " ".repeat(indent));
    } else {
        let entry = format!("{}{key}: {value}", " ".repeat(child_indent));
        lines.insert(insert_at, entry);
    }
    Ok(join_lines(lines, text))
}

/// Finds the header of a nested block and the line where it ends.
/// Returns `(header index, header indent, end index)`.
fn find_block(
    lines: &[String],
    path: &[&str],
) -> std::result::Result<(usize, usize, usize), String> {
    let mut low = 0;
    let mut high = lines.len();
    let mut parent_indent = None;
    let mut found = (0, 0, lines.len());

    for segment in path {
        let prefix = format!("{segment}:");
        let header = (low..high)
            .find(|&index| {
                let trimmed = lines[index].trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return false;
                }
                let indent = lines[index].len() - trimmed.len();
                let is_child = match parent_indent {
                    Some(parent) => indent > parent,
                    None => indent == 0,
                };
                is_child && trimmed.starts_with(&prefix)
            })
            .ok_or_else(|| {
                format!(
                    "no {} section in config.yaml, the setting was not saved",
                    path.join(".")
                )
            })?;

        let trimmed = lines[header].trim_start();
        let indent = lines[header].len() - trimmed.len();
        let end = (header + 1..high)
            .find(|&index| {
                let trimmed = lines[index].trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return false;
                }
                lines[index].len() - trimmed.len() <= indent
            })
            .unwrap_or(high);

        found = (header, indent, end);
        low = header + 1;
        high = end;
        parent_indent = Some(indent);
    }
    Ok(found)
}

fn join_lines(lines: Vec<String>, original: &str) -> String {
    let mut content = lines.join("\n");
    if original.ends_with('\n') {
        content.push('\n');
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "iocc:\n  verbose: true\nmydev:\n  address: 0x4A\n  settings:\n    # keep me\n    gain: 1 # x1\n    threshold: 2.0 # in volts\nother:\n  settings:\n    gain: 9\n";

    #[test]
    fn replaces_only_the_requested_entry() {
        let text = replace_entry(SAMPLE, &[SECTION, SETTINGS], "gain", "4").unwrap();
        assert!(text.contains("gain: 4 # x1"), "{text}");
        assert!(text.contains("threshold: 2.0 # in volts"));
        assert!(text.contains("# keep me") && text.contains("other:"));
    }

    #[test]
    fn inserts_a_missing_entry_at_the_end_of_the_block() {
        let text = replace_entry(SAMPLE, &[SECTION, SETTINGS], "averaging", "16").unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[8], "    averaging: 16", "{text}");
        assert_eq!(lines[9], "other:");
    }

    #[test]
    fn refuses_a_file_without_the_section() {
        let error = replace_entry(
            "iocc:\n  verbose: true\nother:\n  settings:\n    gain: 9\n",
            &[SECTION, SETTINGS],
            "gain",
            "4",
        )
        .unwrap_err();
        assert!(error.contains("no mydev.settings section"), "{error}");
    }

    #[test]
    fn rewrites_the_settings_and_reads_them_back() {
        let text = replace_entry(SAMPLE, &[SECTION, SETTINGS], "gain", "8").unwrap();
        let text = replace_entry(&text, &[SECTION, SETTINGS], "threshold", "1.5").unwrap();
        let root: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        let config: Config = serde_yaml::from_value(root.get(SECTION).unwrap().clone()).unwrap();
        assert_eq!(config.settings.gain, 8);
        assert_eq!(config.settings.threshold, 1.5);
        assert_eq!(config.address, Address(0x4A));
    }

    #[test]
    fn saves_the_shared_file() {
        let dir = std::env::temp_dir().join(format!("iocc-template-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(&path, SAMPLE).unwrap();
        // SAFETY: this is the only test that touches IOCC_CONFIG
        unsafe { std::env::set_var("IOCC_CONFIG", &path) };
        let saved = save_setting("gain", "2").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        unsafe { std::env::remove_var("IOCC_CONFIG") };
        assert_eq!(saved, path);
        assert!(text.contains("gain: 2 # x1"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn address_accepts_both_spellings() {
        assert_eq!(parse_address("0x4A"), Ok(0x4A));
        assert_eq!(parse_address("4a"), Ok(0x4A));
        assert!(parse_address("nope").is_err());
        assert_eq!(Address(0x3C).to_string(), "0x3C");

        // YAML turns `0x4A` into an integer, while `4A` stays a string
        let hex: Config = serde_yaml::from_str("address: 0x4A\n").unwrap();
        let text: Config = serde_yaml::from_str("address: 4A\n").unwrap();
        assert_eq!(hex.address, Address(0x4A));
        assert_eq!(text.address, Address(0x4A));
    }
}
