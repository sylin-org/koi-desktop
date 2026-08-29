//! Koi's desktop workbench: a friendly native shell over the local substrate.
//!
//! Visual language and desktop patterns are borrowed from Ghostlight
//! (`sylin-org/browser-mcp`, `crates/orchestrator/src/desktop/mod.rs`) and
//! re-skinned with the Koi identity published on sylin.org (accent `#60a5fa`,
//! light `#93c5fd`, ground `#0f0e12`). The daemon stays headless; this shell
//! is one more intake adapter over the same loopback HTTP surface.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::Result;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, RunEvent, WebviewWindowBuilder, WindowEvent};

const MAIN_WINDOW: &str = "main";
const DAEMON_ORIGIN: &str = "http://127.0.0.1:5641";
const SERVICE_NAME: &str = "koi";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `--minimized`: login/autostart launches stay in the tray; the workbench
/// window is only built when revealed. Without a usable tray the window is
/// the honest fallback surface, so the flag yields to it.
static START_MINIMIZED: AtomicBool = AtomicBool::new(false);

struct Workbench {
    tray_available: Arc<AtomicBool>,
}

/// The PID of a daemon this workbench started itself ("Run once"). Stop kills
/// exactly this PID — never any other koi process (run-scoped doctrine).
#[derive(Default)]
struct OnDemandDaemon(Mutex<Option<u32>>);

fn main() {
    if let Err(error) = run() {
        eprintln!("Koi workbench failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    if std::env::args().any(|a| a == "--poke") {
        run_poke_and_exit();
    }
    if std::env::args().any(|arg| arg == "--minimized") {
        START_MINIMIZED.store(true, Ordering::SeqCst);
    }
    let workbench = Workbench {
        tray_available: Arc::new(AtomicBool::new(false)),
    };
    let setup_tray_available = Arc::clone(&workbench.tray_available);

    let app = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tauri::Builder::default()
            .manage(OnDemandDaemon::default())
            .plugin(
                tauri_plugin_autostart::Builder::new()
                    .args(["--minimized"])
                    .build(),
            )
            .plugin(tauri_plugin_notification::init())
            .invoke_handler(tauri::generate_handler![
                service_status,
                service_start,
                service_stop,
                daemon_run_once,
                daemon_get,
                pond_publish_ui,
                pond_qr_svg,
                pond_qr_target,
                daemon_status,
                discover_start,
                discover_snapshot,
                discover_ping,
                dns_entries,
                dns_add,
                dns_remove,
                dns_txt_set,
                dns_txt_clear,
                certmesh_status,
                certmesh_diagnose,
                certmesh_log,
                certmesh_invite,
                certmesh_revoke,
                certmesh_create,
                certmesh_unlock,
                certmesh_destroy,
                certmesh_open_enrollment,
                certmesh_close_enrollment,
                certmesh_renew_self,
                certmesh_join,
                open_url,
                notify,
                daemon_status_full,
                status_events_start,
                debug_log
            ])
            .setup(move |app| {
                if let Err(error) = start_ui_poke_listener(app.handle().clone()) {
                    eprintln!("Koi poke listener unavailable: {error}");
                }
                match build_tray(app) {
                    Ok(info_item) => {
                        start_posture_polling(app.handle().clone(), info_item);
                        setup_tray_available.store(true, Ordering::SeqCst);
                    }
                    Err(error) => {
                        eprintln!(
                            "Koi could not create a tray icon in this desktop session: {error}; \
                             the workbench window remains available"
                        );
                    }
                }
                Ok(())
            })
            .on_window_event(|_window, event| {
                if let WindowEvent::Destroyed = event {
                    eprintln!("Koi workbench window ended; the tray can recreate it");
                }
            })
            .build(tauri::generate_context!())
    })) {
        Ok(Ok(app)) => app,
        Ok(Err(error)) => return Err(error.into()),
        Err(_) => anyhow::bail!("Koi's desktop shell failed during startup"),
    };

    let tray_available = Arc::clone(&workbench.tray_available);
    app.run(move |_app, event| {
        if let RunEvent::Ready = event {
            if START_MINIMIZED.load(Ordering::SeqCst) && tray_available.load(Ordering::SeqCst) {
                eprintln!("Koi is starting minimized to its tray");
                return;
            }
            if START_MINIMIZED.load(Ordering::SeqCst) {
                eprintln!(
                    "Koi was asked to start minimized but no tray is available; \
                     showing the workbench instead"
                );
            }
            match build_workbench(_app) {
                Ok(window) => {
                    if let Err(error) = window.set_focus() {
                        eprintln!("Koi could not focus its workbench: {error}");
                    }
                }
                Err(error) => {
                    eprintln!("Koi workbench is unavailable: {error}");
                    if !tray_available.load(Ordering::SeqCst) {
                        eprintln!("Koi has no desktop interaction route and will stop");
                        _app.exit(1);
                    }
                }
            }
        }
    });

    Ok(())
}

// ── daemon transport: Rust owns every network byte (the Ghostlight rule).
// The webview never fetches cross-origin; WebView2's network stack eats
// loopback requests unpredictably, so the shell proxies the loopback API.

/// GET a daemon URL and parse the JSON body, or None on any failure — the
/// flattened form of the call/into_string/from_str chain, sized for clippy
/// and honest about "any failure is just: no data".
fn get_json(agent: &ureq::Agent, url: String) -> Option<serde_json::Value> {
    let text = agent.get(&url).call().ok()?.into_string().ok()?;
    serde_json::from_str(&text).ok()
}

/// GET a validated URL and return the body, or the honest reason it refused:
/// non-2xx daemons answer with a JSON error (`capability_disabled`, …) that
/// names the cause — the pane shows the daemon's own words, never "no data".
fn get_json_or_reason(agent: &ureq::Agent, url: String) -> Result<serde_json::Value, String> {
    match agent.get(&url).call() {
        Ok(response) => {
            let body = response
                .into_string()
                .map_err(|e| format!("{url}: unreadable body: {e}"))?;
            serde_json::from_str(&body).map_err(|e| format!("{url}: malformed body: {e}"))
        }
        Err(ureq::Error::Status(code, response)) => {
            let reason = response
                .into_string()
                .unwrap_or_else(|_| "no body".to_string());
            Err(format!(
                "{url}: {code}: {}",
                reason.chars().take(300).collect::<String>()
            ))
        }
        Err(e) => Err(format!("{url}: {e}")),
    }
}

