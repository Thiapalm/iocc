# iocc-mydev — starter plugin

A standalone Rust crate that already implements the whole contract of an iocc
plugin:

- answers `--iocc-manifest` with TUI panels and actions;
- opens the MCP2221 directly on the command line and uses the TUI broker when
  `IOCC_SOCKET` is set (`"device": "shared"`);
- reads its own section from the shared `config.yaml`, with defaults for a
  missing or partial section;
- saves settings back to that file without touching comments or other sections;
- ships tests for the manifest, the transport selection and the config writer.

The device in `src/device.rs` is imaginary. Replace its register map with your
chip and keep the rest. `src/{bus,config,manifest}.rs` show the reusable parts.

## Create your plugin

From the iocc repository root:

```sh
templates/new-plugin.sh temp-sensor ../iocc-temp-sensor
```

The script copies this directory without `target/` and replaces `mydev` with
`temp-sensor` in the package name, binary name, config section and docs. The
name may contain lowercase letters, digits, `-` and `_`.

Without the script, copy the directory and replace `mydev` yourself:

```sh
cp -R templates/iocc-plugin ../iocc-temp-sensor
cd ../iocc-temp-sensor
grep -rIl --exclude-dir=target --exclude=Cargo.lock mydev . | xargs sed -i 's/mydev/temp-sensor/g'   # Linux
```

## Build and install

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo install --path .
```

`cargo install` puts `iocc-mydev` in `~/.cargo/bin` (in `PATH`); plugins are
found next to the `iocc` binary first, then in every absolute `PATH` entry.
You can also build and copy the binary next to `iocc`:

```sh
cargo build --release
cp target/release/iocc-mydev ~/.local/bin/   # or the folder with the iocc binary
```

During development, skip the install:

```sh
cargo build
PATH="$PWD/target/debug:$PATH" iocc tui
```

Check the manifest without hardware or a running TUI:

```sh
cargo run -- --iocc-manifest | python3 -m json.tool
IOCC_CONFIG=./config.example.yaml cargo run -- --iocc-manifest
```

The second command shows the action defaults coming from the example config.
The flag is handled before the `IOCC_HOST` check, so it works on its own. Every
other command must go through iocc:

```sh
iocc mydev status
iocc mydev read --json
iocc mydev watch --json --interval-ms 500
iocc mydev set gain 4
iocc mydev set threshold 3.3 --no-save
```

`iocc tui` discovers the plugin automatically: tab **3** (Plugins), select
`mydev`, use `[`/`]` to choose a view, `p` to pin it and `Enter`/`→` to run the
actions. The *Watches* column keeps pinned views visible on every tab, even
while another plugin uses the bus.

## Depend on `iocc-bus`

`iocc-bus` is not published on crates.io yet. Inside the iocc repository this
crate uses the path dependency:

```toml
iocc-bus = { version = "0.1.0", path = "../../iocc-bus" }
```

In a separate project, point it at the repository branch that contains the
crate (currently `tui`) and remove the `path`:

```toml
iocc-bus = { git = "https://github.com/Thiapalm/iocc", branch = "tui" }
```

After the 1.2.0 release, replace the branch with the matching tag:

```toml
iocc-bus = { git = "https://github.com/Thiapalm/iocc", tag = "v1.2.0" }
```

The broker exists on Unix only. The template checks `cfg!(unix)` and declares
`exclusive` on Windows, so iocc releases the chip before running each view or
action; adjust that line if you target another platform.

## Configuration

Append [`config.example.yaml`](config.example.yaml) to the shared file
(Linux: `~/.config/iocc/config.yaml`; `iocc` creates the template on first run
and never rewrites an existing file):

```yaml
mydev:
  address: 0x4A
  verbose: false
  i2c_speed_hz: 400000 # in Hz
  reset_on_open: false
  timeout: 10 # in milliseconds
  settings:
    gain: 1
    threshold: 2.0
