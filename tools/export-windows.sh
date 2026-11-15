#!/usr/bin/env bash
# Cross-compiles a Windows release build and exports it, with assets, to the Windows folder.
# Requires: mingw-w64 (sudo apt install mingw-w64) and the x86_64-pc-windows-gnu Rust target.
set -euo pipefail

# Make cargo available when run from a fresh shell.
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"

ROOT="$(git rev-parse --show-toplevel)"
DEST="${ULTRA_SPACE_EXPORT_DIR:-/mnt/c/Users/yangc/Apps/ultra_space}"
TARGET=x86_64-pc-windows-gnu

cd "$ROOT/impl/rust"
# `ULTRA_SPACE_FEATURES=trace` builds with game_bevy's optional features (trace: a Chrome
# trace of every frame, for finding hitches).
cargo build --release -p game_bevy --target "$TARGET" \
    ${ULTRA_SPACE_FEATURES:+--features "$ULTRA_SPACE_FEATURES"}

# Honour a shared build folder (test runs use one to avoid full rebuilds).
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
mkdir -p "$DEST"
cp "$TARGET_DIR/$TARGET/release/ultra_space.exe" "$DEST/"
if [[ -d "$ROOT/assets/export" ]]; then
    mkdir -p "$DEST/assets"
    rsync -a --exclude .gitkeep "$ROOT/assets/export/" "$DEST/assets/"
fi
# Game data the executable loads at runtime (found next to the .exe).
mkdir -p "$DEST/content"
rsync -a --delete --exclude .gitkeep "$ROOT/content/data" "$ROOT/content/lore" "$DEST/content/"
echo "Exported to $DEST"
