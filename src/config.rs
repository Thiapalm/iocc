use serde::{Deserialize, Deserializer};
use std::fmt;
use std::path::Path;
use std::{fs, path::PathBuf};

#[derive(Debug)]
pub struct HexU8(pub u8);

impl<'de> Deserialize<'de> for HexU8 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let value = u8::from_str_radix(
            s.strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .unwrap_or(&s),
            16,
        )
        .map_err(serde::de::Error::custom)?;
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
    #[serde(default)]
    iocc: Iocc,
}
#[derive(Debug, Deserialize)]
#[serde(default)]
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

    ensure_config_file(&path);

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Warning: cannot read {}: {e}; using defaults",
                path.display()
            );
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
 * Creates the config file from the built-in template when it does not exist
 */
fn ensure_config_file(path: &Path) {
    if path.exists() {
        return;
    }
    let created = match path.parent() {
        Some(dir) => fs::create_dir_all(dir),
        None => Ok(()),
    }
    .and_then(|_| fs::write(path, DEFAULT_CONFIG));
    if let Err(e) = created {
        eprintln!("Warning: could not create {}: {e}", path.display());
    }
}

/**
 * Sections touched by an [`import_config`] call
 */
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    pub skipped: Vec<String>,
}

/**
 * Merges the top level sections of `source` (for example the `config.yaml`
 * shipped with a plugin) into the shared `config.yaml`. Sections that already
 * exist are skipped unless `force` is set, and the target file is kept as is
 * otherwise: comments, ordering and the user values are preserved.
 */
pub fn import_config(source: &Path, force: bool) -> Result<ImportReport, String> {
    let path = config_path().ok_or_else(|| "system config directory not found".to_string())?;
    ensure_config_file(&path);
    import_into(&path, source, force)
}

/**
 * Same as [`import_config`] with an explicit target, which keeps it testable
 */
fn import_into(target: &Path, source: &Path, force: bool) -> Result<ImportReport, String> {
    let source_text =
        fs::read_to_string(source).map_err(|e| format!("cannot read {}: {e}", source.display()))?;
    let source_root = parse_sections(&source_text, source)?;
    if source_root.is_empty() {
        return Err(format!(
            "{} has no top level section to import",
            source.display()
        ));
    }

    let target_text =
        fs::read_to_string(target).map_err(|e| format!("cannot read {}: {e}", target.display()))?;
    let target_root = parse_sections(&target_text, target)?;
    // Keep the line ending style of the file (Windows editors write CRLF)
    let newline = if target_text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let blocks = top_level_blocks(&source_text);
    let mut report = ImportReport::default();
    let mut text = target_text.clone();

    for (key, value) in &source_root {
        let Some(name) = key.as_str() else {
            return Err(format!(
                "{}: only plain text section names can be imported",
                source.display()
            ));
        };
        // The `iocc:` section belongs to the host, never to a plugin
        if name == "iocc" {
            report.skipped.push(name.to_string());
            continue;
        }
        let exists = target_root.contains_key(key);
        if exists && !force {
            report.skipped.push(name.to_string());
            continue;
        }

        let block = block_named(&blocks, name).unwrap_or_else(|| serialize_section(name, value));
        if exists {
            text = replace_block(&text, name, &block, newline);
            report.replaced.push(name.to_string());
        } else {
            text = append_block(&text, &block, newline);
            report.added.push(name.to_string());
        }
    }

    if text != target_text {
        fs::write(target, text).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    }
    Ok(report)
}

/**
 * Parses a config file as a mapping of top level sections, so an invalid file
 * is reported instead of being rewritten with a broken header
 */
fn parse_sections(text: &str, path: &Path) -> Result<serde_yaml::Mapping, String> {
    match serde_yaml::from_str::<serde_yaml::Value>(text) {
        Ok(serde_yaml::Value::Null) => Ok(serde_yaml::Mapping::new()),
        Ok(serde_yaml::Value::Mapping(map)) => Ok(map),
        Ok(_) => Err(format!(
            "{} is not a YAML mapping of sections",
            path.display()
        )),
        Err(e) => Err(format!("invalid {}: {e}", path.display())),
    }
}

/**
 * Name of the top level key on `line` (`key:` at column zero), if any
 */
fn top_level_key(line: &str) -> Option<String> {
    if line.is_empty() || line.starts_with([' ', '\t', '#', '-']) {
        return None;
    }
    let (key, _) = line.split_once(':')?;
    let key = key.trim();
    let key = key
        .strip_prefix('"')
        .and_then(|k| k.strip_suffix('"'))
        .or_else(|| key.strip_prefix('\'').and_then(|k| k.strip_suffix('\'')))
        .unwrap_or(key);
    (!key.is_empty()).then(|| key.to_string())
}

/**
 * Splits a config file into `(section, lines)` blocks, keeping the comments
 * and the formatting of every section (blank lines between them stay with the
 * preceding block and are trimmed)
 */
fn top_level_blocks(text: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let starts: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| top_level_key(line).map(|key| (index, key)))
        .collect();

    starts
        .iter()
        .enumerate()
        .map(|(position, (start, key))| {
            let end = starts
                .get(position + 1)
                .map(|(next, _)| *next)
                .unwrap_or(lines.len());
            let mut block = lines[*start..end].to_vec();
            while block.last().is_some_and(|line| line.trim().is_empty()) {
                block.pop();
            }
            (key.clone(), block)
        })
        .collect()
}

