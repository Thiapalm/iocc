#!/usr/bin/env bash
# Installer for IOCC (IOExtender Control Center).
#
# Usage: ./install.sh [--prefix DIR] [--no-udev] [--uninstall] [--yes] [--help]
#   --prefix DIR   Install directory for the binary (default: ~/.local/bin)
#   --no-udev      Skip the Linux udev rule setup
#   --uninstall    Remove iocc and iocc-* plugins from the install directory
#                  (lists the files and asks for confirmation)
#   --yes          Do not ask for confirmation when uninstalling
set -euo pipefail

BIN_DIR="${HOME}/.local/bin"
SETUP_UDEV=1
UNINSTALL=0
ASSUME_YES=0

usage() { sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            [ $# -ge 2 ] || { echo "error: --prefix requires a directory" >&2; exit 1; }
            BIN_DIR="$2"; shift 2 ;;
        --no-udev) SETUP_UDEV=0; shift ;;
        --uninstall) UNINSTALL=1; shift ;;
        --yes|-y) ASSUME_YES=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
done

if [ "$UNINSTALL" -eq 1 ]; then
    targets=()
    for f in "${BIN_DIR}/iocc" "${BIN_DIR}"/iocc-*; do
        [ -e "$f" ] && targets+=("$f")
    done
    if [ "${#targets[@]}" -eq 0 ]; then
        echo "Nothing to remove in ${BIN_DIR}."
        exit 0
    fi
    echo "The following files will be removed (may include third-party iocc-* plugins):"
    printf '  %s\n' "${targets[@]}"
    if [ "$ASSUME_YES" -ne 1 ]; then
        read -r -p "Continue? [y/N] " answer || answer=""
        case "$answer" in y|Y|yes|YES) ;; *) echo "Aborted."; exit 1 ;; esac
    fi
    rm -fv "${targets[@]}"
    echo "Config files in the system config folder were left untouched."
    exit 0
fi

cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. Install Rust from https://www.rust-lang.org/tools/install" >&2
    exit 1
fi

echo "==> Building iocc (release)"
cargo build --release

echo "==> Installing to ${BIN_DIR}"
mkdir -p "$BIN_DIR"
install -m 755 target/release/iocc "${BIN_DIR}/iocc"

# Plugins built next to iocc (iocc-<name>) are installed in the same folder.
for plugin in target/release/iocc-*; do
    [ -f "$plugin" ] && [ -x "$plugin" ] || continue
    case "$plugin" in *.d) continue ;; esac
    install -m 755 "$plugin" "${BIN_DIR}/$(basename "$plugin")"
    echo "    plugin: $(basename "$plugin")"
done

if [ "$(uname -s)" = "Linux" ] && [ "$SETUP_UDEV" -eq 1 ]; then
    echo "==> Installing udev rule (requires sudo)"
    if sudo "${BIN_DIR}/iocc" setup-udev; then
        echo "    Unplug and replug the USBIOExtender board."
    else
        echo "warning: udev setup failed. Run later: sudo ${BIN_DIR}/iocc setup-udev" >&2
    fi
fi

case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) echo "note: ${BIN_DIR} is not in your PATH. Add this to your shell profile:"
       echo "      export PATH=\"${BIN_DIR}:\$PATH\"" ;;
esac

echo "Done. Try: iocc --help"
