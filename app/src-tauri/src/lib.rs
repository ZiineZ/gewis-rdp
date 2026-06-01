use std::{
    io::Write,
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

fn freerdp_path(app: &tauri::AppHandle) -> Result<String, String> {
    if let Ok(dir) = app.path().resource_dir() {
        let p = dir.join("resources").join("xfreerdp");
        if p.is_file() {
            return Ok(p.to_string_lossy().into());
        }
    }
    let p = format!("{}/opt/freerdp-krb5/bin/xfreerdp", home());
    if Path::new(&p).exists() {
        return Ok(p);
    }
    Err("FreeRDP not found. Please reinstall the app or run setup.sh.".into())
}

// ── Connect command ───────────────────────────────────────────────────────────
// Returns immediately so the UI stays responsive.
// All blocking work runs on a background thread.
// Errors are sent back as "connect-error" events.

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

    // Auto-install XQuartz if missing
    if !Path::new("/Applications/Utilities/XQuartz.app").exists()
        && !Path::new("/opt/X11").exists()
    {
        status(app, "Installing XQuartz...");
        let ok = Command::new(format!("{}/bin/brew", brew))
            .args(["install", "--cask", "xquartz"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err("XQuartz install failed. Run: brew install --cask xquartz".into());
        }
    }

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
    thread::sleep(Duration::from_millis(250));
    status(app, "Requesting gateway access ticket...");
    thread::sleep(Duration::from_millis(300));
    status(app, "Requesting remote desktop access ticket...");
    thread::sleep(Duration::from_millis(300));

    // Build xfreerdp argument list
    let mut args: Vec<String> = vec![
        format!("/v:{}", TARGET),
        format!("/u:{}", member),
        format!("/d:{}", REALM),
        format!("/p:{}", password),
        "/sec:nla".into(),
        format!("/gateway:g:{},u:{},d:{},p:{},type:http", GATEWAY, member, REALM, password),
        "/cert:ignore".into(),
        "+credentials-delegation".into(),
    ];

    match display {
        "smart"       => args.push("+dynamic-resolution".into()),
        "fullscreen"  => args.push("/f".into()),
        "allmonitors" => { args.push("/f".into()); args.push("/multimon".into()); }
        _             => {}
    }

    args.push(if clipboard { "+clipboard".into() } else { "-clipboard".into() });
    args.push(if sound     { "/sound".into()     } else { "-sound".into()     });

    status(app, "Opening remote desktop...");

    Command::new("open").args(["-a", "XQuartz"]).spawn().ok();
    thread::sleep(Duration::from_secs(3));

    let launched = Command::new(&freerdp)
        .args(&args)
        .env("KRB5CCNAME", CCACHE)
        .env("DISPLAY", ":0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok();

    app.emit(
        "status",
        if launched {
            "Connected. Remote desktop is opening."
        } else {
            "Failed to launch. Try reinstalling or running setup.sh."
        },
    ).ok();

    Ok(())
}

// ── App entry point ───────────────────────────────────────────────────────────

#[tauri::command]
fn open_url(url: String) {
    Command::new("open").arg(url).spawn().ok();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![connect, open_url])
        .run(tauri::generate_context!())
        .expect("error running GEWIS Remote Desktop");
}
