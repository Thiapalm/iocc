#!/usr/bin/env sh
# Copy templates/iocc-plugin into a new crate and rename the example plugin.
set -eu

usage() {
    cat <<'EOF'
Usage: templates/new-plugin.sh NAME DEST

  NAME   plugin name without the iocc- prefix (lowercase letters, digits, - and _)
  DEST   new directory (created if it does not exist; must be empty otherwise)

Example:
  templates/new-plugin.sh temp-sensor ../iocc-temp-sensor
EOF
}

if [ "$#" -ne 2 ]; then
    usage >&2
    exit 1
fi

name=$1
dest=$2
case "$name" in
    '')
        echo "invalid name: it cannot be empty" >&2
        exit 1
        ;;
    [!a-z]* | *[!a-z0-9_-]*)
        echo "invalid name '$name': use lowercase letters, digits, - and _" >&2
        exit 1
        ;;
esac

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
template=$script_dir/iocc-plugin
if [ ! -d "$template" ]; then
    echo "template not found at $template" >&2
    exit 1
fi
if [ -e "$dest" ] && [ -n "$(ls -A "$dest" 2>/dev/null)" ]; then
    echo "destination '$dest' already exists and is not empty" >&2
    exit 1
fi
mkdir -p "$dest"

# `cp -R dir/. dest/` copies the contents, including dotfiles.
cp -R "$template"/. "$dest"/
rm -rf "$dest/target" "$dest/Cargo.lock"

find "$dest" -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.md' -o -name '*.yaml' \) |
    while IFS= read -r file; do
        sed "s/mydev/$name/g" "$file" >"$file.tmp"
        mv "$file.tmp" "$file"
    done

echo "Created iocc-$name in $dest"
echo
echo "Next:"
echo "  cd $dest"
echo "  cargo test"
echo "  cargo install --path ."
echo "  iocc tui        # tab 3, then select $name"
echo
echo "Then replace the imaginary register map in src/device.rs and edit the"
echo "title/description in src/manifest.rs."
