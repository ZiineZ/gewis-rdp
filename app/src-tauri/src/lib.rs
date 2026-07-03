use std::{
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
use std::io::Write;

use tauri::{Emitter, Manager};

const REALM:   &str = "GEWISWG.GEWIS.NL";
const GATEWAY: &str = "gewisvdesktop.gewis.nl";
const TARGET:  &str = "gewisvdesktop.gewis.nl";

#[cfg(target_os = "macos")]
const CCACHE:  &str = "FILE:/tmp/krb5cc_gewis_rdp";

// ── Helpers ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn brew_prefix() -> Result<String, String> {
    for prefix in &["/opt/homebrew", "/usr/local"] {
        if Path::new(&format!("{}/bin/brew", prefix)).exists() {
            return Ok(prefix.to_string());
        }
    }
    Err("Homebrew not found. Install it from https://brew.sh".into())
}

#[cfg(target_os = "macos")]
fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn status(app: &tauri::AppHandle, msg: &str) {
    app.emit("status", msg).ok();
}

/// Resolve FreeRDP path:
/// - macOS: prefer bundled `sdl-freerdp`, fall back to `~/opt/freerdp-krb5/bin/sdl-freerdp`
/// - Windows: prefer bundled `wfreerdp.exe`, fall back to standard installation paths.
fn freerdp_path(app: &tauri::AppHandle) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(dir) = app.path().resource_dir() {
            let p = dir.join("resources").join("sdl-freerdp");
            if p.is_file() {
                return Ok(p.to_string_lossy().into());
            }
        }
        let p = format!("{}/opt/freerdp-krb5/bin/sdl-freerdp", home());
        if Path::new(&p).exists() {
            return Ok(p);
        }
        Err("FreeRDP not found. Please reinstall the app.".into())
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(dir) = app.path().resource_dir() {
            let p = dir.join("resources").join("wfreerdp.exe");
            if p.is_file() {
                return Ok(p.to_string_lossy().into());
            }
        }
        let p = "C:\\Program Files\\FreeRDP\\wfreerdp.exe";
        if Path::new(p).exists() {
            return Ok(p.into());
        }
        Err("FreeRDP (wfreerdp.exe) not found. Please place wfreerdp.exe in resources.".into())
    }
}

#[tauri::command]
fn open_url(url: String) {
    #[cfg(target_os = "macos")]
    Command::new("open").arg(url).spawn().ok();

    #[cfg(target_os = "windows")]
    Command::new("cmd").args(["/c", "start", "", &url]).spawn().ok();
}

// ── Credential storage (macOS Keychain via the built-in `security` CLI) ────────
// ponytail: shell out to `security` rather than pull in the keyring crate — it's
// native, zero-dep, and the password already transits argv via FreeRDP's /p:,
// so `-w <pw>`'s brief argv exposure adds no new risk on a single-user Mac.
const KEYCHAIN_SERVICE: &str = "GEWIS Remote Desktop";

#[tauri::command]
#[allow(unused_variables)]
fn save_credentials(member: String, password: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let ok = Command::new("security")
            .args(["add-generic-password", "-U",
                   "-s", KEYCHAIN_SERVICE, "-a", &member, "-w", &password])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err("Could not save to Keychain.".into());
        }
    }
    Ok(())
}

#[tauri::command]
#[allow(unused_variables)]
fn load_password(member: String) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("security")
            .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", &member, "-w"])
            .output()
            .ok()?;
        if out.status.success() {
            let pw = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
            if !pw.is_empty() {
                return Some(pw);
            }
        }
    }
    None
}

#[tauri::command]
#[allow(unused_variables)]
fn forget_credentials(member: String) {
    #[cfg(target_os = "macos")]
    {
        Command::new("security")
            .args(["delete-generic-password", "-s", KEYCHAIN_SERVICE, "-a", &member])
            .status()
            .ok();
    }
}