/// Read-only GET against any node's daemon (cycle-1 WP0): the cross-host
/// browser and future scope views read sibling daemons' declared state. GETs
/// are the LAN-readable surface by design; mutations never ride this command.
fn validate_daemon_get(address: &str, port: u16, path: &str) -> Result<String, String> {
    if address.trim().is_empty() || address.contains(['/', '\\', ' ', ':']) {
        return Err("daemon address looks wrong".into());
    }
    if !(1..=65535).contains(&port) {
        return Err("daemon port out of range".into());
    }
    if !path.starts_with('/') || path.contains([' ', '\t', '\r', '\n']) || path.contains("..") {
        return Err("daemon path looks wrong".into());
    }
    Ok(format!("http://{address}:{port}{path}"))
}

#[tauri::command]
fn daemon_get(address: String, port: u16, path: String) -> Result<serde_json::Value, String> {
    let url = validate_daemon_get(&address, port, &path)?;
    get_json_or_reason(&daemon_agent(), url)
}

const UI_POKE_PORT: u16 = 5640;

/// Route a poke-request line. Loopback only; the only meaningful paths are
/// /poke (refresh now) and /health (is a UI listening).
fn poke_route(request_line: &str) -> &'static str {
    if request_line.starts_with("GET /poke ")
        || request_line.starts_with("GET /poke?")
        || request_line == "GET /poke"
    {
        "poke"
    } else if request_line.starts_with("GET /health") {
        "health"
    } else {
        "other"
    }
}

