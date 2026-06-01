# GEWIS Remote Desktop for macOS

A native macOS app for connecting to the GEWIS virtual desktop via Kerberos
authentication. Built with [Tauri 2](https://v2.tauri.app/) (Rust + WebView),
no Electron.

![Screenshot](docs/screenshot.png)

## Why this exists

- The **Windows App** (Microsoft Remote Desktop) fails with `0x3707` because
  the GEWIS RD Gateway requires Kerberos, not NTLM.
- The **Homebrew FreeRDP bottle** is compiled with `WITH_KRB5=OFF` on macOS,
  so it cannot use a Kerberos ticket either.

This project ships a pre-built FreeRDP with `WITH_KRB5=ON` inside the `.app`
bundle, plus a tiny native GUI for entering credentials and launching the
session.

---

## Install (end users)

1. Install **Homebrew** from [brew.sh](https://brew.sh) if you don't have it.
2. Install the dependencies:
   ```sh
   brew install krb5
   brew install --cask xquartz
   ```
3. Add the GEWIS Kerberos realm to `/etc/krb5.conf` (needs sudo):
   ```sh
   sudo tee /etc/krb5.conf > /dev/null <<'EOF'
   [libdefaults]
     default_realm = GEWISWG.GEWIS.NL
     rdns = false

   [realms]
     GEWISWG.GEWIS.NL = {
       kdc = https://gewisvdesktop.gewis.nl/KdcProxy
     }
   EOF
   ```
4. Drag **GEWIS Remote Desktop.app** into `/Applications/`.
5. Clear the Gatekeeper quarantine flag (one-time, the app isn't notarised):
   ```sh
   xattr -cr "/Applications/GEWIS Remote Desktop.app"
   ```
6. Launch it from Spotlight or `/Applications/`.

---

## Build from source

```sh
# One-time: build the KRB5-enabled FreeRDP into ~/opt/freerdp-krb5
./setup.sh

# Bundle FreeRDP into the Tauri project and build the .app
cd app && ./build-app.sh
```

The resulting `.app` is at
`app/src-tauri/target/release/bundle/macos/GEWIS Remote Desktop.app`.

### Requirements

- macOS 11+
- Homebrew (`/opt/homebrew` on Apple Silicon, `/usr/local` on Intel)
- Rust 1.88+ (`brew install rust` or rustup)
- Tauri CLI 2 (`cargo install tauri-cli --version "^2.0"`)

---

## How it works

1. The GUI collects the member number + password.
2. Rust spawns `kinit` to obtain a Kerberos TGT from the KDC proxy at
   `https://gewisvdesktop.gewis.nl/KdcProxy`, storing it in a file cache.
3. Rust spawns the bundled `xfreerdp` with:
   - `/sec:nla` (forces Kerberos for NLA)
   - `/gateway:...,type:http` (HTTPS gateway through the same host)
   - `KRB5CCNAME=FILE:/tmp/krb5cc_gewis_rdp`
4. FreeRDP uses the ticket to authenticate the gateway connection and the
   RDP session.

XQuartz is required because FreeRDP is an X11 application.

---

## Project layout

```
gewis-rdp/
├── app/                          ← Tauri app source
│   ├── src/                      ← HTML/CSS/JS frontend
│   │   ├── index.html
│   │   ├── gewis-logo.svg        ← In-app logo
│   │   └── GewisRDP.svg          ← Source for the app icon
│   ├── src-tauri/                ← Rust backend + Tauri config
│   │   ├── src/lib.rs
│   │   ├── tauri.conf.json
│   │   └── icons/
│   └── build-app.sh              ← Bundle FreeRDP, build, sign
├── setup.sh                      ← Compile FreeRDP with KRB5=ON
├── connect.sh                    ← CLI fallback for the same flow
└── README.md
```

---

## Credits

Made with ❤️ by [Stan Theunissen](https://gewis.nl/en/member/11494).