/**
 * Block of `name` read from the source text, if the section was written as a
 * plain `name:` header (the common case, which keeps the plugin comments)
 */
fn block_named(blocks: &[(String, Vec<String>)], name: &str) -> Option<Vec<String>> {
    blocks
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, block)| block.clone())
}

/**
 * Fallback for sections written in a style the line scanner does not follow
 * (for example flow mappings): re-serialize the parsed value without comments
 */
fn serialize_section(name: &str, value: &serde_yaml::Value) -> Vec<String> {
    let mut map = serde_yaml::Mapping::new();
    map.insert(serde_yaml::Value::String(name.to_string()), value.clone());
    serde_yaml::to_string(&map)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/**
 * Appends a section to the file, separated by a blank line
 */
fn append_block(text: &str, block: &[String], newline: &str) -> String {
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(newline);
    }
    if !out.trim().is_empty() {
        out.push_str(newline);
    }
    out.push_str(&block.join(newline));
    out.push_str(newline);
    out
}

/**
 * Replaces the block of `name` in the target text, or appends it when the
 * header line is not found (the section still exists for serde, e.g. quoted)
 */
fn replace_block(text: &str, name: &str, block: &[String], newline: &str) -> String {
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let mut start = None;
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        let Some(key) = top_level_key(line) else {
            continue;
        };
        if start.is_some() {
            end = index;
            break;
        }
        if key == name {
            start = Some(index);
        }
    }

    let Some(start) = start else {
        return append_block(text, block, newline);
    };
    let mut out: Vec<String> = Vec::new();
    out.extend_from_slice(&lines[..start]);
    out.extend_from_slice(block);
    out.extend_from_slice(&lines[end..]);
    join_lines(out, text, newline)
}

/**
 * Restores the trailing newline after rewriting a block
 */
fn join_lines(lines: Vec<String>, original: &str, newline: &str) -> String {
    let mut content = lines.join(newline);
    if original.ends_with('\n') {
        content.push_str(newline);
    }
    content
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

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str =
        "iocc:\n  address: 0x20\n  verbose: false\n\n# my notes\ntont:\n  address: 0x20\n";

    const SOURCE: &str = "# Example section for the shared iocc config.yaml.\n\
iocc:\n  timeout: 99\n\
\n\
bench:\n  address: 0x20\n  # ignition output\n  ignition:\n    port: portb\n    pin: pin1\n";

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("iocc-config-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn imports_only_missing_sections() {
        let target = temp_file("missing", TARGET);
        let source = temp_file("source", SOURCE);

        let report = import_into(&target, &source, false).unwrap();
        assert_eq!(report.added, ["bench"]);
        assert_eq!(report.skipped, ["iocc"]);

        let text = fs::read_to_string(&target).unwrap();
        // User content and comments survive, the plugin block keeps its comment
        assert!(text.contains("# my notes"));
        assert!(text.contains("tont:"));
        assert!(text.contains("bench:\n  address: 0x20\n  # ignition output\n"));
        // The host section is never touched
        assert!(text.contains("iocc:\n  address: 0x20\n  verbose: false\n"));
        assert!(!text.contains("timeout: 99"));
        // The result is still valid YAML with both sections
        let root: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        assert!(root.get("bench").is_some() && root.get("iocc").is_some());
    }

    #[test]
    fn skips_an_existing_section_unless_forced() {
        let source = temp_file("source-force", SOURCE);

        let target = temp_file("force", TARGET);
        let report = import_into(&target, &source, false).unwrap();
        assert_eq!(report.added, ["bench"]);

        // A second import has nothing to do
        let report = import_into(&target, &source, false).unwrap();
        assert!(report.added.is_empty());
        assert_eq!(report.skipped, ["iocc", "bench"]);

        // --force replaces the block in place, keeping the rest of the file
        fs::write(
            &target,
            format!("{}bench:\n  address: 0x99\n\nlast:\n  x: 1\n", TARGET),
        )
        .unwrap();
        let report = import_into(&target, &source, true).unwrap();
        assert_eq!(report.replaced, ["bench"]);
        let text = fs::read_to_string(&target).unwrap();
        assert!(text.contains("ignition:"));
        assert!(!text.contains("address: 0x99"));
        assert!(text.contains("last:\n  x: 1\n"));
        assert_eq!(text.matches("bench:").count(), 1);
    }

    #[test]
    fn keeps_the_line_endings_of_the_target() {
        let target = temp_file("crlf", "iocc:\r\n  address: 0x20\r\n");
        let source = temp_file("crlf-source", SOURCE);
        let report = import_into(&target, &source, false).unwrap();
        assert_eq!(report.added, ["bench"]);

        let text = fs::read_to_string(&target).unwrap();
        assert_eq!(text.matches("\r\n").count(), text.matches('\n').count());
        assert!(text.contains("\r\nbench:\r\n"));
        assert!(serde_yaml::from_str::<serde_yaml::Value>(&text).is_ok());
    }

    #[test]
    fn refuses_an_invalid_or_empty_source() {
        let target = temp_file("invalid-target", TARGET);
        let source = temp_file("invalid-source", "bench: [unclosed\n");
        let error = import_into(&target, &source, false).unwrap_err();
        assert!(error.contains("invalid"), "{error}");

        let empty = temp_file("empty-source", "# only comments\n");
        let error = import_into(&target, &empty, false).unwrap_err();
        assert!(error.contains("no top level section"), "{error}");
    }
}