/// localhost-only poke listener: any local process (a script, the installer,
/// a second instance with --poke) can nudge every running workbench to
/// re-read the daemon immediately. Never binds beyond 127.0.0.1.
fn start_ui_poke_listener(app: tauri::AppHandle) -> Result<(), String> {
    // A kill-then-relaunch cycle can leave the old socket lingering for a
    // moment; retry the bind instead of going poke-less for the whole run.
    let listener = {
        let mut bound = None;
        for attempt in 0..20 {
            match std::net::TcpListener::bind(("127.0.0.1", UI_POKE_PORT)) {
                Ok(l) => {
                    bound = Some(l);
                    break;
                }
                Err(e) => {
                    if attempt == 19 {
                        return Err(format!("poke port {}: {e}", UI_POKE_PORT));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }
        bound.expect("bind retried 20 times")
    };
    std::thread::spawn(move || {
        use std::io::Write as _;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(250)));
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let request_line = String::from_utf8_lossy(&buf);
            let route = poke_route(
                request_line
                    .split(
                        "
",
                    )
                    .next()
                    .unwrap_or(""),
            );
            let (code, body) = match route {
                "poke" => {
                    let _ = app.emit("ui-poked", serde_json::json!({}));
                    ("200 OK", "poked")
                }
                "health" => ("200 OK", "koi ui here"),
                _ => ("404 NOT FOUND", "try /poke"),
            };
            let response = format!(
                "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    Ok(())
}

/// Second-instance nudge: `koi-desktop --poke` pokes every running UI on
/// this machine and exits without starting a new workbench.
fn run_poke_and_exit() -> ! {
    let result =
        std::net::TcpStream::connect(("127.0.0.1", UI_POKE_PORT)).and_then(|mut stream| {
            use std::io::Write;
            stream.write_all(
                b"GET /poke HTTP/1.1
Host: 127.0.0.1
Connection: close

",
            )?;
            stream.flush()?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stream, &mut buf);
            Ok(String::from_utf8_lossy(&buf).into_owned())
        });
    match result {
        Ok(text) => println!(
            "poked a running koi ui: {}",
            if text.contains("200 OK") {
                "acknowledged".to_string()
            } else {
                text
            }
        ),
        Err(e) => println!("no koi ui running on 127.0.0.1:{} ({e})", UI_POKE_PORT),
    }
    std::process::exit(0);
}
/// Publish the workbench's own interface to the daemon (cycle-1 WP-qr): the
/// five UI files ride a DAT-authenticated PUT; the daemon then serves them at
/// its LAN address so any browser on the network opens the same pond.
#[tauri::command]
fn pond_publish_ui() -> Result<serde_json::Value, String> {
    let files = [
        ("index.html", include_str!("../ui/index.html").to_string()),
        ("app.js", include_str!("../ui/app.js").to_string()),
        ("styles.css", include_str!("../ui/styles.css").to_string()),
        (
            "sentences.js",
            include_str!("../ui/sentences.js").to_string(),
        ),
    ];
    let mut entries = Vec::new();
    for (path, content) in files {
        entries.push(serde_json::json!({ "path": path, "content": content }));
    }
    let png = include_bytes!("../ui/koi.png");
    use base64::Engine as _;
    entries.push(serde_json::json!({
        "path": "koi.png",
        "content": base64::engine::general_purpose::STANDARD.encode(png),
    }));
    daemon_json(
        "PUT",
        "/v1/ui",
        Some(serde_json::json!({ "files": entries })),
    )
}

/// The LAN URL a phone should open: the daemon's address as seen from the
/// network (routing-table lookup; no packet leaves the machine).
#[tauri::command]
fn pond_qr_target() -> Result<String, String> {
    let sock =
        std::net::UdpSocket::bind(("127.0.0.1", 0)).map_err(|e| format!("no local socket: {e}"))?;
    sock.connect("192.168.1.1:80")
        .or_else(|_| sock.connect("8.8.8.8:80"))
        .map_err(|e| format!("no route: {e}"))?;
    let ip = sock
        .local_addr()
        .map_err(|e| format!("no local addr: {e}"))?;
    Ok(format!("http://{}:5641/", ip.ip()))
}

/// Render a QR code for `url` as a compact dark-theme SVG string.
#[tauri::command]
fn pond_qr_svg(url: String) -> Result<String, String> {
    use qrcode::render::svg;
    use qrcode::QrCode;
    let code = QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::M)
        .map_err(|e| format!("qr encode failed: {e}"))?;
    let image = code
        .render()
        .dark_color(svg::Color("#e8e8ec"))
        .light_color(svg::Color("#0d0d11"))
        .quiet_zone(true)
        .build();
    Ok(image)
}

fn daemon_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .build()
}

#[tauri::command]
fn daemon_status() -> Result<serde_json::Value, String> {
    let agent = daemon_agent();
    let mut out = serde_json::json!({ "up": false, "version": null, "posture": null });
    if let Some(status) = get_json(&agent, format!("{DAEMON_ORIGIN}/v1/status")) {
        out["up"] = serde_json::Value::Bool(true);
        out["version"] = status["version"].clone();
        if let Some(posture) = get_json(&agent, format!("{DAEMON_ORIGIN}/v1/certmesh/posture")) {
            out["posture"] = posture["level"].clone();
        }
    }
    Ok(out)
}

/// The whole /v1/status document — the honest glass pane shows the capability
/// ladder with the daemon's own words (skip reasons are data, not log lines).
#[tauri::command]
fn daemon_status_full() -> Result<serde_json::Value, String> {
    get_json(&daemon_agent(), format!("{DAEMON_ORIGIN}/v1/status"))
        .ok_or_else(|| "no daemon — the ladder is unknown, not healthy".to_string())
}

#[tauri::command]
fn discover_snapshot() -> Result<serde_json::Value, String> {
    let response = daemon_agent()
        .get(format!("{DAEMON_ORIGIN}/v1/mdns/browser/snapshot").as_str())
        .call()
        .map_err(|e| format!("snapshot unavailable: {e}"))?;
    let body = response
        .into_string()
        .map_err(|e| format!("snapshot unreadable: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("snapshot malformed: {e}"))
}

/// "Ping the pond": force the daemon's mDNS query burst so every client on the
/// LAN answers immediately. The endpoint is DAT-gated (a POST); the breadcrumb
/// carries the token, exactly like the CLI.
#[tauri::command]
fn discover_ping() -> Result<serde_json::Value, String> {
    let (_, token) = read_breadcrumb().ok_or("no daemon breadcrumb found — is Koi running?")?;
    let mut request =
        daemon_agent().post(format!("{DAEMON_ORIGIN}/v1/mdns/browser/query").as_str());
    if let Some(token) = &token {
        request = request.set("x-koi-token", token);
    }
    let response = request
        .send_json(serde_json::json!({}))
        .map_err(|e| format!("query burst failed: {e}"))?;
    let body = response
        .into_string()
        .map_err(|e| format!("query response unreadable: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("query response malformed: {e}"))
}

/// Authenticated daemon request: breadcrumb token attached, JSON body in/out.
/// Mutations are DAT-gated server-side; reads stay loopback-exempt.
fn daemon_json(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let (endpoint, token) =
        read_breadcrumb().ok_or("no daemon breadcrumb found — is Koi running?")?;
    let mut request = match method {
        "POST" => daemon_agent().post(format!("{endpoint}{path}").as_str()),
        "PUT" => daemon_agent().put(format!("{endpoint}{path}").as_str()),
        "DELETE" => daemon_agent().delete(format!("{endpoint}{path}").as_str()),
        _ => daemon_agent().get(format!("{endpoint}{path}").as_str()),
    };
    if let Some(token) = &token {
        request = request.set("x-koi-token", token);
    }
    let response = match body {
        Some(body) => request
            .send_json(body)
            .map_err(|e| format!("{method} {path} failed: {e}"))?,
        None => request.call().map_err(|e| format!("{path} failed: {e}"))?,
    };
    let text = response
        .into_string()
        .map_err(|e| format!("{path} unreadable: {e}"))?;
    if text.trim().is_empty() {
        return Ok(serde_json::json!({ "ok": true }));
    }
    serde_json::from_str(&text).map_err(|e| format!("{path} malformed: {e}"))
}

// ── CA + membership management (cycle-1, operator direction) ────────
// The Trust pane is role-adaptive: an open node can CREATE a CA or JOIN a
// pond; a locked CA can UNLOCK; an active CA manages enrollment, renews, and
// can DESTROY itself; a member renews its identity. Every request rides the
// local breadcrumb DAT exactly like the CLI; the remote join call carries no
// local token (the CA never sees ours). Passphrases cross loopback only and
// are never logged.

/// Profile presets (koi-certmesh/src/profiles.rs): (enrollment_open,
/// requires_approval, auto_unlock). The UI names them; the daemon owns them.
fn preset_bools(profile: &str) -> Option<(bool, bool, bool)> {
    match profile {
        "just-me" => Some((true, false, true)),
        "team" => Some((true, true, true)),
        "organization" => Some((false, true, false)),
        _ => None,
    }
}

/// 32 bytes of mixing entropy for CA creation (the daemon requires exactly 32
/// bytes of hex). The CA key generator has its own OS RNG; this is additional
/// mixing material. Without a new dependency, mix several OS-seeded
/// `RandomState` keys (std seeds each from OS entropy) with time and address
/// dispersion — unpredictable, honest about not being a documented CSPRNG.
fn mixing_entropy() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut words = [0u64; 4];
    for (i, word) in words.iter_mut().enumerate() {
        let mut h = RandomState::new().build_hasher();
        i.hash(&mut h);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        nanos.hash(&mut h);
        let stack_marker = 0u8;
        (&stack_marker as *const u8).hash(&mut h);
        *word = h.finish();
    }
    let mut hex = String::with_capacity(64);
    for w in words {
        hex.push_str(&format!("{w:016x}"));
    }
    hex
}

/// Normalize a CA endpoint: accept `host:port` or a full URL, default http.
fn normalize_ca_endpoint(endpoint: &str) -> Result<String, String> {
    let e = endpoint.trim().trim_end_matches('/');
    if e.is_empty() {
        return Err("the CA endpoint is required".into());
    }
    if let Some(rest) = e.strip_prefix("http://") {
        if rest.is_empty() || rest.contains('/') || rest.contains(' ') {
            return Err("the CA endpoint looks wrong".into());
        }
        return Ok(format!("http://{rest}"));
    }
    if let Some(rest) = e.strip_prefix("https://") {
        if rest.is_empty() || rest.contains('/') || rest.contains(' ') {
            return Err("the CA endpoint looks wrong".into());
        }
        return Ok(format!("https://{rest}"));
    }
    if e.contains('/') || e.contains(' ') || e.contains('\\') {
        return Err("the CA endpoint looks wrong".into());
    }
    Ok(format!("http://{e}"))
}

/// Split an invite code `<secret>.<ca_fingerprint>` (ADR-017 F3): the CA only
/// ever receives the secret half; the fingerprint half is the joiner's pin.
fn split_invite(invite: &str) -> (String, Option<String>) {
    match invite.split_once('.') {
        Some((secret, fp)) if !fp.is_empty() => (secret.to_string(), Some(fp.to_string())),
        _ => (invite.trim().to_string(), None),
    }
}

fn hex_fingerprint(fp: &str) -> String {
    fp.trim().to_lowercase().replace(':', "")
}

/// POST to a REMOTE daemon without the local token: the CA never sees ours.
fn remote_post(
    agent: &ureq::Agent,
    endpoint: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let response = agent
        .post(format!("{endpoint}{path}").as_str())
        .send_json(body)
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let reason = resp.into_string().unwrap_or_default();
                format!("{code}: {}", reason.chars().take(300).collect::<String>())
            }
            other => format!("{other}"),
        })?;
    let text = response
        .into_string()
        .map_err(|e| format!("{path} unreadable: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path} malformed: {e}"))
}

fn remote_get(
    agent: &ureq::Agent,
    endpoint: &str,
    path: &str,
) -> Result<serde_json::Value, String> {
    let response = agent
        .get(format!("{endpoint}{path}").as_str())
        .call()
        .map_err(|e| format!("{path}: {e}"))?;
    let text = response
        .into_string()
        .map_err(|e| format!("{path} unreadable: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path} malformed: {e}"))
}

#[tauri::command]
fn certmesh_create(
    profile: String,
    passphrase: String,
    confirm: String,
    operator: Option<String>,
) -> Result<serde_json::Value, String> {
    if passphrase.len() < 8 {
        return Err("the CA passphrase needs at least 8 characters — it protects every identity this CA signs.".into());
    }
    if passphrase != confirm {
        return Err("the passphrases do not match.".into());
    }
    let (enrollment_open, requires_approval, auto_unlock) =
        preset_bools(&profile).ok_or_else(|| "unknown CA profile".to_string())?;
    let op = operator
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty());
    if requires_approval && (op.is_none() || op.as_deref() == Some("")) {
        return Err("approval profiles name the operator who approves members.".into());
    }
    daemon_json(
        "POST",
        "/v1/certmesh/create",
        Some(serde_json::json!({
            "passphrase": passphrase,
            "entropy_hex": mixing_entropy(),
            "operator": op,
            "enrollment_open": enrollment_open,
            "requires_approval": requires_approval,
            "auto_unlock": auto_unlock,
        })),
    )
}

