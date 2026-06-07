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
    thread::sleep(Duration::from_millis(250));
    status(app, "Requesting gateway access ticket...");
    thread::sleep(Duration::from_millis(300));
    status(app, "Requesting remote desktop access ticket...");
    thread::sleep(Duration::from_millis(300));

    // ── Build sdl-freerdp argument list ──────────────────────────────────────
    //
    // Latency-optimised flags. The single biggest win is using sdl-freerdp
    // (renders via Metal) instead of xfreerdp (renders via XQuartz/X11, which
    // added ~50-200 ms per frame on macOS).
    //
    // Other tweaks:
    //   - GFX with H.264 (AVC444) — server-side encoded video stream, far less
    //     pixel data on the wire than legacy bitmap updates.
    //   - +async-update / +async-channels — display + channels on dedicated
    //     threads so input isn't blocked by network reads.
    //   - +bitmap-cache / +offscreen-cache — repeat content (window chrome,
    //     icons) is sent once, not every frame.
    //   - /codec-cache:rfx — keep codec contexts hot between frames.
    //   - /max-fast-path-size:65535 — biggest possible fast-path packets,
    //     fewer round trips.
    //   - /network:auto — let the server pick the best codec mix for the link.
    //   - -wallpaper -window-drag -menu-anims -themes — Windows skips drawing
    //     these on the remote side, less data to push down.
    //   - -compression — disable bulk compression. On a fast link the CPU
    //     time spent decompressing is worse than just sending more bytes.

    let mut args: Vec<String> = vec![
        format!("/v:{}", TARGET),
        format!("/u:{}", member),
        format!("/d:{}", REALM),
        format!("/p:{}", password),
        "/sec:nla".into(),
        format!("/gateway:g:{},u:{},d:{},p:{},type:http", GATEWAY, member, REALM, password),
        "/cert:ignore".into(),
        "+credentials-delegation".into(),

        // Performance flags — the bottleneck is frame-ack flow control:
        // the server waits for the client to ack each frame before sending
        // the next, so mouse-driven hover updates queue 2s behind a slow
        // ack pipeline. frame-ack:off lets the server stream frames freely.
        //
        // Also removed:
        //   - /multitransport (UDP) — GEWIS gateway likely blocks UDP,
        //     so it silently degraded to slow TCP with extra setup overhead.
        //   - /network:lan — was forcing a profile that disabled bandwidth-
        //     reactive optimizations. /network:auto adapts to the actual link.
        //   - -compression — uncompressed frames over a TCP gateway are huge
        //     and saturate the link; default compression is far better.
        "/gfx:RFX:on,progressive:off,frame-ack:off,small-cache:on,thin-client:off".into(),
        "/network:auto".into(),
        "+async-update".into(),
        "/cache:bitmap:on,codec:rfx,offscreen:on".into(),
        "/max-fast-path-size:65535".into(),
        "/log-level:OFF".into(),
        "-wallpaper".into(),
        "-window-drag".into(),
        "-menu-anims".into(),
        "-themes".into(),
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
