# Adding a plugin to the iocc TUI

`iocc tui` discovers plugins (`iocc-<name>` executables, same lookup as `iocc <name>`) and asks each one
to describe itself. **No recompilation of iocc is needed**: a plugin only has to answer one extra flag and
print JSON. The plugin can be written in any language.

Plugins that do not answer still show up, with two generic actions: *Show help* and *Run with arguments*.
Nothing is run automatically for them.

## How it works

1. The TUI runs `iocc-<name> --iocc-manifest` (3 s timeout, exit code 0, JSON on stdout).
2. Each **view** is a read only panel: the TUI runs its `command` (every `refresh_ms` if set, otherwise once) and renders stdout.
3. Each **action** is something the user triggers: the TUI asks for the declared `args`, replaces `{name}` in `command` and runs it.
4. The TUI always releases the MCP2221 before running a plugin (the chip accepts one process at a time), so plugins keep opening the device themselves, exactly as with `iocc <name>`.

Plugins are started with `IOCC_HOST=1`, `IOCC_CONFIG=<config.yaml>` and, from the TUI, `IOCC_TUI=1`.
Commands time out after 15 s. Keep view commands fast, since they run while the device is shared.

## Manifest

```json
{
  "protocol": 1,
  "title": "Power monitor",
  "version": "0.2.0",
  "description": "INA219 readings",
  "views": [
    {"id": "power", "label": "Power", "kind": "metrics", "refresh_ms": 1000, "command": ["read", "--json"]}
  ],
  "actions": [
    {"id": "mode", "label": "Set mode", "description": "change the chip mode",
     "args": [{"name": "mode", "label": "Mode", "type": "choice",
               "options": ["PowerDown", "ShuntAndBusContinuous"], "default": "ShuntAndBusContinuous"}],
     "command": ["set-mode", "{mode}"], "confirm": false}
  ]
}
```

| Field | Notes |
|-------|-------|
| `protocol` | must be `1` |
| `views[].kind` | `text` (default), `table` or `metrics` |
| `views[].refresh_ms` | optional; without it the view is loaded once when the plugin is selected |
| `actions[].args[].type` | `text` (default), `number` or `choice` (needs `options`), validated before running |
| `actions[].confirm` | asks `y` before running (use it for anything destructive) |

`command` holds the arguments passed to the plugin (without the program name).

## View output formats

- `text`: shown as is.
- `table`: `{"columns": ["Port", "State"], "rows": [["a", "on"], ["b", 0]]}`
- `metrics`: `{"metrics": [{"name": "Voltage", "value": 12.1, "unit": "V"}]}`, shown with a sparkline of the last 120 samples.

A non zero exit code shows stdout/stderr in red instead.

## Try it

`examples/iocc-demo` is a complete shell example (no hardware needed):

```sh
cp examples/iocc-demo ~/.local/bin/
iocc tui     # tab 3, Plugins
```

## Keys

| Tab | Keys |
|-----|------|
| all | `Tab`/`Shift+Tab` or `1`-`4` switch tab, `q` quit |
| Scan | arrows move, `Enter` opens the address in Registers, `s` rescan, `i` device info |
| Registers | `↑↓` select, `Enter` write byte, `r` read, `p` poll, `a`/`g`/`n` address/start/count, `m` read-only |
| Plugins | `↑↓` plugin, `Enter`/`→` actions, `Esc` back, `R` reload manifests |
| Log | `↑↓` scroll, `c` clear |