#[tauri::command]
fn certmesh_unlock(passphrase: String) -> Result<serde_json::Value, String> {
    if passphrase.is_empty() {
        return Err("the CA passphrase is required.".into());
    }
    daemon_json(
        "POST",
        "/v1/certmesh/unlock",
        Some(serde_json::json!({ "passphrase": passphrase })),
    )
}

#[tauri::command]
fn certmesh_destroy() -> Result<serde_json::Value, String> {
    daemon_json("POST", "/v1/certmesh/destroy", Some(serde_json::json!({})))
}

#[tauri::command]
fn certmesh_open_enrollment() -> Result<serde_json::Value, String> {
    daemon_json(
        "POST",
        "/v1/certmesh/open-enrollment",
        Some(serde_json::json!({})),
    )
}

#[tauri::command]
fn certmesh_close_enrollment() -> Result<serde_json::Value, String> {
    daemon_json(
        "POST",
        "/v1/certmesh/close-enrollment",
        Some(serde_json::json!({})),
    )
}

#[tauri::command]
fn certmesh_renew_self() -> Result<serde_json::Value, String> {
    daemon_json(
        "POST",
        "/v1/certmesh/renew-self",
        Some(serde_json::json!({})),
    )
}

/// Join a pond (the membership ceremony, orchestrated): preflight the CA and
/// pin its fingerprint to the invite's, generate the keypair + CSR LOCALLY
/// (key custody never leaves this machine), send the CSR to the REMOTE CA
/// with the invite secret (or a mesh TOTP), and install the signed cert
/// locally with the pinned fingerprint. Mirrors `koi certmesh join`.
#[tauri::command]
fn certmesh_join(
    endpoint: String,
    invite: Option<String>,
    totp: Option<String>,
) -> Result<serde_json::Value, String> {
    let ca = normalize_ca_endpoint(&endpoint)?;
    let invite = invite
        .map(|i| i.trim().to_string())
        .filter(|i| !i.is_empty());
    let totp = totp.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if invite.is_none() && totp.is_none() {
        return Err("bring either an invite code or a mesh TOTP code.".into());
    }
    if invite.is_some() && totp.is_some() {
        return Err("use an invite OR a TOTP code — not both.".into());
    }
    let (invite_secret, pinned_fp) = match &invite {
        Some(code) => {
            let (secret, fp) = split_invite(code);
            (Some(secret), fp)
        }
        None => (None, None),
    };

    let agent = daemon_agent();

    // 1. Preflight + pin: refuse a CA whose self-reported fingerprint does not
    //    match the invite's pin BEFORE any CSR leaves this machine.
    let ca_status = remote_get(&agent, &ca, "/v1/certmesh/status")?;
    let advertised = ca_status
        .get("ca_fingerprint")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let pinned = match (&pinned_fp, &advertised) {
        (Some(pin), Some(ad)) => {
            if hex_fingerprint(pin) != hex_fingerprint(ad) {
                return Err(format!(
                    "CA fingerprint mismatch — refusing to join. invite pinned {pin}, CA advertised {ad}. \
                     The endpoint may be impersonating the CA, or the invite is for a different mesh."
                ));
            }
            Some(hex_fingerprint(pin))
        }
        (Some(pin), None) => {
            return Err(format!(
                "the CA at {ca} did not report a fingerprint — aborting (the invite expects {pin})"
            ));
        }
        // TOTP join has no out-of-band pin: TOFU on the CA's self-reported fp.
        (None, ad) => ad.as_deref().map(hex_fingerprint),
    };

    // 2. Local key custody: the daemon generates the keypair + CSR; the private
    //    key is written on this machine and never leaves it.
    let local_hostname = hostname();
    let csr_resp = daemon_json(
        "POST",
        "/v1/certmesh/member-csr",
        Some(serde_json::json!({ "hostname": local_hostname, "sans": [] })),
    )?;
    let csr = csr_resp
        .get("csr")
        .and_then(|v| v.as_str())
        .ok_or("the local daemon did not return a CSR")?
        .to_string();

    // 3. The remote CA signs it. Invite secret OR mesh TOTP — never both.
    let mut join_body = serde_json::Map::new();
    join_body.insert("hostname".into(), serde_json::json!(local_hostname));
    join_body.insert("csr".into(), serde_json::json!(csr));
    if let Some(secret) = &invite_secret {
        join_body.insert("invite_token".into(), serde_json::json!(secret));
    } else {
        join_body.insert(
            "auth".into(),
            serde_json::json!({ "method": "totp", "code": totp.clone().unwrap_or_default() }),
        );
    }
    let joined = remote_post(
        &agent,
        &ca,
        "/v1/certmesh/join",
        serde_json::Value::Object(join_body),
    )?;
    let service_cert = joined
        .get("service_cert")
        .and_then(|v| v.as_str())
        .ok_or("the CA response is missing the signed certificate")?;
    let ca_cert = joined
        .get("ca_cert")
        .and_then(|v| v.as_str())
        .ok_or("the CA response is missing the CA certificate")?;

    // 4. Install locally, pinned to the out-of-band invite fingerprint when we
    //    have one (never to the response fingerprint on an invite join).
    let mut install = serde_json::Map::new();
    install.insert("hostname".into(), serde_json::json!(local_hostname));
    install.insert("cert_pem".into(), serde_json::json!(service_cert));
    install.insert("ca_pem".into(), serde_json::json!(ca_cert));
    install.insert("ca_endpoint".into(), serde_json::json!(ca));
    if let Some(fp) = &pinned {
        install.insert("ca_fingerprint".into(), serde_json::json!(fp));
    }
    install.insert("sans".into(), serde_json::json!([]));
    if let Some(policy) = joined.get("policy") {
        install.insert("policy".into(), policy.clone());
    }
    let installed = daemon_json(
        "POST",
        "/v1/certmesh/member-cert",
        Some(serde_json::Value::Object(install)),
    )?;

    Ok(serde_json::json!({
        "enrolled": true,
        "hostname": local_hostname,
        "ca_endpoint": ca,
        "ca_fingerprint": installed.get("ca_fingerprint").or(joined.get("ca_fingerprint")),
    }))
}

