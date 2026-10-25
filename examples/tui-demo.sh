#!/bin/sh
# Starts `iocc tui` with the demo plugins in PATH and a fixed broker socket,
# so a second terminal can share the device (no hardware needed).
set -e

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")

iocc=${IOCC:-"$root/target/debug/iocc"}
[ -x "$iocc" ] || iocc="$root/target/release/iocc"
if [ ! -x "$iocc" ]; then
    echo "error: build iocc first (cargo build) or set IOCC=/path/to/iocc" >&2
    exit 1
fi

export IOCC_SOCKET=${IOCC_SOCKET:-"${XDG_RUNTIME_DIR:-/tmp}/iocc-demo-$(id -u).sock"}
export PATH="$here:$PATH"

echo "iocc:   $iocc"
echo "socket: $IOCC_SOCKET"
echo "Share it from another terminal, e.g.: IOCC_SOCKET=$IOCC_SOCKET iocc wave watch"
echo

exec "$iocc" tui