// ── Auto-update (checks GitHub Releases, see plugins.updater in tauri.conf) ─────
#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(update.version)),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn do_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check().await.map_err(|e| e.to_string())?
        .ok_or("No update available")?;
    update
        .download_and_install(|_downloaded, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

// ── First-run Kerberos config (macOS) ──────────────────────────────────────────
// Writes /etc/krb5.conf with the GEWIS realm via a single admin prompt, so users
// don't have to hand-edit a system file. Leaves an existing config untouched.
#[cfg(target_os = "macos")]
fn ensure_krb5_conf(app: &tauri::AppHandle) -> Result<(), String> {
    const CONF: &str = "/etc/krb5.conf";
    const CONTENT: &str = "[libdefaults]\n  default_realm = GEWISWG.GEWIS.NL\n  rdns = false\n\n[realms]\n  GEWISWG.GEWIS.NL = {\n    kdc = https://gewisvdesktop.gewis.nl/KdcProxy\n  }\n";

    if let Ok(existing) = std::fs::read_to_string(CONF) {
        if existing.contains(REALM) {
            return Ok(()); // already configured
        }
        // Don't clobber a config that has other realms — show the snippet instead.
        return Err(format!(
            "Your {} is missing the GEWIS realm. Add this block, then reconnect:\n\n{}",
            CONF, CONTENT
        ));
    }

    status(app, "Configuring Kerberos (one-time)...");
    let tmp = "/tmp/gewis_krb5.conf";
    std::fs::write(tmp, CONTENT).map_err(|e| format!("temp write failed: {}", e))?;

    // osascript shows the native macOS admin password dialog.
    let script = format!(
        "do shell script \"cp {} {} && chmod 644 {}\" with administrator privileges",
        tmp, CONF, CONF
    );
    let ok = Command::new("osascript")
        .args(["-e", &script])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return Err("Kerberos setup was cancelled — it's required to log in.".into());
    }
    Ok(())
}

/// Probe whether the GEWIS RDP server is reachable directly on TCP 3389.
/// True on TU/e WiFi (and some VPNs), false from the public internet.
/// Used to decide whether to skip the HTTPS gateway tunnel.
fn is_direct_reachable() -> bool {
    let addr = match format!("{}:3389", TARGET).to_socket_addrs() {
        Ok(mut iter) => match iter.next() {
            Some(a) => a,
            None => return false,
        },
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).is_ok()
}

// ── Connect command ───────────────────────────────────────────────────────────
// Returns immediately so the UI stays responsive.
// All blocking work runs on a background thread.

#[tauri::command]
fn connect(
    app: tauri::AppHandle,
    member: String,
    password: String,
    display: String,
    scale: String,
    clipboard: bool,
    sound: bool,
) -> Result<(), String> {
    if password.contains(',') {
        return Err(
            "Passwords containing commas are not supported. \
             Please change your GEWIS password and try again."
                .into(),
        );
    }

    thread::spawn(move || {
        if let Err(e) = run_connect(&app, &member, &password, &display, &scale, clipboard, sound) {
            app.emit("connect-error", e).ok();
        }
    });

    Ok(())
}

