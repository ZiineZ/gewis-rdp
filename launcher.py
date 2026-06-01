#!/usr/bin/env python3
"""GEWIS Remote Desktop — local web launcher"""

import http.server, socketserver, json, subprocess, os, sys, threading, time, webbrowser

# ── Config ────────────────────────────────────────────────────────────────────

PORT    = 7070
HOST    = "127.0.0.1"
REALM   = "GEWISWG.GEWIS.NL"
GATEWAY = "gewisvdesktop.gewis.nl"
TARGET  = "gewisvdesktop.gewis.nl"
CCACHE  = "FILE:/tmp/krb5cc_gewis_rdp"

try:
    BREW = subprocess.check_output(["brew","--prefix"], text=True, stderr=subprocess.DEVNULL).strip()
except Exception:
    sys.exit("Homebrew not found — run setup.sh first.")

KINIT   = os.path.join(BREW, "opt/krb5/bin/kinit")
FREERDP = os.path.expanduser("~/opt/freerdp-krb5/bin/xfreerdp")

if not os.path.isfile(FREERDP):
    sys.exit("FreeRDP not found — run setup.sh first.")

# ── Embedded UI ───────────────────────────────────────────────────────────────

HTML = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>GEWIS Remote Desktop</title>
<style>
  :root {
    --red:    #c0392b;
    --red-dk: #962d22;
    --red-lt: #fdf2f1;
    --bg:     #f0f2f5;
    --card:   #ffffff;
    --border: #e2e5ea;
    --text:   #1c1c2e;
    --muted:  #6b7280;
    --ok:     #16a34a;
    --ok-bg:  #f0fdf4;
    --err-bg: #fef2f2;
    --info-bg:#eff6ff;
    --info:   #1d4ed8;
    --radius: 10px;
  }
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    background: var(--bg);
    min-height: 100dvh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px 16px;
  }
  .card {
    background: var(--card);
    border-radius: 16px;
    box-shadow: 0 1px 3px rgba(0,0,0,.06), 0 8px 32px rgba(0,0,0,.08);
    padding: 36px 32px 32px;
    width: 100%;
    max-width: 420px;
  }

  /* Header */
  .header { display: flex; align-items: center; gap: 14px; margin-bottom: 28px; }
  .logo {
    width: 46px; height: 46px; border-radius: 12px;
    background: var(--red);
    display: flex; align-items: center; justify-content: center;
    color: #fff; font-size: 22px; font-weight: 800; letter-spacing: -1px;
    flex-shrink: 0; user-select: none;
  }
  .header-text h1 { font-size: 18px; font-weight: 700; color: var(--text); }
  .header-text p  { font-size: 13px; color: var(--muted); margin-top: 1px; }

  /* Divider */
  .divider { border: none; border-top: 1px solid var(--border); margin: 20px 0; }

  /* Form fields */
  .field { margin-bottom: 14px; }
  .field label { display: block; font-size: 13px; font-weight: 600; color: var(--text); margin-bottom: 5px; }
  .field input {
    width: 100%; padding: 10px 13px;
    border: 1.5px solid var(--border); border-radius: var(--radius);
    font-size: 15px; color: var(--text); outline: none;
    transition: border-color .15s;
    background: #fff;
  }
  .field input:focus { border-color: var(--red); }
  .field input::placeholder { color: #b0b8c4; }

  /* Section labels */
  .section-label {
    font-size: 11px; font-weight: 700; letter-spacing: .06em;
    text-transform: uppercase; color: var(--muted);
    margin-bottom: 10px;
  }

  /* Display mode buttons */
  .display-grid {
    display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px;
    margin-bottom: 18px;
  }
  .display-btn {
    border: 1.5px solid var(--border); border-radius: var(--radius);
    padding: 10px 4px; text-align: center; cursor: pointer;
    transition: all .15s; background: #fff; user-select: none;
  }
  .display-btn:hover { border-color: var(--red); background: var(--red-lt); }
  .display-btn.active { border-color: var(--red); background: var(--red-lt); }
  .display-btn input { display: none; }
  .display-icon { font-size: 18px; display: block; margin-bottom: 4px; }
  .display-name { font-size: 11px; font-weight: 600; color: var(--text); display: block; }
  .display-desc { font-size: 10px; color: var(--muted); display: block; margin-top: 1px; }

  /* Toggle rows */
  .toggle-row {
    display: flex; align-items: center; justify-content: space-between;
    padding: 11px 0; border-bottom: 1px solid var(--border);
  }
  .toggle-row:last-child { border-bottom: none; }
  .toggle-info { }
  .toggle-info .toggle-name { font-size: 14px; font-weight: 500; color: var(--text); }
  .toggle-info .toggle-desc { font-size: 12px; color: var(--muted); margin-top: 1px; }

  .switch { position: relative; width: 42px; height: 24px; flex-shrink: 0; }
  .switch input { opacity: 0; width: 0; height: 0; }
  .track {
    position: absolute; inset: 0; border-radius: 24px;
    background: #d1d5db; cursor: pointer; transition: background .2s;
  }
  .track::before {
    content: ''; position: absolute;
    width: 18px; height: 18px; left: 3px; top: 3px;
    border-radius: 50%; background: #fff;
    box-shadow: 0 1px 3px rgba(0,0,0,.2);
    transition: transform .2s;
  }
  .switch input:checked ~ .track { background: var(--red); }
  .switch input:checked ~ .track::before { transform: translateX(18px); }

  /* Connect button */
  #connectBtn {
    width: 100%; padding: 13px;
    margin-top: 20px;
    background: var(--red); color: #fff;
    border: none; border-radius: var(--radius);
    font-size: 15px; font-weight: 700;
    cursor: pointer; transition: background .15s;
    letter-spacing: .01em;
  }
  #connectBtn:hover:not(:disabled) { background: var(--red-dk); }
  #connectBtn:disabled { background: #d1d5db; cursor: not-allowed; }

  /* Status */
  #status { display: none; margin-top: 14px; padding: 12px 14px; border-radius: var(--radius); font-size: 13px; line-height: 1.4; }
  #status.error   { display: block; background: var(--err-bg); color: var(--red); }
  #status.success { display: block; background: var(--ok-bg);  color: var(--ok); }
  #status.loading { display: block; background: var(--info-bg); color: var(--info); }
