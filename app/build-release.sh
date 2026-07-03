#!/bin/zsh
# Stage GitHub-release artifacts and generate the updater manifest (latest.json).
#
# Run build-app.sh first (it produces the .dmg, the signed .app.tar.gz, and .sig).
# This script copies them to hyphenated names (GitHub mangles spaces in URLs) and
# writes latest.json pointing at the versioned release download URL.
#
#   ./build-release.sh            # stage into dist/release/
#   ./build-release.sh --publish  # ...and create the GitHub release with gh
#
# The updater endpoint in tauri.conf.json is
# https://github.com/ZiineZ/gewis-rdp/releases/latest/download/latest.json
# so latest.json MUST be uploaded as a release asset named exactly "latest.json".

set -e
cd "$(dirname "$0")"

REPO="ZiineZ/gewis-rdp"
NAME="GEWIS Remote Desktop"
VERSION=$(grep '"version"' src-tauri/tauri.conf.json | head -1 | sed 's/[^0-9.]//g')
TAG="v${VERSION}"
MACOS_DIR="src-tauri/target/release/bundle/macos"
OUT="dist/release"

TARBALL="$MACOS_DIR/${NAME}.app.tar.gz"
SIG="$TARBALL.sig"
DMG="dist/${NAME}-${VERSION}.dmg"

for f in "$TARBALL" "$SIG" "$DMG"; do
    [[ -f "$f" ]] || { echo "Missing $f — run ./build-app.sh first."; exit 1; }
done

rm -rf "$OUT"; mkdir -p "$OUT"

# Hyphenated asset names — no spaces to get mangled in download URLs.
DMG_ASSET="GEWIS-Remote-Desktop-${VERSION}.dmg"
TAR_ASSET="GEWIS-Remote-Desktop.app.tar.gz"
cp "$DMG"     "$OUT/$DMG_ASSET"
cp "$TARBALL" "$OUT/$TAR_ASSET"

# latest.json — darwin-aarch64 (Apple Silicon) only; that's all we ship.
SIGNATURE=$(cat "$SIG")
URL="https://github.com/${REPO}/releases/download/${TAG}/${TAR_ASSET}"
PUBDATE=$(date -u +%Y-%m-%dT%H:%M:%SZ)

python3 - "$VERSION" "$PUBDATE" "$SIGNATURE" "$URL" > "$OUT/latest.json" <<'PY'
import json, sys
version, pubdate, signature, url = sys.argv[1:5]
print(json.dumps({
    "version": version,
    "notes": "See the release page for details.",
    "pub_date": pubdate,
    "platforms": {
        "darwin-aarch64": {"signature": signature, "url": url}
    }
}, indent=2))
PY

echo "Staged in $OUT:"
ls -1 "$OUT"

if [[ "$1" == "--publish" ]]; then
    echo "Publishing $TAG to $REPO..."
    gh release create "$TAG" \
        "$OUT/$DMG_ASSET" "$OUT/$TAR_ASSET" "$OUT/latest.json" \
        --repo "$REPO" --title "$TAG" --notes-file dist/release-notes.md
else
    echo ""
    echo "Not published. To publish:"
    echo "  gh release create $TAG \\"
    echo "    \"$OUT/$DMG_ASSET\" \"$OUT/$TAR_ASSET\" \"$OUT/latest.json\" \\"
    echo "    --repo $REPO --title \"$TAG\" --notes-file dist/release-notes.md"
fi
