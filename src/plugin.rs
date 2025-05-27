use crate::config::*;
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/**
 * Execute plugin
 */
pub fn run_plugin(plugin_name: &str, args: &[String], config: &Iocc) {
    //define required plugin name for detection (iocc-xxxxx)
    let plugin_bin = {
        let base = format!("iocc-{}", plugin_name);
        if cfg!(windows) {
            format!("{}.exe", base)
        } else {
            base
        }
    };

    //Search plugin on main folder
    let paths = vec![
        env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf())),
        Some(env::current_dir().unwrap()),
        env::var("PATH").ok().map(|_| PathBuf::from("")),
    ];

    //If found, execute it
    for path_opt in paths.into_iter().flatten() {
        let full_path = path_opt.join(&plugin_bin);
        if full_path.exists() && full_path.is_file() {
            if config.is_verbose() {
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

    //search plugin on PATH and execute it if found
    if let Ok(paths) = env::var("PATH") {
        for path in env::split_paths(&paths) {
            let full_path = path.join(&plugin_bin);
            if full_path.exists() && full_path.is_file() {
                if config.is_verbose() {
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

    eprintln!("Plugin '{}' not found.", plugin_bin);
}

/**
 * Searches for available plugins and assembles a list to be used for help command
 */
pub fn list_plugins() -> Vec<String> {
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
