# iocc-bus

Rust client for the device broker exposed by `iocc tui`. A `shared` plugin can use the
MCP2221 while the TUI keeps it open, without reimplementing the JSON protocol.

The TUI listens on a Unix socket and passes its path as `IOCC_SOCKET`. The client also
reads `$XDG_RUNTIME_DIR/iocc.sock` when the variable is not set. The broker only exists on
Unix; on other platforms the `Bus` methods return `Error::Unsupported`.

```rust
let mut bus = iocc_bus::Bus::connect()?;
let scan = bus.scan()?;
println!("{} {:?}", scan.info, scan.devices);

let data = bus.read(0x4A, 0x02, 2)?;
bus.write(0x4A, 0x02, &[0x01])?;
let pin = bus.gpio_read(0)?;
bus.gpio_write(0, !pin)?;
# Ok::<(), iocc_bus::Error>(())
```

Available operations: `ping`, `status`, `scan`, `info`, `read`, `write`, `gpio_read`,
`gpio_write`. Requests are serialized (one line out, one line back), matching the broker.

## Dependency

This crate lives in the `iocc` repository and is not published on crates.io yet. Depend on
it by path or git:

```toml
[dependencies]
iocc-bus = { path = "../iocc/iocc-bus" }
# or
# iocc-bus = { git = "https://github.com/Thiapalm/iocc", package = "iocc-bus" }
```

See [`docs/tui-plugins.md`](../docs/tui-plugins.md#sharing-the-device-between-plugins) for
the manifest fields and the raw protocol.