</style>
</head>
<body>
<div class="card">

  <div class="header">
    <div class="logo">G</div>
    <div class="header-text">
      <h1>GEWIS Remote Desktop</h1>
      <p>Virtual desktop access via RD Gateway</p>
    </div>
  </div>

  <div class="field">
    <label for="member">Member number</label>
    <input id="member" type="text" placeholder="m11494" autocomplete="username" spellcheck="false">
  </div>
  <div class="field">
    <label for="password">Password</label>
    <input id="password" type="password" placeholder="••••••••" autocomplete="current-password">
  </div>

  <hr class="divider">

  <div class="section-label">Display mode</div>
  <div class="display-grid">
    <label class="display-btn" id="d-smart">
      <input type="radio" name="display" value="smart" checked>
      <span class="display-icon">⊡</span>
      <span class="display-name">Smart</span>
      <span class="display-desc">Auto-resize</span>
    </label>
    <label class="display-btn" id="d-fullscreen">
      <input type="radio" name="display" value="fullscreen">
      <span class="display-icon">⛶</span>
      <span class="display-name">Fullscreen</span>
      <span class="display-desc">This display</span>
    </label>
    <label class="display-btn" id="d-allmonitors">
      <input type="radio" name="display" value="allmonitors">
      <span class="display-icon">▣</span>
      <span class="display-name">All screens</span>
      <span class="display-desc">Span all</span>
    </label>
    <label class="display-btn" id="d-windowed">
      <input type="radio" name="display" value="windowed">
      <span class="display-icon">▭</span>
      <span class="display-name">Windowed</span>
      <span class="display-desc">Fixed size</span>
    </label>
  </div>

  <div class="section-label">Options</div>
  <div>
    <div class="toggle-row">
      <div class="toggle-info">
        <div class="toggle-name">Clipboard</div>
        <div class="toggle-desc">Share copy/paste with remote desktop</div>
      </div>
      <label class="switch">
        <input type="checkbox" id="clipboard" checked>
        <span class="track"></span>
      </label>
    </div>
    <div class="toggle-row">
      <div class="toggle-info">
        <div class="toggle-name">Sound</div>
        <div class="toggle-desc">Play audio from the remote desktop</div>
      </div>
      <label class="switch">
        <input type="checkbox" id="sound" checked>
        <span class="track"></span>
      </label>
    </div>
  </div>

  <button id="connectBtn" onclick="connect()">Connect</button>
  <div id="status"></div>

</div>

