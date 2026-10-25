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
sets a fixed `IOCC_SOCKET` and starts the TUI.

## What to try

1. Tab **3** (Plugins): select `demo`, press `p` to pin *Live sensors*.
2. Select `wave`, press `p` to pin *Live signals*.
3. Switch to tabs **1** or **2**: the *Watches* column keeps both panels updating.
4. Back on tab 3, with `wave` selected, press `→` to focus the actions:
   - **Apply load for 4s** is `shared`: the watches keep updating while it runs.
   - **Use the device for 3s** is exclusive: the watch titles change to `paused` and
     the streams restart after it finishes.
5. `P` unpins everything.

## Test the broker from the command line

With the TUI running (the launcher prints the socket path), in another terminal:

```sh
IOCC_SOCKET=/tmp/iocc-demo.sock iocc wave watch
```

`iocc wave watch` prints one JSON metrics line per sample and, because it implements the
broker protocol, reads through the TUI instead of opening the device. Without
`IOCC_SOCKET` it falls back to reading the device directly.

The real hardware example is `iocc-powmon`: its README explains the same flow for the
INA226.
