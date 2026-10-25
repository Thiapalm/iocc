# Adding a plugin to the iocc TUI

`iocc tui` discovers plugins (`iocc-<name>` executables, same lookup as `iocc <name>`) and asks each one
to describe itself. **No recompilation of iocc is needed**: a plugin only has to answer one extra flag and
print JSON. The plugin can be written in any language.

Plugins that do not answer still show up, with two generic actions: *Show help* and *Run with arguments*.
Nothing is run automatically for them.

## How it works

1. The TUI runs `iocc-<name> --iocc-manifest` (3 s timeout, exit code 0, JSON on stdout).
2. Each **view** is a read only panel: the TUI runs its `command` (every `refresh_ms` if set, otherwise once) and renders stdout. With `stream: true` the command is started once and every stdout line updates the panel.
3. Each **action** is something the user triggers: the TUI asks for the declared `args`, replaces `{name}` in `command` and runs it.
4. By default the TUI **releases** the MCP2221 before running a plugin (the chip accepts one process at a time), so the plugin opens the device itself. Plugins that declare `"device": "shared"` instead receive `IOCC_SOCKET` and leave the chip to the TUI broker (see below), which lets monitoring keep running while another plugin uses the bus.

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
    {"id": "power", "label": "Power", "kind": "metrics", "stream": true, "device": "shared",
     "command": ["watch", "--json"]}
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
| `views[].stream` | `true`: start the command once and read one update per stdout line |
| `views[].device` | `exclusive` (default) or `shared` (uses `IOCC_SOCKET`, see below) |
| `actions[].args[].type` | `text` (default), `number` or `choice` (needs `options`), validated before running |
| `actions[].confirm` | asks `y` before running (use it for anything destructive) |
| `actions[].device` | `exclusive` (default) or `shared` |

`command` holds the arguments passed to the plugin (without the program name).

## Sharing the device between plugins

Only one process can open the MCP2221. The TUI keeps it open and exposes a small broker on a Unix
socket, whose path is passed to plugins as `IOCC_SOCKET` (also exported to manifest requests).
A plugin that talks through the socket can run **while** other plugins use the bus, and the TUI
serializes the transactions. This is what makes it possible to watch power consumption while
another plugin drives GPIO, for example.

Protocol: one JSON object per line, one response line each.

```jsonc
// request                              // response
{"id":1,"op":"info"}                   {"id":1,"ok":true,"info":"MCP rev A.6 ..."}
{"id":2,"op":"read","address":74,"register":2,"count":2}
                                       {"id":2,"ok":true,"data":[18,171]}
{"id":3,"op":"write","address":74,"register":5,"data":[1,2]}
                                       {"id":3,"ok":true}
// errors: {"id":3,"ok":false,"error":"..."}
```

`address` is 7 bit, `register` is the register written before reading, `count` is 1..256.
`info` also needs `address` (any valid one) and returns the chip information.

Rules:

- `shared` views/actions get `IOCC_SOCKET`. If the plugin ignores it and opens the device itself it
  will fail with "resource busy" while the TUI is running.
- `exclusive` (default) runs still work: the TUI pauses streaming panels, releases the chip, runs the
  plugin and restarts the panels afterwards.
- The socket is only created on Unix; elsewhere shared plugins are skipped.
- Command line use is unchanged: `iocc <name> ...` never sets `IOCC_SOCKET`, so the plugin still opens
  the device itself. Only the TUI sets it, and only for `shared` views/actions.

## Pinning panels

Press `p` on the Plugins tab to **pin** the panel of the selected plugin (preferably a `stream` one).
Pinned panels stay visible in the *Watches* column on every tab and keep updating while you use
other plugins. `P` unpins everything.

## View output formats

- `text`: shown as is.
- `table`: `{"columns": ["Port", "State"], "rows": [["a", "on"], ["b", 0]]}`
- `metrics`: `{"metrics": [{"name": "Voltage", "value": 12.1, "unit": "V"}]}`, shown with a sparkline of the last 120 samples.

A non zero exit code shows stdout/stderr in red instead.

## Try it

Two complete shell examples need no hardware:

| Plugin | Shows |
|---|---|
| `examples/iocc-demo` | stream/table/text views, all argument kinds, confirm, shared and exclusive actions |
| `examples/iocc-wave` | simulated signals built for pinning: two watchable panels plus a shared and an exclusive action |

From the repository root, after `cargo build`:

```sh
./examples/tui-demo.sh
```

The launcher puts `examples/` in `PATH` and sets a fixed `IOCC_SOCKET`. Then, on tab 3,
pin the panel of each plugin with `p`: both stay live in *Watches* on every tab. Run
`Apply load for 4s` (shared) and the watches keep updating; run `Use the device for 3s`
(exclusive) and they show `paused` before restarting. See `examples/README.md` for the
full walkthrough, including using the broker from another terminal.

## Keys

| Tab | Keys |
|-----|------|
| all | `Tab`/`Shift+Tab` or `1`-`4` switch tab, `q` quit |
| Scan | arrows move, `Enter` opens the address in Registers, `s` rescan, `i` device info |
| Registers | `↑↓` select, `Enter` write byte, `r` read, `p` poll, `a`/`g`/`n` address/start/count, `m` read-only |
| Plugins | `↑↓` plugin, `Enter`/`→` actions, `Esc` back, `p` pin panel, `P` unpin all, `R` reload manifests |
| Log | `↑↓` scroll, `c` clear |