/// Care (cycle-1 WP8): one OS notification when a watched inhabitant fades.
/// Opt-in by starring; never more than one per fade episode. Degrades
/// honestly — if the platform refuses, the command says so.
#[tauri::command]
fn notify(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| format!("notification refused: {e}"))
}

/// Passage (cycle-1 WP7): open a pond endpoint in the default browser.
/// Only http(s) passes — an mDNS announcement never gets to execute anything.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) URLs can be opened".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| format!("could not open {url}: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("could not open {url}: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("could not open {url}: {e}"))?;
    }
    Ok(())
}

// ── Trust pane (cycle-1 WP6): the certmesh doors ────────────────────
// Reads: status (GET-exempt by design — the joiner protocol depends on it),
// diagnose (loopback-exempt), log (DAT-gated GET — the one audit read).
// Mutations: invite/revoke are DAT-gated POSTs. All ride the breadcrumb
// token exactly like the CLI; the server decides what each method may do.

#[tauri::command]
fn certmesh_status() -> Result<serde_json::Value, String> {
    daemon_json("GET", "/v1/certmesh/status", None)
}

#[tauri::command]
fn certmesh_diagnose() -> Result<serde_json::Value, String> {
    daemon_json("GET", "/v1/certmesh/diagnose", None)
}

#[tauri::command]
fn certmesh_log() -> Result<serde_json::Value, String> {
    daemon_json("GET", "/v1/certmesh/log", None)
}

#[tauri::command]
fn certmesh_invite(hostname: String, ttl_mins: Option<i64>) -> Result<serde_json::Value, String> {
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Err("A grant needs the member's hostname.".into());
    }
    let mut body = serde_json::json!({ "hostname": hostname });
    if let Some(ttl) = ttl_mins {
        if ttl <= 0 {
            return Err("Invite TTL must be a positive number of minutes.".into());
        }
        body["ttl_mins"] = serde_json::json!(ttl);
    }
    daemon_json("POST", "/v1/certmesh/invite", Some(body))
}

#[tauri::command]
fn certmesh_revoke(hostname: String, reason: Option<String>) -> Result<serde_json::Value, String> {
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Err("A revocation needs the member's hostname.".into());
    }
    daemon_json(
        "POST",
        "/v1/certmesh/revoke",
        Some(serde_json::json!({ "hostname": hostname, "reason": reason })),
    )
}

#[tauri::command]
fn dns_entries() -> Result<serde_json::Value, String> {
    daemon_json("GET", "/v1/dns/entries", None)
}

#[tauri::command]
fn dns_add(name: String, ip: String, ttl: Option<u32>) -> Result<serde_json::Value, String> {
    if name.trim().is_empty() || ip.trim().is_empty() {
        return Err("A record needs both a name and an IP.".into());
    }
    daemon_json(
        "POST",
        "/v1/dns/add",
        Some(serde_json::json!({ "name": name.trim(), "ip": ip.trim(), "ttl": ttl })),
    )
}