```

- `IOCC_CONFIG` points at this file; without it the plugin uses the defaults in
  `src/config.rs`.
- `settings.gain` / `settings.threshold` are the persisted action defaults.
  *Set gain* and *Set alert threshold* call `save_setting`, which rewrites only
  that line, keeps its inline comment and appends the key inside
  `mydev.settings` when it does not exist yet.
- `--no-save` changes the device without writing the file; the next run
  restores the saved value (all commands call `device::apply` on start).
- `verbose: true` prints the parsed config, useful when the file is not being
  picked up.
- `src/config.rs` writes only a nested mapping (`mydev.settings.key: value`).
  Flow-style sections (`mydev: { settings: {...} }`) are not supported.

The TUI requests the manifest again after a successful action, so the next
prompt already shows the value that was saved. The manifest is generated from
the same `Settings`, so no extra bookkeeping is needed.

## Adapt it to your chip

| File | What to change |
|---|---|
| `src/device.rs` | Register addresses, chip ID, scaling, signed values, validation |
| `src/config.rs` | `SECTION`, `Settings` fields/defaults, address/speed defaults |
| `src/manifest.rs` | Title, description, views, actions, arguments and defaults |
| `src/main.rs` | CLI commands and their readable output |
| `Cargo.toml` | Package/binary name, description, `iocc-bus` source |
| `tests/manifest.rs` | Binary name used by the integration tests |

Checklist:

1. Rename package, binary, config section and docs (`templates/new-plugin.sh`
   does it).
2. Replace the imaginary register map and the chip ID check.
3. Decide which views need the bus. Read-only sampling should be
   `"device": "shared"` so it survives other plugins; anything that resets or
   invalidates the chip state is better left `exclusive` (the default).
4. Add actions for the settings users change often, with `default` from the
   saved config, and save the new value after applying it.
5. Keep `--iocc-manifest` fast: it must not open the device (the TUI has a 3 s
   timeout).
6. Test `iocc mydev <command>` (direct) and `iocc tui` (broker) with and
   without hardware.

## Manifest reference

This template publishes:

| View | Kind | How it updates |
|---|---|---|
| `live` | `metrics` | streaming every 200 ms, `shared` |
| `voltage` | `chart` of `Voltage` | streaming every 500 ms, `shared`; `m` cycles Voltage/Current/Temperature |
| `status` | `table` | snapshot every 5 s, `shared` |
| `guide` | `text` | loaded once, no device |

| Action | Arguments | Device |
|---|---|---|
| `gain` | choice `1`/`2`/`4`/`8`, default from the config | `shared` |
| `threshold` | number in volts, default from the config | `shared` |
| `reset` | confirmation prompt | `exclusive` |

On Windows the manifest declares `exclusive` for those views/actions instead,
because the broker only exists on Unix (`cfg!(unix)` in `src/manifest.rs`).

Output formats expected by the TUI:

```json
{"metrics": [{"name": "Voltage", "value": 12.1, "unit": "V"}]}
{"columns": ["Field", "Value"], "rows": [["Chip ID", "0x4D59"]]}
```

A `chart` view uses the same JSON as `metrics`; `"metric"` picks the initial
series. A `stream` view must print one JSON object per line and flush. See
[`docs/tui-plugins.md`](../../docs/tui-plugins.md) for the complete protocol,
the broker operations and the pinning/recording keys.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| Plugin does not appear | The file must be named `iocc-<name>`, executable, next to `iocc` or in `PATH` |
| `resource busy` | A `shared` view opened the USB device itself. Keep `IOCC_SOCKET` and use `iocc_bus::Bus` |
| `device in exclusive use by a plugin` | Another plugin is running an `exclusive` action; wait for it to finish |
| `IOCC_SOCKET` is missing | On Windows there is no broker, or the view/action is declared `exclusive` |
| Settings are not saved | `IOCC_CONFIG` is missing, or the file has no `mydev.settings` block |
| Manifest timed out | `--iocc-manifest` opened the device or took longer than 3 s |
| Action prompt shows an old value | Save the new value, and make the manifest read the same config the command writes |
