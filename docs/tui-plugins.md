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
Manifest requests time out after 3 s, views and actions after 15 s; streaming views run until the TUI
stops them. Keep snapshot commands fast, since they run while the device is shared.

A snapshot view that needs the bus must declare `"device": "shared"` as well. An `exclusive` snapshot
view pauses every stream while it runs, so a 1 s refresh would stop and restart monitoring
continuously — the panel would keep showing `paused`/`stopped`.

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
| `views[].label` | panel title; defaults to the view `id` |
| `views[].kind` | `text` (default), `table` or `metrics` |
| `views[].refresh_ms` | optional; without it the view is loaded once and kept until `R` reloads the manifests. Ignored when `stream` is set |
| `views[].stream` | `true`: start the command once and read one update per stdout line |
| `views[].device` | `exclusive` (default) or `shared` (uses `IOCC_SOCKET`, see below) |
| `actions[].args[].type` | `text` (default), `number` or `choice` (needs `options`), validated before running |
| `actions[].confirm` | asks `y` before running (use it for anything destructive) |
| `actions[].device` | `exclusive` (default) or `shared` |

`command` holds the arguments passed to the plugin (without the program name). A view without
`refresh_ms` is a one-shot snapshot: it is loaded once and kept until `R` reloads the manifests. A
`stream` view keeps its process running; `text` views append lines (keeping the last 200) while `table`
and `metrics` views replace their content on every line.

## Sharing the device between plugins

Only one process can open the MCP2221. The TUI keeps it open and exposes a small broker on a Unix
socket, whose path is passed to plugins as `IOCC_SOCKET` (also exported to manifest requests).
A plugin that talks through the socket can run **while** other plugins use the bus, and the TUI
serializes the transactions. This is what makes it possible to watch power consumption while
another plugin drives GPIO, for example.

The socket path is chosen in this order: a non-empty `IOCC_SOCKET`, then `$XDG_RUNTIME_DIR/iocc.sock`,
then a per process file in the temp directory (only reachable through the variable, e.g. when there
is no runtime dir). It is created with mode `0600` and recreated when the TUI starts. The broker only
exists on Unix.

Protocol: one JSON object per line, one response line each.

```jsonc
// request                                       // response
{"id":1,"op":"ping"}                             {"id":1,"ok":true,"protocol":1,"version":"1.2.0"}
{"id":2,"op":"status"}                           {"id":2,"ok":true,"info":"MCP rev A.6 ..."}
{"id":3,"op":"scan"}                             {"id":3,"ok":true,"info":"MCP rev A.6 ...","devices":[32,74]}
{"id":4,"op":"info","address":74}                {"id":4,"ok":true,"info":"MCP rev A.6 ...","address":74}
{"id":5,"op":"read","address":74,"register":2,"count":2}
                                                 {"id":5,"ok":true,"data":[18,171]}
{"id":6,"op":"write","address":74,"register":5,"data":[1,2]}
                                                 {"id":6,"ok":true}
{"id":7,"op":"gpio_read","pin":0}                {"id":7,"ok":true,"pin":0,"value":true}
{"id":8,"op":"gpio_write","pin":0,"value":true}  {"id":8,"ok":true,"pin":0,"value":true}
// errors: {"id":8,"ok":false,"error":"..."}
```

`address` is 7 bit, `register` is the register written before reading, `count` is clamped to 1..256 and
`data` is the byte list written after the register. `info` also needs `address` (any valid one) and
returns the chip information. `scan` returns the same information plus the addresses that answered.
`pin` is 0..3; `gpio_write` configures the pin as an output (keeping the other pins) and `gpio_read`
fails when the pin is not configured as GPIO. `ping` does not touch the device. `id` is optional; when
present it is echoed in the response. Invalid JSON, unknown ops and device errors answer
`{"ok":false,"error":"..."}`.

Rules:

- `shared` views/actions get `IOCC_SOCKET`. If the plugin ignores it and opens the device itself it
  will fail with "resource busy" while the TUI is running.
- `exclusive` (default) runs still work: the TUI pauses streaming panels, releases the chip, runs the
  plugin and restarts the panels afterwards. While such a plugin runs, broker requests fail with
  `device in exclusive use by a plugin`.
- A stream stopped by the TUI (pause for an exclusive action, unpin, selection change, shutdown) is
  not an error and is restarted when it is wanted again. A stream that exits by itself shows its exit
  code/stderr in the panel and stays stopped until `R` reloads the manifests.
- The socket is only created on Unix; elsewhere shared views show an error panel and shared actions
  refuse to run, while exclusive ones are unchanged.
- Command line use is unchanged: `iocc <name> ...` never sets `IOCC_SOCKET`, so the plugin still opens
  the device itself. Only the TUI sets it, and only for `shared` views/actions.
- The native `iocc status`/`scan`/`read`/`write` commands open the device directly by default
  (`iocc read ...`), but accept `--via-tui` to go through a running TUI instead:

  ```sh
  iocc --via-tui status
  iocc --via-tui scan
  iocc --via-tui read -a 4A -r 02 -e 2
  iocc --via-tui write -a 4A -r 02 -d 01
  ```

  It uses `IOCC_SOCKET` or `$XDG_RUNTIME_DIR/iocc.sock`; export `IOCC_SOCKET` when the TUI was started
  with a custom path. The flag is rejected for `iocc tui` and plugins (plugins use `IOCC_SOCKET`).
- To use a shared plugin from another terminal while the TUI is open, export the path yourself:

  ```sh
  IOCC_SOCKET=/run/user/1000/iocc.sock iocc powmon read --json
  ```

  This works because the command line inherits the environment; it only helps plugins that implement
  the protocol.

