use std::{
    io::Write,
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use tauri::{Emitter, Manager};

const REALM:   &str = "GEWISWG.GEWIS.NL";
const GATEWAY: &str = "gewisvdesktop.gewis.nl";
const TARGET:  &str = "gewisvdesktop.gewis.nl";
const CCACHE:  &str = "FILE:/tmp/krb5cc_gewis_rdp";

// ── Helpers ───────────────────────────────────────────────────────────────────

fn brew_prefix() -> Result<String, String> {
    for prefix in &["/opt/homebrew", "/usr/local"] {
        if Path::new(&format!("{}/bin/brew", prefix)).exists() {
            return Ok(prefix.to_string());
        }
    }
    Err("Homebrew not found. Install it from https://brew.sh".into())
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn status(app: &tauri::AppHandle, msg: &str) {
    app.emit("status", msg).ok();
}

/// Resolve sdl-freerdp: prefer the binary bundled inside the .app,
/// fall back to a manually built binary at ~/opt/freerdp-krb5 for development.
fn freerdp_path(app: &tauri::AppHandle) -> Result<String, String> {
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

#[tauri::command]
fn open_url(url: String) {
    Command::new("open").arg(url).spawn().ok();
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
        if let Err(e) = run_connect(&app, &member, &password, &display, clipboard, sound) {
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
    clipboard: bool,
    sound: bool,
) -> Result<(), String> {
    status(app, "Checking prerequisites...");

    let freerdp = freerdp_path(app)?;
    let brew    = brew_prefix()?;

    // Kerberos ticket via Homebrew krb5
    status(app, "Contacting GEWIS identity server...");

    let kinit = format!("{}/opt/krb5/bin/kinit", brew);
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
        "smart"       => args.push("+dynamic-resolution".into()),
        "fullscreen"  => args.push("/f".into()),
        "allmonitors" => { args.push("/f".into()); args.push("/multimon".into()); }
        _             => {}
    }

    args.push(if clipboard { "+clipboard".into() } else { "-clipboard".into() });
    args.push(if sound     { "/sound".into()     } else { "-sound".into()     });

    status(app, "Opening remote desktop...");

    // No XQuartz — sdl-freerdp uses Metal directly via SDL3
    let launched = Command::new(&freerdp)
        .args(&args)
        .env("KRB5CCNAME", CCACHE)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok();

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
        .invoke_handler(tauri::generate_handler![connect, open_url])
        .run(tauri::generate_context!())
        .expect("error running GEWIS Remote Desktop");
}
