# Demo plugins

Both plugins are plain shell scripts: they describe themselves to the TUI and never
touch the USB device, so they run without hardware.

| Plugin | What it shows |
|---|---|
| `iocc-demo` | views (stream, table, text), all argument kinds, confirm, shared and exclusive actions |
| `iocc-wave` | simulated signals built for pinning: two chart panels you can pin together, a shared action that keeps watches live and an exclusive one that pauses them |

## Run

From the repository root, after `cargo build`:

```sh
./examples/tui-demo.sh
```

The script puts `examples/` in `PATH` (that is how the TUI finds `iocc-*` executables),
sets a fixed `IOCC_SOCKET` and starts the TUI. Set `IOCC=/path/to/iocc` to use another
binary. Plugins already installed in `PATH` (for example `iocc-powmon`) also appear in
the list; use `↑↓` to reach the demos by name.

## What to try

1. Tab **3** (Plugins): select `demo`, choose *Live sensors* with `[`/`]` and press `p`.
2. Still on `demo`, select *Ports* and press `p` again: two panels from the same plugin.
3. Select `wave`, press `p` to pin *Live signals*.
4. Still on `wave`, use `]` to reach *Voltage graph* and *Current graph* and press `p`
   on each: the two charts stay visible at the same time (`m` switches the series of
   the selected chart).
5. Switch to tabs **1** or **2**: the *Watches* column keeps all four panels updating.
6. Back on tab 3, with `wave` selected, press `→` to focus the actions:
   - **Apply load for 4s** is `shared`: the watches keep updating while it runs.
   - **Use the device for 3s** is exclusive: the watch titles change to `paused` and
     the streams restart after it finishes.
7. Press `g` to record the pinned metrics to `iocc-record-<timestamp>.csv` in the
   current directory (`g` again stops; `IOCC_RECORD=path` chooses the file).
8. `P` unpins everything, `q` quits.

Pinning is per panel, and a plugin may contribute several. When the column cannot show
all of them, `PageUp`/`PageDown` scrolls the Watches window; `,`/`.` reorder and `x`
unpins one. Use `z` to zoom a panel while reading it.

## Test the broker from the command line

With the TUI running (the launcher prints the socket path), in another terminal:

```sh
IOCC_SOCKET=/run/user/1000/iocc-demo-1000.sock iocc wave watch
```

`iocc wave watch` prints one JSON metrics line per sample and, because it implements the
broker protocol, reads through the TUI instead of touching the device. Without
`IOCC_SOCKET` it still runs (the demo is fake), but a real plugin would try to open the
device and find it busy while the TUI holds it.

The native commands can also use the broker explicitly:

```sh
iocc --via-tui scan
iocc --via-tui read -a 4A -r 02 -e 2
```

Those need real hardware; the demo plugins only simulate the device.

The real hardware example is `iocc-powmon`: its README explains the same flow for the
INA226.