In Rust, the [`iocc-bus`](../iocc-bus) crate implements this protocol (Unix; the `Bus` stubs return
`Unsupported` elsewhere), so a plugin can just call the typed methods:

```rust
let mut bus = iocc_bus::Bus::connect()?;
let scan = bus.scan()?;
println!("{} {:?}", scan.info, scan.devices);
let data = bus.read(0x4A, 0x02, 2)?;
```

A minimal client (Python, Unix) for the example above:

```python
import json, os, socket

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ["IOCC_SOCKET"])
request = {"id": 1, "op": "read", "address": 0x4A, "register": 0x02, "count": 2}
sock.sendall((json.dumps(request) + "\n").encode())
reply = json.loads(sock.makefile().readline())
print(reply["data"] if reply["ok"] else reply["error"])
```

For streams, send a request per sample and read one line per request; there is no subscription op.

## Pinning panels and recording

Press `[`/`]` on the Plugins tab to select a panel (the title gets a `▸`) and `p` to **pin** it. Pinned
panels are drawn in the *Watches* column on every tab and keep updating while other plugins are used.

- The panel selection is per plugin and wraps around. `p` toggles the selected panel, so a plugin can
  have several pinned panels (for example its live stream and a snapshot table).
- `x` unpins the selected panel, `P` unpins everything and `,`/`.` move the selected watch up/down.
- When the column cannot show every watch, `PageUp`/`PageDown` scrolls the window; the title shows
  `1-3/6` style ranges.
- A pinned snapshot view (no `stream`) keeps refreshing with its `refresh_ms`, just like the plugin
  panel.
- `z` zooms the selected panel to the full body (the plugin list and actions are hidden); `z` again
  returns. `Ctrl+↑`/`Ctrl+↓` scrolls the selected panel (10 lines with `Ctrl+PageUp`/`Ctrl+PageDown`).
- On the Registers tab `p` toggles polling instead; pinning only works on the Plugins tab.
- The streams of the plugin currently selected keep running when you switch tabs (they are just not
  drawn); pinned ones are also drawn everywhere.

### Recording to CSV

Press `g` on the Plugins tab to start recording the **pinned `metrics` panels**; press `g` again to
stop. The header shows a red `REC` while it runs, and starting/stopping writes the file path to the
log. The default file is `iocc-record-<timestamp>.csv` in the current directory; set `IOCC_RECORD` to
use another path.

The long format has one row per metric sample:

```csv
timestamp_ms,plugin,view,metric,unit,value
1770000000123,powmon,live,Current,A,1.523
```

Fields containing a comma, quote or newline are quoted, and each sample is flushed, so an interrupted
TUI keeps the data written so far. Snapshot metrics refresh at their own pace; a stream that stops
simply stops producing rows.

## View output formats

- `text`: shown as is (streaming text keeps the last 200 lines).
- `table`: `{"columns": ["Port", "State"], "rows": [["a", "on"], ["b", 0]]}`
- `metrics`: `{"metrics": [{"name": "Voltage", "value": 12.1, "unit": "V"}]}`, shown with a sparkline of the last 120 samples.

A non zero exit code shows stdout/stderr in red instead.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `USB error: Resource busy` | the plugin opened the MCP2221 while the TUI holds it. Declare the view/action `shared` and use `IOCC_SOCKET`. |
| `cannot connect to the iocc TUI broker at ...` | stale `IOCC_SOCKET`, or the TUI that created it is gone. Unset it to fall back to direct device access. |
| `device in exclusive use by a plugin` | an `exclusive` view/action is using the chip; wait for it to finish. |
| Panel says `command exited ...` | the streaming command ended. Fix it and press `R` to reload the manifests. |
| `this panel needs the device broker (shared device), unavailable here` | the platform is not Unix, or the TUI could not create the socket. |
| Watches show `paused` | an exclusive action is running; they restart automatically when it finishes. |
| A `shared` action is refused | there is no broker socket (not Unix, or a stale `IOCC_SOCKET`); see above. |
| `pin a metrics panel before recording (p)` | `g` records only pinned `metrics` views; pin one first. |
| `Error: no broker socket ...` with `--via-tui` | the TUI is not running, or it uses a custom `IOCC_SOCKET` that is not exported in this shell. |
| Recording file is not where expected | the path is in the log when recording starts; set `IOCC_RECORD` to choose it. |

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
pick a panel with `[`/`]` and pin it with `p`: both plugins stay live in *Watches* on every
tab. The demo also has a *Ports* table you can pin alongside *Live sensors* to have two
watches from the same plugin. Run `Apply load for 4s` (shared) and the watches keep
updating; run `Use the device for 3s` (exclusive) and they show `paused` before
restarting. Press `g` to record the pinned metrics to CSV. See `examples/README.md` for
the full walkthrough, including using the broker from another terminal.

## Keys

| Tab | Keys |
|-----|------|
| all | `Tab`/`Shift+Tab` or `1`-`4` switch tab, `PageUp`/`PageDown` scroll the Watches column, `q` quit |
| Scan | arrows move, `Enter` opens the address in Registers, `s` rescan, `i` device info |
| Registers | `↑↓` select, `Enter` write byte, `r` read, `p` poll, `a`/`g`/`n` address/start/count, `m` read-only |
| Plugins | `↑↓` plugin, `[`/`]` panel, `Enter`/`→` actions, `Esc` back, `p` pin/unpin, `x` unpin, `,`/`.` reorder, `z` zoom, `g` CSV, `Ctrl+↑`/`Ctrl+↓` scroll panel, `P` unpin all, `R` reload manifests |
| Log | `↑↓` scroll, `c` clear |
