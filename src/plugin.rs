use crate::config::*;
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PLUGIN_PREFIX: &str = "iocc-";

/**
 * Directories searched for plugins, in priority order: the folder of the iocc
 * executable first, then every absolute PATH entry. The current directory is
 * deliberately not searched to avoid running untrusted executables, so empty
 * and relative PATH entries (which mean the current directory) are skipped.
 */
fn plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    {
        dirs.push(dir);
    }
    if let Some(paths) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&paths).filter(|p| p.is_absolute()));
    }
    dirs
}

/**
 * Plugin names are restricted so they can never escape the search directories
 */
fn is_valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/**
 * Whether the file can be executed (execute permission bit on Unix)
 */
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/**
 * Execute plugin and return its exit code
 */
pub fn run_plugin(plugin_name: &str, args: &[String], config: &Iocc) -> i32 {
    if !is_valid_plugin_name(plugin_name) {
        eprintln!("Invalid plugin name '{plugin_name}'.");
        return 1;
    }

    //define required plugin name for detection (iocc-xxxxx)
    let plugin_bin = if cfg!(windows) {
        format!("{PLUGIN_PREFIX}{plugin_name}.exe")
    } else {
        format!("{PLUGIN_PREFIX}{plugin_name}")
    };

    let Some(full_path) = plugin_dirs()
        .into_iter()
        .map(|dir| dir.join(&plugin_bin))
        .find(|path| path.is_file())
    else {
        eprintln!("Plugin '{plugin_bin}' not found.");
        return 1;
    };

    if config.is_verbose() {
        println!("[host] Running plugin: {}", full_path.display());
    }

    let mut command = Command::new(&full_path);
    command
        .args(args)
        .stdout(Stdio::inherit())
        .env("IOCC_HOST", "1");
    if let Some(path) = config_path() {
        command.env("IOCC_CONFIG", path);
    }

    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("Could not run plugin {}: {e}", full_path.display());
            1
        }
    }
}

/**
 * Searches for available plugins and assembles a list to be used for help command
 */
pub fn list_plugins() -> Vec<String> {
    let mut found = BTreeSet::new();

    for dir in plugin_dirs() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_executable(&path) {
                continue;
            }
            if let Some(file_name) = path.file_name().and_then(OsStr::to_str)
                && let Some(name) = file_name.strip_prefix(PLUGIN_PREFIX)
            {
                let name = name.strip_suffix(".exe").unwrap_or(name);
                if is_valid_plugin_name(name) {
                    found.insert(name.to_string());
                }
            }
        }
    }

    found.into_iter().collect()
}

/**
 * Text listing the detected plugins, appended to the help output
 */
pub fn plugins_help() -> String {
    let plugins = list_plugins();
    if plugins.is_empty() {
        "No Plugin detected.".to_string()
    } else {
        let list: Vec<String> = plugins.iter().map(|p| format!("  {p}")).collect();
        format!("Detected Plugins:\n{}", list.join("\n"))
    }
}
