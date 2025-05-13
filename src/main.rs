use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio, exit};
use std::thread;
mod comm;
use comm::*;
#[derive(Parser)]
#[command(name = "iocc", version, about = "IO Control Center para comunicação com MCP2221 via plugins", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Exibe status local
    Status,
    /// Gera auto-complete (zsh, bash, etc)
    Scan,
    /// Executa plugin dinâmico
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct Config {
    verbose: bool,
    port: u16,
}

fn start_plugin_server() -> std::thread::JoinHandle<()> {
    thread::spawn(|| {
        let listener = match TcpListener::bind("127.0.0.1:4000") {
            Ok(x) => {
                println!("[IOCC] Server Started");
                x
            }
            Err(x) => {
                println!("[IOCC] Falha ao abrir porta 4000");
                return;
            }
        };

        println!("[IOCC] Servidor de plugins iniciado em 127.0.0.1:4000");

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    //                    thread::spawn(move || {
                    handle_plugin_connection(stream);
                    return;
                    //                    });
                }
                Err(e) => {
                    eprintln!("[IOCC] Erro ao aceitar conexão do plugin: {}", e);
                }
            }
        }
    })
}

fn handle_plugin_connection(mut stream: TcpStream) {
    let mut buf = [0u8; 64];
    loop {
        match stream.read_exact(&mut buf) {
            Ok(_) => {
                if buf[0] == 0xFF {
                    println!("[IOCC] Plugin informou encerramento.");
                    return;
                } else {
                    //todo aqui é onde fica a mágica de rodar extrair os valores e rodar o I2C
                    //todo [0] - status, [1] - address, [2] - read/write, [3] - register, [4] - number of data to read/write, [5..59] - data
                    //todo se for apenas write, manda apenas status 0, se for read, manda status + bytes lidos
                    println!("[IOCC] Recebido do plugin: {:?}", buf);
                    let response = [buf[0] + 1];
                    if let Err(e) = stream.write_all(&response) {
                        eprintln!("[IOCC] Falha ao responder plugin: {}", e);
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
}

fn read_config() -> Config {
    let config_content = fs::read_to_string("config.yaml").unwrap_or_default();
    serde_yaml::from_str(&config_content).unwrap_or(Config {
        verbose: false,
        port: 4000,
    })
}

fn run_plugin(plugin_name: &str, args: &[String], config: &Config) {
    //let plugin_bin = format!("iocc-{}", plugin_name);
    let plugin_bin = {
        let base = format!("iocc-{}", plugin_name);
        if cfg!(windows) {
            format!("{}.exe", base)
        } else {
            base
        }
    };

    println!("{:?}", plugin_bin);
    let paths = vec![
        env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf())),
        Some(env::current_dir().unwrap()),
        env::var("PATH").ok().map(|_| PathBuf::from("")),
    ];

    println!("{:?}", paths);

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
                            let name = file_name.trim_end_matches(".exe").replacen("iocc-", "", 1);
                            found.insert(name);
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
                                let name =
                                    file_name.trim_end_matches(".exe").replacen("iocc-", "", 1);
                                found.insert(name);
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
    println!("Starting Server");
    let server_handle = start_plugin_server();
    let cli = Cli::parse();
    let config = read_config();

    let mut i2cdev = match start_device() {
        Ok(x) => x,
        Err(err) => {
            println!("Error: {err}, check USB connection");
            kill_myself(&config);
            exit(1);
        }
    };

    let raw_args: Vec<String> = env::args().collect();

    // Se o comando for 'help', exibir ajuda e os plugins detectados
    if raw_args.contains(&"help".to_string()) {
        let mut cmd = Cli::command();
        cmd.print_help().unwrap();

        // Aqui será a listagem dos plugins
        let plugins = list_plugins();
        if !plugins.is_empty() {
            println!("\nPlugins detectados:");
            for plugin in plugins {
                println!("  {}", plugin);
            }
        } else {
            println!("\nNenhum plugin detectado.");
        }

        kill_myself(&config);

        return; // Retornar para não prosseguir com a execução normal
    }

    match &cli.command {
        Some(Commands::Status) => {
            println!("Status: tudo funcionando corretamente.");
        }
        Some(Commands::Scan) => {
            scan_bus(&mut i2cdev);
            kill_myself(&config);
        }
        Some(Commands::External(args)) => {
            if !args.is_empty() {
                let cmd = &args[0];
                run_plugin(cmd, &args[1..], &config);
            } else {
                eprintln!("Comando externo não especificado.");
            }
        }
        None => {
            let mut cmd = Cli::command();
            cmd.print_help().unwrap();

            // Listar plugins quando nenhum comando específico for fornecido
            let plugins = list_plugins();
            if !plugins.is_empty() {
                println!("\nPlugins detectados:");
                for plugin in plugins {
                    println!("  {}", plugin);
                }
            } else {
                println!("\nNenhum plugin detectado.");
            }
            kill_myself(&config);
        }
    }
    let _ = server_handle.join();
}

fn kill_myself(config: &Config) {
    match TcpStream::connect(("127.0.0.1", config.port)) {
        Ok(mut stream) => {
            println!("[MAIN] Conectado a SERVER");

            let mut buf = [0u8; 64];
            buf[0] = 0xff;
            stream.write_all(&buf).unwrap();
            println!("[MAIN] Enviado");
        }
        Err(e) => {
            eprintln!("[MAIN] Falha ao conectar: {}", e);
        }
    }
}
