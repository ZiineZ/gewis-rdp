#!/bin/zsh
# Build a custom DMG installer for GEWIS Remote Desktop.
#
# Why this script and not Tauri's built-in DMG bundler:
#   - Tauri's DMG config schema is parsed correctly but the values are not
#     forwarded to bundle_dmg.sh in the current CLI version, so the background
#     and window layout are silently dropped.
#   - The default bundle_dmg.sh (from create-dmg) drives Finder via AppleScript,
#     which fails silently on macOS 12+ without explicit accessibility grants.
#
# `appdmg` writes the .DS_Store directly, no AppleScript, so it Just Works.

set -e
cd "$(dirname "$0")"

APP_NAME="GEWIS Remote Desktop"
VERSION="1.3.0"
APP_PATH="src-tauri/target/release/bundle/macos/${APP_NAME}.app"
DMG_PATH="dist/${APP_NAME}-${VERSION}.dmg"
ICONS_DIR="src-tauri/icons"

if ! [[ -d "$APP_PATH" ]]; then
    echo "Build the .app first: cargo tauri build"
    exit 1
fi

# Install appdmg locally if missing (~5 MB, no global pollution)
if ! [[ -d "node_modules/appdmg" ]]; then
    echo "Installing appdmg (one-time)..."
    npm install --no-save --silent appdmg
fi

# Generate the multi-resolution TIFF background from the 2x PNG.
# appdmg needs this to render the background correctly on Retina displays.
echo "Generating Retina background..."
sips -z 400 660 "${ICONS_DIR}/dmg-background.png" \
     --out "${ICONS_DIR}/dmg-background-1x.png" > /dev/null
tiffutil -cathidpicheck \
     "${ICONS_DIR}/dmg-background-1x.png" \
     "${ICONS_DIR}/dmg-background.png" \
     -out "${ICONS_DIR}/dmg-background.tiff" 2>/dev/null
rm "${ICONS_DIR}/dmg-background-1x.png"

mkdir -p dist
rm -f "$DMG_PATH"

echo "Packaging DMG..."
node_modules/.bin/appdmg dmg-config.json "$DMG_PATH" 2>&1 | tail -3

# Ad-hoc sign the DMG so Gatekeeper doesn't immediately reject it
codesign --force --sign - "$DMG_PATH" 2>/dev/null

# Clean up the generated TIFF (regenerated each build)
rm -f "${ICONS_DIR}/dmg-background.tiff"

echo ""
echo "Built: $DMG_PATH ($(du -h "$DMG_PATH" | cut -f1))"
echo ""
echo "Distribute: send the .dmg to anyone, they double-click and drag the"
echo "app into the Applications shortcut. After first launch they may need:"
echo "  xattr -cr \"/Applications/${APP_NAME}.app\""
