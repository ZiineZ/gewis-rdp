#!/bin/zsh
# GEWIS Remote Desktop — one-time setup for macOS
set -e

echo "=== GEWIS Remote Desktop Setup ==="
echo ""

# Detect Homebrew
BREW=$(brew --prefix 2>/dev/null) || {
    echo "Homebrew not found. Install it first: https://brew.sh"
    exit 1
}

# ── Step 1: Homebrew packages ────────────────────────────────────────────────
echo "[1/4] Installing packages (freerdp, krb5, xquartz)..."
brew install freerdp krb5
brew install --cask xquartz
echo ""

# ── Step 2: Kerberos config ──────────────────────────────────────────────────
echo "[2/4] Writing /etc/krb5.conf (needs sudo)..."
sudo tee /etc/krb5.conf > /dev/null <<'EOF'
[libdefaults]
  default_realm = GEWISWG.GEWIS.NL
  rdns = false

[realms]
  GEWISWG.GEWIS.NL = {
    kdc = https://gewisvdesktop.gewis.nl/KdcProxy
  }
EOF
echo "Done."
echo ""

# ── Step 3: Build FreeRDP with KRB5 ─────────────────────────────────────────
# The Homebrew bottle has Kerberos disabled on macOS; we rebuild it from source.

INSTALL_DIR="$HOME/opt/freerdp-krb5"

# Skip build if already done
if [[ -x "$INSTALL_DIR/bin/xfreerdp" ]]; then
    KRB5_STATUS=$("$INSTALL_DIR/bin/xfreerdp" +buildconfig 2>&1 | grep -o "WITH_KRB5=ON" || true)
    if [[ "$KRB5_STATUS" == "WITH_KRB5=ON" ]]; then
        echo "[3/4] FreeRDP with KRB5 already built — skipping."
        echo ""
        echo "[4/4] Setup complete!"
        echo "Run: ~/gewis-rdp/connect.sh"
        exit 0
    fi
fi

echo "[3/4] Building FreeRDP 3.26.0 with Kerberos support (~5-10 min)..."
BUILD_DIR="$HOME/freerdp-krb5-build"
mkdir -p "$BUILD_DIR"

if [[ ! -f "$BUILD_DIR/freerdp-3.26.0.tar.gz" ]]; then
    echo "  Downloading source..."
    curl -L -o "$BUILD_DIR/freerdp-3.26.0.tar.gz" \
        "https://github.com/FreeRDP/FreeRDP/archive/refs/tags/3.26.0.tar.gz"
fi

cd "$BUILD_DIR"
if [[ ! -d FreeRDP-3.26.0 ]]; then
    echo "  Extracting..."
    tar xzf freerdp-3.26.0.tar.gz
fi

# macOS Sequoia compatibility: replace Mac shadow server with X11 one
sed -i '' 's/add_subdirectory(Mac)/add_subdirectory(X11)/' \
    FreeRDP-3.26.0/server/shadow/CMakeLists.txt

mkdir -p "$INSTALL_DIR"
echo "  Configuring..."

"${BREW}/bin/cmake" \
    -S FreeRDP-3.26.0 -B build \
    -DCMAKE_INSTALL_PREFIX="$INSTALL_DIR" \
    -DCMAKE_INSTALL_NAME_DIR="$INSTALL_DIR/lib" \
    -DBUILD_SHARED_LIBS=ON \
    -DWITH_X11=ON \
    -DWITH_JPEG=ON \
    -DWITH_MANPAGES=OFF \
    -DWITH_WEBVIEW=OFF \
    -DWITH_CLIENT_SDL=ON \
    -DWITH_CLIENT_SDL2=OFF \
    -DWITH_CLIENT_SDL3=ON \
    -DWITH_CLIENT_MAC=OFF \
    -DWITH_PLATFORM_SERVER=OFF \
    -DWITH_KRB5=ON \
    -DKRB5_ROOT_CONFIG="${BREW}/opt/krb5/bin/krb5-config" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_PREFIX_PATH="${BREW};/opt/X11" \
    -DCMAKE_IGNORE_PREFIX_PATH=/opt/local \
    -DCMAKE_INSTALL_RPATH="${INSTALL_DIR}/lib;${BREW}/lib;/opt/X11/lib" \
    -DWITH_VERBOSE_WINPR_ASSERT=OFF \
    -DWITH_VIDEOTOOLBOX=ON \
    -DWITH_GFX_H264=ON \
    -DWITH_DEBUG_ALL=OFF \
    -DWITH_VAAPI=OFF \
    -DWITH_VAAPI_H264_ENCODING=OFF \
    > /dev/null

echo "  Compiling (this takes a few minutes)..."
"${BREW}/bin/cmake" --build build -j$(sysctl -n hw.ncpu)

echo "  Installing..."
"${BREW}/bin/cmake" --install build > /dev/null

echo ""
echo "[4/4] Setup complete!"
echo ""
echo "Run: ~/gewis-rdp/connect.sh"