#[tauri::command]
fn dns_remove(name: String) -> Result<serde_json::Value, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A record name is required.".into());
    }
    daemon_json("DELETE", format!("/v1/dns/remove/{name}").as_str(), None)
}

#[tauri::command]
fn dns_txt_set(name: String, value: String) -> Result<serde_json::Value, String> {
    if name.trim().is_empty() || value.is_empty() {
        return Err("A TXT record needs both a name and a value.".into());
    }
    daemon_json(
        "PUT",
        "/v1/dns/txt",
        Some(serde_json::json!({ "name": name.trim(), "value": value })),
    )
}

#[tauri::command]
fn dns_txt_clear(name: String, value: String) -> Result<serde_json::Value, String> {
    daemon_json(
        "DELETE",
        "/v1/dns/txt",
        Some(serde_json::json!({ "name": name.trim(), "value": value })),
    )
}

static DISCOVER_STARTED: AtomicBool = AtomicBool::new(false);

/// Push the live discover-stream state to the UI so "live" is never stale.
fn emit_stream_state(app: &tauri::AppHandle, state: &str) {
    let _ = app.emit("discover-stream", serde_json::json!(state));
}

/// Breadcrumb: the daemon's two-line discovery file (endpoint + DAT). The
/// workbench reads it exactly like the CLI does.
fn read_breadcrumb() -> Option<(String, Option<String>)> {
    #[cfg(windows)]
    let path = {
        let program_data =
            std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
        std::path::PathBuf::from(program_data)
            .join("koi")
            .join("koi.endpoint")
    };
    #[cfg(not(windows))]
    let path = std::path::PathBuf::from("/var/run/koi.endpoint");
    let body = std::fs::read_to_string(path).ok()?;
    let mut lines = body.lines();
    let endpoint = lines.next()?.trim().to_owned();
    if endpoint.is_empty() {
        return None;
    }
    let token = lines
        .next()
        .and_then(|l| l.trim().strip_prefix("dat:").map(str::to_owned));
    Some((endpoint, token))
}

fn fetch_status_value(agent: &ureq::Agent, endpoint: &str) -> serde_json::Value {
    let mut out = serde_json::json!({ "up": true, "version": null, "posture": null });
    if let Some(status) = get_json(agent, format!("{endpoint}/v1/status")) {
        out["version"] = status["version"].clone();
        if let Some(posture) = get_json(agent, format!("{endpoint}/v1/certmesh/posture")) {
            out["posture"] = posture["level"].clone();
        }
    }
    out
}

fn emit_down_status(app: &tauri::AppHandle) {
    // "Down" means the daemon is truly absent — healthz fails too. A daemon
    // that merely lacks /v1/events (older builds) is still up.
    let healthz_ok = daemon_agent()
        .get(format!("{DAEMON_ORIGIN}/healthz").as_str())
        .call()
        .is_ok();
    if !healthz_ok {
        let _ = app.emit(
            "daemon-status",
            serde_json::json!({ "up": false, "version": null, "posture": null }),
        );
    }
}

static STATUS_STREAM_STARTED: AtomicBool = AtomicBool::new(false);