<script>
  // Highlight active display mode card
  document.querySelectorAll("input[name=display]").forEach(r =>
    r.addEventListener("change", () => {
      document.querySelectorAll(".display-btn").forEach(b => b.classList.remove("active"));
      r.closest(".display-btn").classList.add("active");
    })
  );
  // Set initial highlight
  document.querySelector("input[name=display]:checked").closest(".display-btn").classList.add("active");

  document.addEventListener("keydown", e => { if (e.key === "Enter") connect(); });

  async function connect() {
    const member   = document.getElementById("member").value.trim();
    const password = document.getElementById("password").value;
    const display  = document.querySelector("input[name=display]:checked").value;
    const clipboard = document.getElementById("clipboard").checked;
    const sound     = document.getElementById("sound").checked;

    if (!member)                      return status("Enter your member number.", "error");
    if (!/^[mM]\\d+$/.test(member))  return status("Member number should look like m11494.", "error");
    if (!password)                    return status("Enter your password.", "error");

    const btn = document.getElementById("connectBtn");
    btn.disabled = true;
    btn.textContent = "Connecting…";
    status("Getting Kerberos ticket…", "loading");

    try {
      const res = await fetch("/connect", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ member, password, display, clipboard, sound })
      });
      const d = await res.json();
      if (d.success) {
        status("✓ Authenticated — remote desktop will open in a moment.", "success");
        btn.textContent = "Launch another session";
      } else {
        status(d.error || "Connection failed.", "error");
        btn.textContent = "Connect";
      }
    } catch {
      status("Could not reach the launcher. Is launcher.py still running?", "error");
      btn.textContent = "Connect";
    }
    btn.disabled = false;
  }

  function status(msg, type) {
    const el = document.getElementById("status");
    el.textContent = msg;
    el.className = type;
  }
</script>
</body>
</html>"""

# ── HTTP handler ──────────────────────────────────────────────────────────────

class Handler(http.server.BaseHTTPRequestHandler):

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.end_headers()
        self.wfile.write(HTML.encode())

    def do_POST(self):
        if self.path != "/connect":
            self.send_response(404); self.end_headers(); return

        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        data = json.loads(body)

        member   = data.get("member",   "").strip()
        password = data.get("password", "")
        display  = data.get("display",  "smart")
        clipboard = data.get("clipboard", True)
        sound     = data.get("sound",     True)

        if not member or not password:
            return self._json({"success": False, "error": "Missing credentials."})

        # FreeRDP embeds the password in a comma-delimited gateway flag — commas break it
        if "," in password:
            return self._json({"success": False,
                "error": "Passwords containing commas are not supported. Please change your GEWIS password and try again."})

        # ── kinit ─────────────────────────────────────────────────────────────
        kr = subprocess.run(
            [KINIT, "-c", CCACHE, f"{member}@{REALM}"],
            input=password + "\n", text=True, capture_output=True
        )
        if kr.returncode != 0:
            return self._json({"success": False,
                "error": "Kerberos authentication failed — check your member number and password."})

        # ── Build xfreerdp command ────────────────────────────────────────────
        cmd = [
            FREERDP,
            f"/v:{TARGET}",
            f"/u:{member}",
            f"/d:{REALM}",
            f"/p:{password}",
            "/sec:nla",
            f"/gateway:g:{GATEWAY},u:{member},d:{REALM},p:{password},type:http",
            "/cert:ignore",
            "+credentials-delegation",
        ]

        if   display == "smart":       cmd.append("+dynamic-resolution")
        elif display == "fullscreen":  cmd.append("/f")
        elif display == "allmonitors": cmd += ["/f", "/multimon"]
        # "windowed" → no extra display flags

        cmd.append("+clipboard" if clipboard else "-clipboard")
        cmd.append("/sound"     if sound     else "-sound")

        # ── Launch (async: XQuartz → 3s → xfreerdp) ──────────────────────────
        env = {**os.environ, "KRB5CCNAME": CCACHE, "DISPLAY": ":0"}

        def launch():
            subprocess.Popen(["open", "-a", "XQuartz"])
            time.sleep(3)
            subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        threading.Thread(target=launch, daemon=True).start()
        self._json({"success": True})

    def _json(self, data):
        body = json.dumps(data).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_): pass   # silence request logs

# ── Entry point ───────────────────────────────────────────────────────────────

if __name__ == "__main__":
    url = f"http://{HOST}:{PORT}"
    print(f"  GEWIS Remote Desktop Launcher")
    print(f"  {url}")
    print(f"  Ctrl+C to stop\n")

    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer((HOST, PORT), Handler) as srv:
        threading.Timer(1.0, lambda: webbrowser.open(url)).start()
        try:
            srv.serve_forever()
        except KeyboardInterrupt:
            print("\nStopped.")
