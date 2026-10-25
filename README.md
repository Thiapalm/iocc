# `IOExtender Control Center (IOCC)`

<p align=center>
  <a href="https://github.com/Thiapalm/iocc"><img src="https://img.shields.io/badge/version-v1.0.1-red"></a>
  <a href="http://www.apache.org/licenses/LICENSE-2.0"><img src="https://img.shields.io/badge/License-ApacheV2-green"></a>
 <a href="http://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-green"></a>
</p>

## Description
This software was developed to work in conjunction with USBIOExtender hardware (https://github.com/Thiapalm/usbioextender). This is a command line tool for communicating with I2C devices connected to the board.
The USBIOExtender board has an I2C connector and a number of IO Pins that can be configured via this software to be used straight from the command line, without the need to install any driver.

## Features
- Run from the command line
- Scan the I2C Bus for connected devices (each address is probed with a 1 byte read, which some devices such as EEPROMs or ADCs may treat as an operation)
- Configure USBIOExtender IO Pins
- Communicate with any I2C device connected to USBIOExtender via Read/Write operations
- Read one or more bytes at the same time (via --expected parameter
)
- Write one or more bytes (use space to separate bytes)
- Show board Status
- Can be extended via plugins
- Can configure default communication parameters via a simple config.yaml file
- Plugins can also use the same config.yaml file

## Version Revision

1.0.0 - First Version

1.0.1
- Can be installed with `cargo install`
- `config.yaml` is now created automatically on first run in the system config folder
- New `setup-udev` command to grant USB access on Linux
- Plugins now receive `IOCC_HOST` and `IOCC_CONFIG`; plugin contract documented ("Writing plugins")
- Plugin detection and execution rewritten; plugin name validation
- General code fixes and refactoring
- Added `install.sh` installer

## Installation
There is no installation required, however, for better usage it is recommended to add the software folder into the system PATH. All plugins must be located into the same folder as the main iocc program. The `config.yaml` file is created automatically on first run in the system config folder (Linux: `~/.config/iocc/config.yaml`, macOS: `~/Library/Application Support/iocc/`, Windows: `%APPDATA%\iocc\`).

### Prerequisites
- This software requires USBIOExtender hardware to work properly (https://github.com/Thiapalm/usbioextender).
- There is no software requirement to use the this cli program, but if you need to compile it, then rust 1.88 or newer is required (https://www.rust-lang.org/).


### Installation Steps
1. Clone the repository:
   ```bash
   git clone https://github.com/thiapalm/iocc.git
   ```
2. Navigate to the project directory.
3. Run (make sure you have rust compiler installed)
   ```rust
   cargo build --release.
   ```
4. Copy the generated executable into a folder of your choice
5. Add the folder into the system PATH

### Linux USB permissions
By default only root can access the USBIOExtender (MCP2221, VID `04d8`, PID `00dd`). Without a udev rule, iocc fails with `USB error: Access denied (insufficient permissions)`. Install the rule once (after `cargo install` or `cargo build`):

```bash
sudo $(which iocc) setup-udev
```

This writes `/etc/udev/rules.d/60-mcp2221.rules` and reloads udev. Then unplug and replug the board. The rule uses `TAG+="uaccess"`, which grants access to the user logged in locally on the machine. It does not apply to SSH sessions or system services; for those, edit the file and use `GROUP="plugdev", MODE="0660"` (and add the user to that group).

## Usage
Instructions on how to use the software.

### Usage Examples

- Fetch device Status:
![Status](images/status.gif)

- Scan for connected devices:
![Scan](images/scan.gif)

- Reading from any connected device:
![Reading](images/iocc_read.gif)

- Writing to any connected device:
![Writing](images/write.gif)

- Get Help:
![Help](images/help.gif)

- Get Help on commands:
![Command Help](images/read_help.gif)

- Get Help on Plugins:
![Plugin Help](images/plugin_help.gif)

## Writing plugins
Plugins are built separately from iocc and cannot run on their own: they are always started by iocc (e.g. `iocc powmon status`). iocc itself works without any plugin. The contract between them is:

- **Name and location:** the executable must be named `iocc-<name>` (`.exe` on Windows, name restricted to letters, digits, `-` and `_`) and live in the same folder as `iocc` or in the `PATH`. `iocc <name> <args>` runs it with the remaining arguments and returns its exit code.
- **Configuration:** the shared file `config.yaml` lives in the system config folder (`<config dir>/iocc/config.yaml`, e.g. `~/.config/iocc/config.yaml` on Linux). Each plugin reads its own top-level section (e.g. `powmon:`) and should fall back to its own defaults if the section is missing. iocc only reads the `iocc:` section and creates the file on first run.
- **Environment:** iocc sets `IOCC_HOST=1` and `IOCC_CONFIG=<path to config.yaml>` for the plugin. Plugins should use `IOCC_CONFIG` instead of computing the path, and refuse to run (asking the user to use `iocc <name>`) when `IOCC_HOST` is not set.
- **Device access:** iocc does not open the USBIOExtender when running a plugin, so the plugin opens the MCP2221 itself.
- **Errors:** write errors to `stderr` and exit with a non-zero code.
- **Terminal UI (optional):** `iocc tui` opens an interactive interface (bus scan, register editor, plugins, log). A plugin adds its own panels and actions by answering `iocc-<name> --iocc-manifest` with a JSON description, with no change or recompilation of iocc. See [docs/tui-plugins.md](docs/tui-plugins.md) and `examples/iocc-demo`.

## Contributing
How to contribute to the project? How to open pull requests?

1. Fork the repository.
2. Create a branch for your changes (`git checkout -b feature/AmazingFeature`).
3. Make the changes and create commits in this branch.
4. Push your changes using `git push origin feature/AmazingFeature`.
5. Open a pull request.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contact
Contact information in case of questions or issues.
palmieri@adastra.aleeas.com