/// The lamp rides the daemon's unified SSE stream (`/v1/events`, DAT-gated):
/// connect → push a fresh status; heartbeat → refresh; any event → forward.
/// No polling anywhere in the loop; reconnects with backoff when it drops.
#[tauri::command]
fn status_events_start(app: tauri::AppHandle) -> Result<(), String> {
    if STATUS_STREAM_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    std::thread::spawn(move || {
        let agent = daemon_agent();
        loop {
            let Some((endpoint, token)) = read_breadcrumb() else {
                emit_down_status(&app);
                std::thread::sleep(Duration::from_secs(3));
                continue;
            };
            let mut request = agent.get(format!("{endpoint}/v1/events").as_str());
            if let Some(token) = &token {
                request = request.set("x-koi-token", token);
            }
            match request.call() {
                Ok(response) => {
                    let _ = app.emit("daemon-status", fetch_status_value(&agent, &endpoint));
                    let mut reader = std::io::BufReader::new(response.into_reader());
                    let mut kind = String::new();
                    let mut data = String::new();
                    loop {
                        let mut line = String::new();
                        match std::io::BufRead::read_line(&mut reader, &mut line) {
                            Ok(0) => break,
                            Ok(_) => {}
                            Err(_) => break,
                        }
                        let line = line.trim_end();
                        if let Some(k) = line.strip_prefix("event:") {
                            kind = k.trim().to_owned();
                        } else if let Some(d) = line.strip_prefix("data:") {
                            data.push_str(d.trim());
                        } else if line.is_empty() {
                            if kind == "heartbeat" {
                                let _ = app
                                    .emit("daemon-status", fetch_status_value(&agent, &endpoint));
                            } else if !kind.is_empty() {
                                let parsed: Option<serde_json::Value> = if data.is_empty() {
                                    None
                                } else {
                                    serde_json::from_str(&data).ok()
                                };
                                let _ = app.emit(
                                    "daemon-event",
                                    serde_json::json!({ "kind": kind, "data": parsed }),
                                );
                            }
                            kind.clear();
                            data.clear();
                        }
                    }
                }
                Err(_) => emit_down_status(&app),
            }
            // Stream dropped or daemon absent; try again shortly.
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    Ok(())
}

/// One Rust-side SSE reader keeps the daemon's browse session alive and
/// forwards every event to the webview. Reconnects forever with backoff.
#[tauri::command]
fn discover_start(app: tauri::AppHandle) -> Result<(), String> {
    if DISCOVER_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    std::thread::spawn(move || {
        let agent = daemon_agent();
        loop {
            match agent
                .get(format!("{DAEMON_ORIGIN}/v1/mdns/browser/events").as_str())
                .call()
            {
                Ok(response) => {
                    emit_stream_state(&app, "live");
                    let mut reader = std::io::BufReader::new(response.into_reader());
                    let mut kind = String::new();
                    let mut data = String::new();
                    loop {
                        let mut line = String::new();
                        match std::io::BufRead::read_line(&mut reader, &mut line) {
                            Ok(0) => break,
                            Ok(_) => {}
                            Err(_) => break,
                        }
                        let line = line.trim_end();
                        if let Some(k) = line.strip_prefix("event:") {
                            kind = k.trim().to_owned();
                        } else if let Some(d) = line.strip_prefix("data:") {
                            data.push_str(d.trim());
                        } else if line.is_empty() {
                            if !kind.is_empty() {
                                let parsed: Option<serde_json::Value> = if data.is_empty() {
                                    None
                                } else {
                                    serde_json::from_str(&data).ok()
                                };
                                let _ = app.emit(
                                    "mdns-event",
                                    serde_json::json!({ "kind": kind, "data": parsed }),
                                );
                            }
                            kind.clear();
                            data.clear();
                        }
                    }
                }
                Err(_) => {
                    emit_stream_state(&app, "offline");
                }
            }
            // The daemon is down or the stream dropped; try again shortly.
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    Ok(())
}

/// Debug sink: the workbench's own console, on disk, so a headless session can
/// diagnose the webview. Milestones + errors only. Repo-local `.tmp/` when the
/// shell's working directory is the checkout (the dev-loop case); falls back to
/// the OS temp dir otherwise. Safe to remove later.
#[tauri::command]
fn debug_log(message: String) {
    let dir = std::env::current_dir()
        .map(|cwd| cwd.join(".tmp"))
        .ok()
        .filter(|p| p.is_dir())
        .or_else(|| {
            std::env::var("LOCALAPPDATA")
                .ok()
                .map(|d| std::path::PathBuf::from(d).join("Koi"))
        })
        .unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&dir);
    let line = format!(
        "[{}] {}\n",
        chrono_like_timestamp(),
        message.replace('\n', " | ")
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("workbench-debug.log"))
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

fn chrono_like_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}s")
}

#[derive(Serialize)]
struct ServiceStatus {
    installed: bool,
    running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn sc(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("sc")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

#[tauri::command]
fn service_status() -> ServiceStatus {
    #[cfg(windows)]
    {
        match sc(&["query", SERVICE_NAME]) {
            Ok(output) => {
                let text = String::from_utf8_lossy(&output.stdout);
                if text.contains("1060") {
                    ServiceStatus {
                        installed: false,
                        running: false,
                        detail: None,
                    }
                } else {
                    ServiceStatus {
                        installed: true,
                        running: text.contains("RUNNING"),
                        detail: None,
                    }
                }
            }
            Err(error) => ServiceStatus {
                installed: false,
                running: false,
                detail: Some(format!("sc query failed: {error}")),
            },
        }
    }
    #[cfg(not(windows))]
    {
        ServiceStatus {
            installed: false,
            running: false,
            detail: Some("unsupported platform".into()),
        }
    }
}

#[tauri::command]
fn service_start() -> Result<StartResult, String> {
    #[cfg(windows)]
    {
        sc(&["start", SERVICE_NAME])
            .map_err(|e| format!("could not run sc: {e}"))
            .and_then(|output| {
                if output.status.success() {
                    Ok(StartResult::ok(
                        "Service start issued; it should be serving in moments.",
                    ))
                } else {
                    let text = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                    Err(
                        if text.contains("5") || text.to_lowercase().contains("access") {
                            "Starting a system service needs an elevated session. \
                         Right-click Koi → Run as administrator, or run `sc start koi` \
                         from an elevated terminal."
                                .to_string()
                        } else if !text.is_empty() {
                            text
                        } else {
                            "Service refused to start.".to_string()
                        },
                    )
                }
            })
    }
    #[cfg(not(windows))]
    {
        Err("unsupported platform".into())
    }
}

#[tauri::command]
fn service_stop() -> Result<StartResult, String> {
    #[cfg(windows)]
    {
        sc(&["stop", SERVICE_NAME])
            .map_err(|e| format!("could not run sc: {e}"))
            .and_then(|output| {
                if output.status.success() {
                    Ok(StartResult::ok("Stop issued."))
                } else {
                    Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
                }
            })
    }
    #[cfg(not(windows))]
    {
        Err("unsupported platform".into())
    }
}

#[derive(Serialize)]
struct StartResult {
    message: String,
}

impl StartResult {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[tauri::command]
fn daemon_run_once(state: tauri::State<'_, OnDemandDaemon>) -> Result<StartResult, String> {
    let exe = locate_koi_exe().ok_or_else(|| {
        "koi.exe was not found on PATH or in its usual install locations. \
         Install Koi first (`winget`/archive), then try again."
            .to_string()
    })?;
    let child = Command::new(&exe)
        .arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | 0x0000_0008 /* DETACHED_PROCESS */)
        .spawn()
        .map_err(|e| format!("could not launch {exe}: {e}"))?;
    *state.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(child.id());
    Ok(StartResult::ok(format!(
        "Started koi (pid {}) as a plain process; Stop ends exactly this instance.",
        child.id()
    )))
}

/// Locate a usable koi binary without guessing broadly: PATH first, then the
/// documented install locations.
fn locate_koi_exe() -> Option<String> {
    #[cfg(windows)]
    {
        if let Ok(output) = Command::new("where.exe")
            .arg("koi.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            if output.status.success() {
                if let Some(first) = String::from_utf8_lossy(&output.stdout).lines().next() {
                    let path = first.trim();
                    if !path.is_empty() {
                        return Some(path.to_owned());
                    }
                }
            }
        }
        for candidate in [
            r"C:\ProgramData\koi\koi.exe",
            r"C:\Program Files\koi\koi.exe",
        ] {
            if std::path::Path::new(candidate).is_file() {
                return Some(candidate.to_owned());
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        Some("koi".into())
    }
}

// ── desktop shell (borrowed from Ghostlight) ──────────────────────────

/// The tray borrows Ghostlight's shape: id-dispatched menu, left click reveals
/// the workbench, Quit is the only exit, and an unusable tray degrades honestly.
fn build_tray(app: &mut tauri::App) -> Result<MenuItem<tauri::Wry>> {
    let status = MenuItem::with_id(app, "status", "Koi · starting", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Workbench", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Koi", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&status, &open, &quit])?;

    let mut builder = TrayIconBuilder::with_id("koi")
        .tooltip("Koi")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => reveal_from_tray(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                reveal_from_tray(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(status)
}

fn reveal_from_tray(app: &tauri::AppHandle) {
    if let Err(error) = show_workbench(app) {
        eprintln!("Koi could not open its workbench from the tray: {error}");
    }
}

fn show_workbench(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => window.show(),
        None => build_workbench(app).and_then(|window| window.show()),
    }
}

fn build_workbench(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, tauri::Error> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == MAIN_WINDOW)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let window = WebviewWindowBuilder::from_config(app, &config)?.build()?;
    window.show()?;
    Ok(window)
}

/// Polls the daemon's posture endpoint and keeps the tray's disabled status
/// line truthful ("{host} · {level}"), exactly like `/v1/certmesh/posture`
/// reports it over loopback. A missing daemon reads "offline" — never invented.
fn start_posture_polling(_app: tauri::AppHandle, status_item: MenuItem<tauri::Wry>) {
    std::thread::spawn(move || {
        let host = hostname();
        loop {
            let line = match posture_level() {
                Some(level) => format!("{host} · {level}"),
                None => format!("{host} · offline"),
            };
            if let Err(error) = status_item.set_text(line) {
                eprintln!("Koi could not update its tray status line: {error}");
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

fn posture_level() -> Option<String> {
    let body = ureq::get(format!("{DAEMON_ORIGIN}/v1/certmesh/posture").as_str())
        .timeout(Duration::from_secs(3))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let value: serde_json::Value = serde_json::from_str(&body).ok()?;
    value["level"].as_str().map(str::to_owned)
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "koi".into())
}

#[cfg(test)]
mod cycle1_guards {
    // The daemon's SSE event kinds (koi-dashboard/src/forward.rs). Adding a
    // kind there means adding its sentence here — the guard makes the drift
    // loud at build time, not at the operator's desk.
    const KNOWN_KINDS: &[&str] = &[
        "mdns.found",
        "mdns.resolved",
        "mdns.removed",
        "health.changed",
        "dns.updated",
        "dns.removed",
        "dns.txt_updated",
        "dns.txt_removed",
        "certmesh.joined",
        "certmesh.revoked",
        "certmesh.destroyed",
        "certmesh.cert_renewed",
        "certmesh.cert_expiring_soon",
        "certmesh.cert_renewal_failed",
        "certmesh.bundle_updated",
        "proxy.updated",
        "proxy.removed",
        "runtime.started",
        "runtime.stopped",
        "runtime.updated",
        "runtime.disconnected",
        "runtime.reconnected",
    ];

    const SENTENCES_JS: &str = include_str!("../ui/sentences.js");

    #[test]
    fn every_known_kind_has_a_sentence_entry_and_a_registry_view() {
        for kind in KNOWN_KINDS {
            assert!(
                SENTENCES_JS.contains(&format!("\"{kind}\":")),
                "sentences.js has no registry entry for {kind}"
            );
            assert!(
                SENTENCES_JS.contains(&format!("case \"{kind}\":")),
                "sentences.js has no sentence line for {kind}"
            );
        }
    }

    #[test]
    fn sentences_js_declares_the_module_contract() {
        assert!(SENTENCES_JS.contains("window.KoiSentences"));
        assert!(SENTENCES_JS.contains("function sentenceFor("));
        assert!(SENTENCES_JS.contains("function targetOf("));
    }

    #[test]
    fn cross_host_get_refuses_nonsense() {
        assert!(
            super::validate_daemon_get("192.168.1.44", 16541, "/v1/mdns/browser/snapshot",).is_ok()
        );
        assert!(super::validate_daemon_get("", 16541, "/x").is_err());
        assert!(super::validate_daemon_get("192.168.1.44/x", 16541, "/x").is_err());
        assert!(super::validate_daemon_get("192.168.1.44", 0, "/x").is_err());
        assert!(super::validate_daemon_get("192.168.1.44", 16541, "v1/x").is_err());
        assert!(super::validate_daemon_get("192.168.1.44", 16541, "/x y").is_err());
        let traversal = ["/x", "..", "y"].join("/");
        assert!(traversal.contains(".."));
        assert!(super::validate_daemon_get("192.168.1.44", 16541, &traversal).is_err());
    }

    /// Live acceptance (WP0): a sibling daemon's browse snapshot is reachable
    /// with the exact command the workbench uses. Ignored by default; run
    /// with `cargo test -- --ignored` while the LAN is up.
    #[test]
    #[ignore]
    fn cross_host_get_reaches_brook() {
        // Live acceptance (WP0): a sibling daemon answers the exact GET the
        // workbench uses - either with its browse snapshot (browser mounted)
        // or with a DECLARED capability skip (ADR-035: honest degradation).
        // Both prove the per-node read; silence is the only failure. ureq
        // wraps non-2xx inside Err(Status(code, response)), so both branches
        // carry a parseable body. Ignored by default; run with
        // `cargo test -- --ignored` while the LAN is up.
        let target = super::validate_daemon_get("192.168.1.44", 5641, "/v1/mdns/browser/snapshot")
            .expect("target should validate");
        let response = super::daemon_agent().get(&target).call();
        let (code, text) = match response {
            Ok(response) => {
                let code = response.status();
                let text = response
                    .into_string()
                    .expect("sibling response should be text");
                (code, text)
            }
            Err(ureq::Error::Status(code, response)) => {
                let text = response.into_string().unwrap_or_default();
                (code, text)
            }
            Err(e) => panic!("transport: {e}"),
        };
        let body: serde_json::Value =
            serde_json::from_str(text.trim()).unwrap_or(serde_json::json!({}));
        match code {
            200 => assert!(
                body.get("instances").is_some(),
                "snapshot should carry instances"
            ),
            503 => assert_eq!(
                body["error"], "capability_disabled",
                "a declared skip is the only acceptable non-200: {body}"
            ),
            other => panic!("unexpected status {other}: {text}"),
        }
    }
}
