# Copilot Instructions

`iocc` is a Rust (edition 2024) CLI that talks to I2C devices through the USBIOExtender board (MCP2221 USB bridge, via the `mcp2221` crate and `embedded-hal` 0.2 blocking I2C traits).

## Build / test / lint

- Build: `cargo build --release`
- Lint/format: `cargo clippy --all-targets -- -D warnings` / `cargo fmt`
- Tests: `cargo test` runs **without hardware** and covers both the `iocc` package and the `iocc-bus` workspace member. The TUI tests put `examples/` on `PATH`, so they exercise the demo plugins (shell scripts) end to end. Running the binary itself (`cargo run -- <command>`) still needs the physical MCP2221 attached over USB.

## Architecture

- `src/main.rs` – clap (derive) CLI: `status`, `scan`, `read`, `write`, `tui`, plus `setup-udev`, plus `#[command(external_subcommand)]` for plugins. Flow: build the clap command (`build_cli`, which appends detected plugins to the root help only when help is requested) → parse → `setup-udev` (needs no config/USB) → `read_config()` → dispatch. The MCP2221 is opened lazily (`open_device`) only by `status`/`scan`/`read`/`write`, never for plugins; errors go to stderr with exit code 1. The global `--via-tui` flag swaps `open_device` for a `iocc_bus::Bus` connection in those four commands (rejected for `tui`/plugins).
- `src/comm.rs` – MCP2221 I2C/GPIO operations (`start_device`, `scan_addresses`, `scan_report`, `read_device`, `write_device`, `gpio_read`, `gpio_write`). `scan_report` formats the info + address grid shared by the direct and broker paths.
- `src/config.rs` – loads `config.yaml` from the system config dir via `dirs::config_dir()` (Linux: `~/.config/iocc/config.yaml`). If missing, it is created from the embedded repo `config.yaml` (`include_str!`). Unreadable/invalid files warn on stderr and fall back to defaults (0x20, 400 kHz, timeout 10 ms, non-verbose).
- `src/plugin.rs` – plugin discovery/execution.
- `src/tui/` – `iocc tui` (ratatui). `backend.rs` owns the MCP2221 in a `SharedDevice`, runs exclusive plugins (device released) and streaming panels (`StreamStart`/`StreamStop`); `broker.rs` (Unix) serves `IOCC_SOCKET` JSON-line requests (`ping`, `status`, `scan`, `info`, `read`, `write`, `gpio_read`, `gpio_write`) so `shared` plugins use the same chip concurrently; `manifest.rs` is the plugin description protocol (`--iocc-manifest`: views with `stream`/`device`/`metric`, actions with `device`); `record.rs` writes the pinned metrics to long-format CSV; `ui.rs` draws, including pinned *Watches* with selection, scroll and zoom; `mod.rs` holds app state/keys. No iocc recompilation is needed for new plugins.
- `iocc-bus/` – reusable workspace crate with the broker client (`Bus::connect`, typed `status`/`scan`/`info`/`read`/`write`/`gpio_*`). Unix implementation, stubs returning `Unsupported` elsewhere. `examples/` – shell demo plugins (`iocc-demo`, `iocc-wave`) that need no hardware; the end-to-end test uses them through `PATH`, and `tui-demo.sh` launches the TUI with them. `docs/tui-plugins.md` is the manifest, broker and keys reference.
- `src/udev.rs` – `iocc setup-udev` writes `/etc/udev/rules.d/60-mcp2221.rules` (Linux, needs root, `TAG+="uaccess"`; must sort before `73-seat-late.rules`) so the local user can access the USB device.

## Plugin system

- A plugin is a separate executable named `iocc-<name>` (`.exe` on Windows); `iocc <name> args…` runs it with the remaining args.
- Lookup order: directory of the iocc executable → each `PATH` entry (the current dir is intentionally not searched). Names must match `[A-Za-z0-9_-]+`; the plugin's exit code is propagated. Plugins are not standalone: the host sets `IOCC_HOST=1` and `IOCC_CONFIG=<config.yaml path>` when spawning them. `list_plugins()` scans the same dirs for the help listing.
- From the TUI the plugin also receives `IOCC_TUI=1`, and `IOCC_SOCKET` when the manifest entry is `shared`. The command line (`plugin_command`) must never set `IOCC_SOCKET`, so plugins keep opening the device as before; only `plugin_command_with` (TUI) passes it.
- `shared` plugins should use the `iocc-bus` crate instead of hand-rolling the JSON protocol. `--via-tui` is only for the native commands: it never changes what plugins receive.
- `config.yaml` is shared: the host only parses the top-level `iocc:` section (`RootConfig`); other top-level keys (e.g. `ont:`, `powmon:`) belong to plugins, which read the same file themselves. Keep host parsing tolerant of unknown sections.

## Conventions

- Hex values are parsed with `parse_u8_hex` in `main.rs` (CLI; I2C addresses go through `parse_i2c_address`, max 0x7F) and `HexU8` in `config.rs` (YAML, written as `0x20`); both accept an optional `0x` prefix and are base-16 only.
- Verbose output uses a `[host]` prefix and is gated by `config.is_verbose()`.
- Doc comments use `/** ... */` block style above functions.
- Keep the command line behaviour stable: plain `iocc status/scan/read/write` opens the device directly, only `shared` manifest entries get the broker, and `exclusive` (the default) still releases the chip and restarts streams afterwards.
- Tests must not need hardware. The TUI tests set `PATH` to `examples/` and drive `App`/`backend` directly; they use `unsafe { std::env::set_var }` (edition 2024). CSV tests write to `std::env::temp_dir()`; keep temp file names valid on Windows (no `:`).
- Dual-licensed MIT OR Apache-2.0.