fn run_connect(
    app: &tauri::AppHandle,
    member: &str,
    password: &str,
    display: &str,
    scale: &str,
    clipboard: bool,
    sound: bool,
) -> Result<(), String> {
    status(app, "Checking prerequisites...");

    let freerdp = freerdp_path(app)?;

    #[cfg(target_os = "macos")]
    let ccache = {
        let brew = brew_prefix()?;

        // First-run: make sure /etc/krb5.conf has the GEWIS realm.
        ensure_krb5_conf(app)?;

        let kinit = format!("{}/opt/krb5/bin/kinit", brew);
        if !Path::new(&kinit).exists() {
            return Err("MIT Kerberos not found. Run: brew install krb5".into());
        }

        // Kerberos ticket via Homebrew krb5
        status(app, "Contacting GEWIS identity server...");

        let mut proc = Command::new(&kinit)
            .args(["-c", CCACHE, &format!("{}@{}", member, REALM)])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start kinit: {}", e))?;

        status(app, "Sending credentials...");

        proc.stdin
            .as_mut()
            .ok_or("kinit stdin unavailable")?
            .write_all(format!("{}\n", password).as_bytes())
            .map_err(|e| e.to_string())?;

        drop(proc.stdin.take());

        status(app, "Waiting for Kerberos response...");

        if !proc.wait().map_err(|e| e.to_string())?.success() {
            return Err(
                "Authentication failed. Are you on TU/e WiFi or is your VPN on?".into(),
            );
        }

        status(app, "Identity confirmed.");
        CCACHE
    };

    // ── Network-path detection ────────────────────────────────────────────────
    //
    // On TU/e WiFi the RDP server is reachable directly on TCP 3389, which
    // is dramatically faster than the HTTPS gateway. The gateway is only
    // needed when off-campus.
    //
    // Probe port 3389 with a 1.5 s timeout. If reachable, use direct mode.
    // If not, fall through to the gateway path.
    let direct_ok = is_direct_reachable();

    if direct_ok {
        status(app, "On TU/e network — using direct connection.");
    } else {
        status(app, "Off-campus — connecting via gateway...");
    }
    thread::sleep(Duration::from_millis(200));

    let mut args: Vec<String> = Vec::new();

    if direct_ok {
        // Direct TCP — fastest path. No gateway overhead, default flags suffice.
        args.push(format!("/v:{}:3389", TARGET));
    } else {
        // Gateway path — slower but works from anywhere.
        args.push(format!("/v:{}", TARGET));
        args.push(format!(
            "/gateway:g:{},u:{},d:{},p:{},type:http",
            GATEWAY, member, REALM, password
        ));
    }

    args.extend([
        format!("/u:{}", member),
        format!("/d:{}", REALM),
        format!("/p:{}", password),
        "/sec:nla".into(),
        "/cert:ignore".into(),
        "+credentials-delegation".into(),
        "/log-level:OFF".into(),
        // Cosmetic flags — Windows skips drawing these server-side, less data.
        "-wallpaper".into(),
        "-window-drag".into(),
        "-menu-anims".into(),
        "-themes".into(),
    ]);

    match display {
        // Real macOS fullscreen (own Space via SDL_VIDEO_MAC_FULLSCREEN_SPACES).
        "fullscreen"  => args.push("/f".into()),
        // Resizable window; +dynamic-resolution reflows the remote desktop on resize.
        "windowed"    => args.push("+dynamic-resolution".into()),
        // Fullscreen spanning every monitor.
        "allmonitors" => { args.push("/f".into()); args.push("/multimon".into()); }
        _             => args.push("/f".into()),
    }

    // HiDPI: tell the server to render the desktop at this DPI scale, the same
    // way Windows' "Display scaling" setting does. Without this, Retina panels
    // report their full physical pixel count and everything renders tiny.
    // ponytail: 200 default = Retina 2x; dropdown covers the rest.
    if scale != "100" {
        args.push(format!("/scale-desktop:{}", scale));
    }

    args.push(if clipboard { "+clipboard".into() } else { "-clipboard".into() });
    args.push(if sound     { "/sound".into()     } else { "-sound".into()     });

    status(app, "Opening remote desktop...");

    // Spawn the FreeRDP client process.
    let mut cmd = Command::new(&freerdp);
    cmd.args(&args);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    #[cfg(target_os = "macos")]
    {
        cmd.env("KRB5CCNAME", ccache);
        cmd.env("SDL_VIDEO_MAC_FULLSCREEN_SPACES", "1");
    }

    let launched = cmd.spawn().is_ok();

    app.emit(
        "status",
        if launched {
            "Connected. Remote desktop is opening."
        } else {
            "Failed to launch. Try reinstalling."
        },
    ).ok();

    Ok(())
}

// ── App entry point ───────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            connect,
            open_url,
            save_credentials,
            load_password,
            forget_credentials,
            check_for_update,
            do_update,
        ])
        .run(tauri::generate_context!())
        .expect("error running GEWIS Remote Desktop");
}
