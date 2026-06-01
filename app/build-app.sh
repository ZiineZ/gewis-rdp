#!/bin/zsh
set -e
cd "$(dirname "$0")"

echo "=== GEWIS Remote Desktop — build macOS app ==="
echo ""

# ── Rust ──────────────────────────────────────────────────────────────────────
if ! command -v rustc &>/dev/null; then
    echo "[1/4] Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet
    source "$HOME/.cargo/env"
else
    echo "[1/4] Rust $(rustc --version | awk '{print $2}') — OK"
fi

# ── Tauri CLI ─────────────────────────────────────────────────────────────────
if ! command -v cargo-tauri &>/dev/null; then
    echo "[2/4] Installing Tauri CLI..."
    cargo install tauri-cli --version "^2.0" --quiet
else
    echo "[2/4] Tauri CLI $(cargo tauri --version 2>&1 | head -1) — OK"
fi

# ── App icon ──────────────────────────────────────────────────────────────────
echo "[3/4] Generating icons..."
ICON_SRC="src-tauri/icons/icon_source.png"

if ! [[ -f "$ICON_SRC" ]]; then
    # Generate a simple red square with "G" using ImageMagick (brew install imagemagick)
    if command -v convert &>/dev/null; then
        convert -size 1024x1024 xc:'#C50000' \
            -fill white -font Helvetica-Bold -pointsize 680 \
            -gravity Center -annotate 0 "G" \
            "$ICON_SRC" 2>/dev/null
    else
        # Fallback: copy the GEWIS SVG rendered to PNG via rsvg-convert or sips
        echo "  ImageMagick not found; using minimal placeholder icon."
        # Create a 1x1 red PNG using Python (no deps needed)
        python3 -c "
import struct, zlib, base64, io

def png(w, h, rgb):
    def chunk(tag, data):
        c = zlib.crc32(tag + data) & 0xffffffff
        return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', c)
    raw = b''
    for _ in range(h):
        raw += b'\\x00' + bytes(rgb) * w
    compressed = zlib.compress(raw)
    return (b'\\x89PNG\\r\\n\\x1a\\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', compressed)
            + chunk(b'IEND', b''))

import sys
sys.stdout.buffer.write(png(1024, 1024, [197, 0, 0]))
" > "$ICON_SRC"
    fi
fi

# Let Tauri generate all required icon sizes from the source
cargo tauri icon "$ICON_SRC" --output src-tauri/icons 2>/dev/null || true

# ── Build ─────────────────────────────────────────────────────────────────────
echo "[4/4] Building app (first run takes ~2-5 min to compile dependencies)..."
cargo tauri build

APP="src-tauri/target/release/bundle/macos/GEWIS Remote Desktop.app"
if [[ -d "$APP" ]]; then
    # Ad-hoc sign the bundled FreeRDP binaries and the app.
    # install_name_tool invalidates code signatures; macOS requires valid signatures to launch.
    echo "  Signing bundled binaries..."
    for f in "$APP/Contents/Resources/resources/"*; do
        codesign --force --sign - "$f" 2>/dev/null
    done
    codesign --force --deep --sign - "$APP" 2>/dev/null

    echo ""
    echo "✓ Built: $APP"
    echo ""
    echo "✓ Built: $APP"
    echo ""

    # Build the DMG installer
    ./build-dmg.sh

    echo ""
    echo "Install to Applications (drag from the .dmg, or copy directly):"
    echo "  cp -r \"$APP\" /Applications/"
    echo "  xattr -cr \"/Applications/GEWIS Remote Desktop.app\""
fi
