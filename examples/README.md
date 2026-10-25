# Demo plugins

Both plugins are plain shell scripts: they describe themselves to the TUI and never
touch the USB device, so they run without hardware.

| Plugin | What it shows |
|---|---|
| `iocc-demo` | views (stream, table, text), all argument kinds, confirm, shared and exclusive actions |
| `iocc-wave` | simulated signals built for pinning: two watchable panels, a shared action that keeps watches live and an exclusive one that pauses them |

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

1. Tab **3** (Plugins): select `demo`, press `p` to pin *Live sensors*.
2. Select `wave`, press `p` to pin *Live signals*.
3. Switch to tabs **1** or **2**: the *Watches* column keeps both panels updating.
4. Back on tab 3, with `wave` selected, press `→` to focus the actions:
   - **Apply load for 4s** is `shared`: the watches keep updating while it runs.
   - **Use the device for 3s** is exclusive: the watch titles change to `paused` and
     the streams restart after it finishes.
5. `P` unpins everything, `q` quits.

Pinning keeps one panel per plugin, and it picks the first `stream` view: that is why
the demo has two plugins — one panel each. The column height is split evenly between
the pinned panels.

## Test the broker from the command line

With the TUI running (the launcher prints the socket path), in another terminal:

```sh
IOCC_SOCKET=/run/user/1000/iocc-demo-1000.sock iocc wave watch
```

`iocc wave watch` prints one JSON metrics line per sample and, because it implements the
broker protocol, reads through the TUI instead of touching the device. Without
`IOCC_SOCKET` it still runs (the demo is fake), but a real plugin would try to open the
device and find it busy while the TUI holds it.

The real hardware example is `iocc-powmon`: its README explains the same flow for the
INA226.
