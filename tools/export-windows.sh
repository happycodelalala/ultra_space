#!/usr/bin/env bash
# Cross-compiles a Windows release build and exports it, with assets, to the Windows folder.
# Requires: mingw-w64 (sudo apt install mingw-w64) and the x86_64-pc-windows-gnu Rust target.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
DEST="${ULTRA_SPACE_EXPORT_DIR:-/mnt/c/Users/yangc/Apps/ultra_space}"
TARGET=x86_64-pc-windows-gnu

cd "$ROOT/impl/rust"
cargo build --release -p game_bevy --target "$TARGET"

mkdir -p "$DEST"
cp "target/$TARGET/release/ultra_space.exe" "$DEST/"
if [[ -d "$ROOT/assets/export" ]]; then
    mkdir -p "$DEST/assets"
    rsync -a --exclude .gitkeep "$ROOT/assets/export/" "$DEST/assets/"
fi
echo "Exported to $DEST"
