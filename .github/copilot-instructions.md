# Copilot Instructions

`iocc` is a Rust (edition 2024) CLI that talks to I2C devices through the USBIOExtender board (MCP2221 USB bridge, via the `mcp2221` crate and `embedded-hal` 0.2 blocking I2C traits).

## Build / test / lint

- Build: `cargo build --release`
- Lint/format: `cargo clippy` / `cargo fmt`
- There are no tests in the repo; running the binary (`cargo run -- <command>`) requires the physical MCP2221 hardware attached over USB.

## Architecture

- `src/main.rs` – clap (derive) CLI: `status`, `scan`, `read`, `write`, plus `#[command(external_subcommand)]` for plugins. Flow: handle `--help` manually (appends detected plugins) → parse → `read_config()` → `start_device()` (exits with code 1 on USB failure) → dispatch.
- `src/comm.rs` – MCP2221 I2C operations (`start_device`, `scan_bus`, `read_device`, `write_device`).
- `src/config.rs` – loads `config.yaml` from the system config dir via `dirs::config_dir()` (Linux: `~/.config/iocc/config.yaml`). If missing, it is created from the embedded repo `config.yaml` (`include_str!`). Unreadable/invalid files warn on stderr and fall back to defaults (0x20, 400 kHz, timeout 10 ms, non-verbose).
- `src/plugin.rs` – plugin discovery/execution.

## Plugin system

- A plugin is a separate executable named `iocc-<name>` (`.exe` on Windows); `iocc <name> args…` runs it with the remaining args.
- Lookup order: directory of the iocc executable → current dir → each `PATH` entry. `list_plugins()` scans the same places for the help listing.
- `config.yaml` is shared: the host only parses the top-level `iocc:` section (`RootConfig`); other top-level keys (e.g. `ont:`, `powmon:`) belong to plugins, which read the same file themselves. Keep host parsing tolerant of unknown sections.

## Conventions

- Hex values (addresses, registers, data bytes) are parsed with `parse_u8_hex` in `main.rs` (CLI) and `HexU8` in `config.rs` (YAML, written as `0x20`); both accept an optional `0x` prefix and are base-16 only.
- Verbose output uses a `[host]` prefix and is gated by `config.is_verbose()`.
- Doc comments use `/** ... */` block style above functions.
- Dual-licensed MIT OR Apache-2.0.
