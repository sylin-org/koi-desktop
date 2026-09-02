//! Koi's desktop workbench: a friendly native shell over the local substrate.
//!
//! Visual language and desktop patterns are borrowed from Ghostlight
//! (`sylin-org/browser-mcp`, `crates/orchestrator/src/desktop/mod.rs`) and
//! re-skinned with the Koi identity published on sylin.org (accent `#60a5fa`,
//! light `#93c5fd`, ground `#0f0e12`). The daemon stays headless; this shell
//! is one more intake adapter over the same loopback HTTP surface.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod local_daemon;
#[cfg(target_os = "linux")]
mod service_manager;

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
#[cfg(windows)]
const SERVICE_NAME: &str = "koi";
#[cfg(windows)]
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
    configure_linux_webview();
    if std::env::args().any(|a| a == "--poke") {
        run_poke_and_exit();
    }
    let ui_listener = match claim_ui_instance()? {
        UiInstanceClaim::Primary(listener) => listener,
        UiInstanceClaim::Existing { poked } => {
            println!(
                "Koi is already running{}",
                if poked { "; revealed it" } else { "" }
            );
            return Ok(());
        }
    };
    if std::env::args().any(|arg| arg == "--minimized") {
        START_MINIMIZED.store(true, Ordering::SeqCst);
    }
    let workbench = Workbench {
        tray_available: Arc::new(AtomicBool::new(false)),
    };
    let setup_tray_available = Arc::clone(&workbench.tray_available);
    let close_tray_available = Arc::clone(&workbench.tray_available);

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
                pond_disable,
                pond_qr_svg,
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
                probe_http,
                open_url,
                notify,
                autostart_state,
                autostart_set,
                daemon_status_full,
                status_events_start,
                debug_log
            ])
            .setup(move |app| {
                start_ui_poke_listener(ui_listener, app.handle().clone());
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
            .on_window_event(move |window, event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    if close_keeps_tray_alive(&close_tray_available) {
                        api.prevent_close();
                        if let Err(error) = window.hide() {
                            eprintln!("Koi could not hide its workbench in the tray: {error}");
                        }
                    }
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

fn close_keeps_tray_alive(tray_available: &AtomicBool) -> bool {
    tray_available.load(Ordering::SeqCst)
}

/// WebKitGTK's DMA-BUF renderer aborts at Wayland protocol setup on the
/// NVIDIA+i915 Plasma stack used by the CachyOS reference machine (upstream
/// wry#1366 / tauri#10702). Keep the native Wayland backend and disable only
/// that renderer; an operator-provided value always wins.
#[cfg(target_os = "linux")]
fn configure_linux_webview() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland");
    if wayland && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        eprintln!("Koi selected WebKitGTK's Wayland compatibility renderer");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_webview() {}

// ── daemon transport: Rust owns every network byte (the Ghostlight rule).
// The webview never fetches cross-origin; WebView2's network stack eats
// loopback requests unpredictably, so the shell proxies the loopback API.

fn get_local_json(
    agent: &ureq::Agent,
    access: &local_daemon::DaemonAccess,
    path: &str,
) -> Option<serde_json::Value> {
    let text = agent
        .get(&access.url(path))
        .set("x-koi-token", &access.token)
        .call()
        .ok()?
        .into_string()
        .ok()?;
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

/// ASYNC + spawn_blocking: a remote peer can take seconds to answer (or
/// never), and sync commands hold the main thread.
#[tauri::command]
async fn daemon_get(address: String, port: u16, path: String) -> Result<serde_json::Value, String> {
    let url = validate_daemon_get(&address, port, &path)?;
    tauri::async_runtime::spawn_blocking(move || get_json_or_reason(&daemon_agent(), url))
        .await
        .map_err(|e| format!("daemon_get task: {e}"))?
}

const UI_POKE_PORT: u16 = 5640;
const UI_REQUEST_LINE_LIMIT: u64 = 1024;

enum UiInstanceClaim {
    Primary(std::net::TcpListener),
    Existing { poked: bool },
}

/// Claim the same loopback listener used for UI pokes before constructing any
/// tray or window. Binding is atomic, so simultaneous autostart/session-restore
/// launches cannot both become resident workbenches.
fn claim_ui_instance() -> Result<UiInstanceClaim> {
    match std::net::TcpListener::bind(("127.0.0.1", UI_POKE_PORT)) {
        Ok(listener) => Ok(UiInstanceClaim::Primary(listener)),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            let poked = show_running_ui()
                .map(|response| poke_response_acknowledged(&response))
                .unwrap_or(false);
            Ok(UiInstanceClaim::Existing { poked })
        }
        Err(error) => anyhow::bail!("claim Koi UI port {UI_POKE_PORT}: {error}"),
    }
}

/// Route a UI request line. Loopback only; /show reveals and refreshes the
/// workbench, /poke refreshes it, and /health reports whether it is listening.
fn poke_route(request_line: &str) -> &'static str {
    if request_line.starts_with("GET /show ")
        || request_line.starts_with("GET /show?")
        || request_line == "GET /show"
    {
        "show"
    } else if request_line.starts_with("GET /poke ")
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

fn read_ui_request_line(reader: impl std::io::Read) -> std::io::Result<String> {
    use std::io::BufRead as _;

    let mut line = String::new();
    std::io::BufReader::new(reader.take(UI_REQUEST_LINE_LIMIT)).read_line(&mut line)?;
    Ok(line)
}

/// localhost-only poke listener: any local process (a script, the installer,
/// or a second ordinary launch) can nudge the one resident workbench to
/// re-read the daemon immediately. Never binds beyond 127.0.0.1.
fn start_ui_poke_listener(listener: std::net::TcpListener, app: tauri::AppHandle) {
    std::thread::spawn(move || {
        use std::io::Write as _;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(250)));
            let request_line = read_ui_request_line(&mut stream).unwrap_or_default();
            let route = poke_route(request_line.trim_end_matches(['\r', '\n']));
            let (code, body) = match route {
                "show" => {
                    let ui_app = app.clone();
                    match app.run_on_main_thread(move || {
                        reveal_from_tray(&ui_app);
                        let _ = ui_app.emit("ui-poked", serde_json::json!({}));
                    }) {
                        Ok(()) => ("200 OK", "show scheduled"),
                        Err(_) => ("500 INTERNAL SERVER ERROR", "show unavailable"),
                    }
                }
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
}

fn request_running_ui(path: &str) -> std::io::Result<String> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", UI_POKE_PORT))?;
    use std::io::Write;
    let request = ui_request(path);
    stream.write_all(request.as_bytes())?;
    stream.flush()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut buf = Vec::new();
    let _ = std::io::Read::read_to_end(&mut stream, &mut buf);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn ui_request(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
}

fn poke_running_ui() -> std::io::Result<String> {
    request_running_ui("/poke")
}

fn show_running_ui() -> std::io::Result<String> {
    request_running_ui("/show")
}

fn poke_response_acknowledged(response: &str) -> bool {
    response.contains("200 OK")
}

/// Second-instance nudge: `koi-desktop --poke` pokes every running UI on
/// this machine and exits without starting a new workbench.
fn run_poke_and_exit() -> ! {
    let result = poke_running_ui();
    match result {
        Ok(text) => println!(
            "poked a running koi ui: {}",
            if poke_response_acknowledged(&text) {
                "acknowledged".to_string()
            } else {
                text
            }
        ),
        Err(e) => println!("no koi ui running on 127.0.0.1:{} ({e})", UI_POKE_PORT),
    }
    std::process::exit(0);
}
/// Publish the workbench's fixed browser bundle, then express the operator's
/// desire for Pond to run. The daemon owns interface choice, socket binding,
/// firewall assessment, retry, and the exact URL returned to this view.
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
    )?;
    daemon_json("PUT", "/v1/pond", None)
}

#[tauri::command]
fn pond_disable() -> Result<serde_json::Value, String> {
    daemon_json("DELETE", "/v1/pond", None)
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
    let mut out =
        serde_json::json!({ "up": false, "version": null, "posture": null, "data_root": null });
    let Ok(access) = local_daemon::discover() else {
        return Ok(out);
    };
    out["data_root"] = serde_json::json!(access.data_root.clone());
    if let Some(status) = get_local_json(&agent, &access, "/v1/status") {
        out["up"] = serde_json::Value::Bool(true);
        out["version"] = status["version"].clone();
        if let Some(posture) = get_local_json(&agent, &access, "/v1/certmesh/posture") {
            out["posture"] = posture["level"].clone();
        }
    }
    Ok(out)
}

/// The whole /v1/status document — the honest glass pane shows the capability
/// ladder with the daemon's own words (skip reasons are data, not log lines).
#[tauri::command]
fn daemon_status_full() -> Result<serde_json::Value, String> {
    daemon_json("GET", "/v1/status", None)
        .map_err(|_| "no daemon — the ladder is unknown, not healthy".to_string())
}

#[tauri::command]
fn discover_snapshot() -> Result<serde_json::Value, String> {
    daemon_json("GET", "/v1/mdns/browser/snapshot", None)
}

/// "Ping the pond": force the daemon's mDNS query burst so every client on the
/// LAN answers immediately. The endpoint is DAT-gated (a POST); the breadcrumb
/// carries the token, exactly like the CLI.
#[tauri::command]
fn discover_ping() -> Result<serde_json::Value, String> {
    daemon_json(
        "POST",
        "/v1/mdns/browser/query",
        Some(serde_json::json!({})),
    )
}

/// Authenticated daemon request: breadcrumb token attached, JSON body in/out.
/// Mutations are DAT-gated server-side; reads stay loopback-exempt.
fn daemon_json(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let access = local_daemon::discover()
        .map_err(|error| format!("no local Koi daemon access — {error}"))?;
    let mut request = match method {
        "POST" => daemon_agent().post(access.url(path).as_str()),
        "PUT" => daemon_agent().put(access.url(path).as_str()),
        "DELETE" => daemon_agent().delete(access.url(path).as_str()),
        _ => daemon_agent().get(access.url(path).as_str()),
    };
    request = request.set("x-koi-token", &access.token);
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
async fn certmesh_join(
    endpoint: String,
    invite: Option<String>,
    totp: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || certmesh_join_blocking(endpoint, invite, totp))
        .await
        .map_err(|e| format!("join task: {e}"))?
}

fn certmesh_join_blocking(
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

#[derive(Serialize)]
struct AutostartState {
    handled: bool,
    enabled: bool,
    detail: String,
}

const HYPR_AUTOSTART_BEGIN: &str = "-- BEGIN Koi desktop autostart (managed by Koi)";
const HYPR_AUTOSTART_END: &str = "-- END Koi desktop autostart (managed by Koi)";

/// Omarchy/Hyprland does not consume XDG autostart entries. Report whether
/// this session needs Koi's compositor-native startup adapter; every other
/// desktop continues through tauri-plugin-autostart.
#[tauri::command]
fn autostart_state() -> Result<AutostartState, String> {
    let Some(path) = hyprland_autostart_path() else {
        return Ok(AutostartState {
            handled: false,
            enabled: false,
            detail: "the desktop uses its native autostart provider".into(),
        });
    };
    let source = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let enabled = managed_autostart_range(&source)?.is_some();
    Ok(AutostartState {
        handled: true,
        enabled,
        detail: format!("managed through {}", path.display()),
    })
}

/// Add or remove only Koi's marked block. The surrounding Hyprland config is
/// operator-owned and is preserved byte-for-byte.
#[tauri::command]
fn autostart_set(enabled: bool) -> Result<AutostartState, String> {
    let Some(path) = hyprland_autostart_path() else {
        return Ok(AutostartState {
            handled: false,
            enabled: false,
            detail: "the desktop uses its native autostart provider".into(),
        });
    };
    let source = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let executable = std::env::current_exe()
        .map_err(|e| format!("could not locate the Koi workbench executable: {e}"))?;
    if enabled && is_checkout_binary(&executable) {
        return Err(
            "install Koi Desktop to a durable location before enabling login startup; source-checkout build paths are not retained"
                .into(),
        );
    }
    let updated = render_hyprland_autostart(&source, enabled, &executable)?;
    if updated != source {
        let temporary = path.with_extension(format!("lua.koi-tmp-{}", std::process::id()));
        std::fs::write(&temporary, updated)
            .map_err(|e| format!("could not write {}: {e}", temporary.display()))?;
        let permissions = std::fs::metadata(&path)
            .map_err(|e| format!("could not inspect {}: {e}", path.display()))?
            .permissions();
        std::fs::set_permissions(&temporary, permissions)
            .map_err(|e| format!("could not preserve permissions on {}: {e}", path.display()))?;
        std::fs::rename(&temporary, &path)
            .map_err(|e| format!("could not replace {}: {e}", path.display()))?;
    }
    Ok(AutostartState {
        handled: true,
        enabled,
        detail: format!("managed through {}", path.display()),
    })
}

fn is_checkout_binary(executable: &std::path::Path) -> bool {
    executable.ancestors().any(|ancestor| {
        ancestor.file_name().is_some_and(|name| name == "target")
            && ancestor
                .parent()
                .is_some_and(|parent| parent.join("Cargo.toml").is_file())
    })
}

#[cfg(target_os = "linux")]
fn hyprland_autostart_path() -> Option<std::path::PathBuf> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if !desktop.to_ascii_lowercase().contains("hyprland") {
        return None;
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })?;
    let path = config.join("hypr/autostart.lua");
    path.is_file().then_some(path)
}

#[cfg(not(target_os = "linux"))]
fn hyprland_autostart_path() -> Option<std::path::PathBuf> {
    None
}

fn managed_autostart_range(source: &str) -> Result<Option<std::ops::Range<usize>>, String> {
    match (source.find(HYPR_AUTOSTART_BEGIN), source.find(HYPR_AUTOSTART_END)) {
        (None, None) => Ok(None),
        (Some(begin), Some(end)) if begin < end => {
            let mut finish = end + HYPR_AUTOSTART_END.len();
            if source.as_bytes().get(finish) == Some(&b'\n') {
                finish += 1;
            }
            Ok(Some(begin..finish))
        }
        _ => Err("Koi's managed Hyprland autostart block is incomplete; repair it manually before changing this setting".into()),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\\"'\\\"'"))
}

fn render_hyprland_autostart(
    source: &str,
    enabled: bool,
    executable: &std::path::Path,
) -> Result<String, String> {
    let mut rendered = source.to_owned();
    if let Some(range) = managed_autostart_range(&rendered)? {
        rendered.replace_range(range, "");
    }
    if enabled {
        if !rendered.is_empty() && !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        let command = format!("{} --minimized", shell_quote(&executable.to_string_lossy()));
        let lua_command = serde_json::to_string(&command)
            .map_err(|e| format!("could not encode the autostart command: {e}"))?;
        rendered.push_str(HYPR_AUTOSTART_BEGIN);
        rendered.push('\n');
        rendered.push_str(&format!("o.launch_on_start({lua_command})\n"));
        rendered.push_str(HYPR_AUTOSTART_END);
        rendered.push('\n');
    }
    Ok(rendered)
}

/// Passage liveness probe (operator direction): an Open button is only shown
/// once an HTTP server has actually ANSWERED at the endpoint. HEAD first with
/// GET as fallback; ANY status code counts (404/401 still mean "an HTTP server
/// is listening" — the browser renders the page, we only claim the server).
/// GET is only tried when the server ANSWERED the HEAD badly (bad status/body
/// line): a refused or timed-out connection would fail GET identically, so we
/// skip it and stay fast. https is tried after http; a TLS-handshake failure
/// stays unconfirmed rather than guessed (the self-signed corner is F2's open
/// question), so a dead port and an unprobeable port look the same: no button.
///
/// Returns the scheme that answered — the UI composes the URL from THIS, so
/// the button never promises a scheme the probe did not verify.
///
/// ASYNC + spawn_blocking: blocking network I/O in a sync Tauri command runs
/// on the MAIN thread and froze the UI when the boot render queued a probe
/// per announcement (measured 2026-08-29). Heavy commands must yield.
#[tauri::command]
async fn probe_http(host: String, port: u16) -> Result<Option<String>, String> {
    let host = host.trim().trim_end_matches('.').to_string();
    if host.is_empty()
        || host.contains(['/', '\\', ' ', '@', '?', '#'])
        || !(host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')))
    {
        return Err("the endpoint host looks wrong".into());
    }
    if port == 0 {
        return Err("the endpoint port looks wrong".into());
    }
    tauri::async_runtime::spawn_blocking(move || probe_http_blocking(host, port))
        .await
        .map_err(|e| format!("probe task: {e}"))?
}

fn probe_http_blocking(host: String, port: u16) -> Result<Option<String>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .build();
    for scheme in ["http", "https"] {
        let url = format!("{scheme}://{host}:{port}/");
        for method in ["HEAD", "GET"] {
            let request = if method == "HEAD" {
                agent.head(&url)
            } else {
                agent.get(&url)
            };
            match request.call() {
                // Any response at all — even 404/405/500 — is an HTTP server.
                Ok(_) => return Ok(Some(scheme.to_string())),
                Err(ureq::Error::Status(_, _)) => return Ok(Some(scheme.to_string())),
                Err(ureq::Error::Transport(t))
                    if matches!(
                        t.kind(),
                        ureq::ErrorKind::BadStatus | ureq::ErrorKind::BadHeader
                    ) =>
                {
                    // Something answered but disliked HEAD — one GET retry.
                    continue;
                }
                // Connect refused / timeout / TLS / DNS: GET would fail the
                // same way, so this scheme is settled without the retry.
                Err(_) => break,
            }
        }
    }
    Ok(None)
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

fn fetch_status_value(
    agent: &ureq::Agent,
    access: &local_daemon::DaemonAccess,
) -> serde_json::Value {
    let mut out = serde_json::json!({
        "up": true,
        "version": null,
        "posture": null,
        "data_root": access.data_root.clone(),
    });
    if let Some(status) = get_local_json(agent, access, "/v1/status") {
        out["version"] = status["version"].clone();
        if let Some(posture) = get_local_json(agent, access, "/v1/certmesh/posture") {
            out["posture"] = posture["level"].clone();
        }
    }
    out
}

fn emit_down_status(app: &tauri::AppHandle) {
    // "Down" means the daemon is truly absent — healthz fails too. A daemon
    // that merely lacks /v1/events (older builds) is still up.
    let healthz_ok = local_daemon::discover()
        .is_ok_and(|access| daemon_agent().get(&access.url("/healthz")).call().is_ok());
    if !healthz_ok {
        let _ = app.emit(
            "daemon-status",
            serde_json::json!({
                "up": false,
                "version": null,
                "posture": null,
                "data_root": null,
            }),
        );
    }
}

static STATUS_STREAM_STARTED: AtomicBool = AtomicBool::new(false);

fn decode_daemon_event(kind: &str, data: &str) -> Option<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_str(data).ok()?;
    let Some(version) = parsed.get("event_v") else {
        // Compatibility with the older dashboard stream, whose data field was
        // already the domain payload rather than a versioned wire envelope.
        return Some(parsed);
    };
    if version.as_u64() != Some(1)
        || parsed.get("event_type").and_then(|value| value.as_str()) != Some(kind)
    {
        return None;
    }
    parsed.get("data").cloned()
}

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
            let Ok(access) = local_daemon::discover() else {
                emit_down_status(&app);
                std::thread::sleep(Duration::from_secs(3));
                continue;
            };
            let request = agent
                .get(&access.url("/v1/events"))
                .set("x-koi-token", &access.token);
            match request.call() {
                Ok(response) => {
                    let _ = app.emit("daemon-status", fetch_status_value(&agent, &access));
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
                                let _ =
                                    app.emit("daemon-status", fetch_status_value(&agent, &access));
                            } else if !kind.is_empty() {
                                if let Some(payload) = decode_daemon_event(&kind, &data) {
                                    let _ = app.emit(
                                        "daemon-event",
                                        serde_json::json!({ "kind": kind, "data": payload }),
                                    );
                                }
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
            let response = local_daemon::discover().and_then(|access| {
                agent
                    .get(&access.url("/v1/mdns/browser/events"))
                    .set("x-koi-token", &access.token)
                    .call()
                    .map_err(|error| error.to_string())
            });
            match response {
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

#[cfg(windows)]
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
        #[cfg(target_os = "linux")]
        {
            linux_service_status()
        }
        #[cfg(not(target_os = "linux"))]
        ServiceStatus {
            installed: false,
            running: false,
            detail: Some("service controls are unavailable on this platform".into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_service_status() -> ServiceStatus {
    service_manager::status()
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
        #[cfg(target_os = "linux")]
        {
            service_manager::start().map(StartResult::ok)
        }
        #[cfg(not(target_os = "linux"))]
        Err("service controls are unavailable on this platform".into())
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
        #[cfg(target_os = "linux")]
        {
            service_manager::stop().map(StartResult::ok)
        }
        #[cfg(not(target_os = "linux"))]
        Err("service controls are unavailable on this platform".into())
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
    let installed = service_status();
    if installed.installed {
        return Err(if installed.running {
            "Koi is already running as the installed service; refusing to start a second instance."
                .into()
        } else {
            "Koi is installed as a service; start that service instead of creating a parallel instance."
                .into()
        });
    }
    // A successful hand-off is itself proof of a live daemon even when its HTTP
    // adapter is disabled or still reconciling. A private breadcrumb may be a
    // crash artifact, but treating it as an ownership collision is safer than
    // racing a replacement process; the operator can repair the one real
    // deployment explicitly.
    if local_daemon::discover().is_ok() {
        return Err(
            "Koi is already running on this machine; refusing to start a second instance.".into(),
        );
    }
    let exe = locate_koi_exe().ok_or_else(|| {
        "koi.exe was not found on PATH or in its usual install locations. \
         Install Koi first (`winget`/archive), then try again."
            .to_string()
    })?;
    let mut command = Command::new(&exe);
    command
        .arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW | 0x0000_0008 /* DETACHED_PROCESS */);
    let child = command
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
    let window = match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => window,
        None => build_workbench(app)?,
    };
    window.show()?;
    window.set_focus()
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
    let access = local_daemon::discover().ok()?;
    let body = ureq::get(&access.url("/v1/certmesh/posture"))
        .set("x-koi-token", &access.token)
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
    use std::sync::atomic::AtomicBool;

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
    fn close_only_stays_resident_when_the_tray_exists() {
        assert!(super::close_keeps_tray_alive(&AtomicBool::new(true)));
        assert!(!super::close_keeps_tray_alive(&AtomicBool::new(false)));
    }

    #[test]
    fn only_a_successful_poke_response_is_acknowledged() {
        assert!(super::poke_response_acknowledged(
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\npoked"
        ));
        assert!(!super::poke_response_acknowledged(
            "HTTP/1.1 404 NOT FOUND\r\n\r\n"
        ));
    }

    #[test]
    fn ordinary_launch_and_poke_have_distinct_routes() {
        assert_eq!(super::poke_route("GET /show HTTP/1.1"), "show");
        assert_eq!(super::poke_route("GET /poke HTTP/1.1"), "poke");
        assert_eq!(super::poke_route("GET /health HTTP/1.1"), "health");
        assert_eq!(super::poke_route("GET /unknown HTTP/1.1"), "other");
    }

    struct FragmentedReader(std::io::Cursor<Vec<u8>>);

    impl std::io::Read for FragmentedReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let end = buf.len().min(3);
            self.0.read(&mut buf[..end])
        }
    }

    #[test]
    fn ui_request_line_survives_fragmented_tcp_reads() {
        let request = super::ui_request("/poke");
        let reader = FragmentedReader(std::io::Cursor::new(request.into_bytes()));
        let line = super::read_ui_request_line(reader).unwrap();

        assert_eq!(line, "GET /poke HTTP/1.1\r\n");
        assert_eq!(
            super::poke_route(line.trim_end_matches(['\r', '\n'])),
            "poke"
        );
    }

    #[test]
    fn ui_request_is_one_exact_bounded_http_message() {
        let request = super::ui_request("/show");
        assert_eq!(
            request,
            "GET /show HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        );
        assert!(request.len() < super::UI_REQUEST_LINE_LIMIT as usize);
    }

    #[test]
    fn daemon_event_decoder_unwraps_the_versioned_wire_payload() {
        let raw = serde_json::json!({
            "event_v": 1,
            "event_type": "runtime.stopped",
            "id": "event-1",
            "data": { "id": "container-1", "name": "forge" }
        })
        .to_string();

        let payload = super::decode_daemon_event("runtime.stopped", &raw)
            .expect("a version-one event should decode");
        assert_eq!(payload["id"], "container-1");
        assert_eq!(payload["name"], "forge");
        assert!(payload.get("event_v").is_none());
    }

    #[test]
    fn daemon_event_decoder_skips_unknown_versions_and_mismatched_kinds() {
        let unknown = serde_json::json!({
            "event_v": 2,
            "event_type": "runtime.stopped",
            "id": "event-2",
            "data": { "name": "forge" }
        })
        .to_string();
        assert!(super::decode_daemon_event("runtime.stopped", &unknown).is_none());

        let mismatched = serde_json::json!({
            "event_v": 1,
            "event_type": "runtime.started",
            "id": "event-3",
            "data": { "name": "forge" }
        })
        .to_string();
        assert!(super::decode_daemon_event("runtime.stopped", &mismatched).is_none());
    }

    #[test]
    fn daemon_event_decoder_keeps_legacy_bare_payloads_compatible() {
        let raw = serde_json::json!({ "id": "container-1", "name": "forge" }).to_string();
        let payload = super::decode_daemon_event("runtime.stopped", &raw)
            .expect("a legacy payload should still decode");
        assert_eq!(payload["name"], "forge");
    }

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

    #[test]
    fn hyprland_autostart_preserves_user_config_and_round_trips() {
        let original = "-- Extra autostart processes.\no.launch_on_start(\"keep-me\")\n";
        let executable = std::path::Path::new("/opt/Koi pond/koi-desktop");
        let enabled = super::render_hyprland_autostart(original, true, executable)
            .expect("managed block should render");
        assert!(enabled.starts_with(original));
        assert!(enabled.contains(super::HYPR_AUTOSTART_BEGIN));
        assert!(enabled.contains("'/opt/Koi pond/koi-desktop' --minimized"));
        assert_eq!(
            super::render_hyprland_autostart(&enabled, false, executable)
                .expect("managed block should be removable"),
            original
        );
    }

    #[test]
    fn hyprland_autostart_replaces_stale_installed_path() {
        let old = super::render_hyprland_autostart(
            "-- mine\n",
            true,
            std::path::Path::new("/old/koi-desktop"),
        )
        .expect("old block should render");
        let updated =
            super::render_hyprland_autostart(&old, true, std::path::Path::new("/new/koi-desktop"))
                .expect("old block should be replaced");
        assert!(!updated.contains("/old/koi-desktop"));
        assert!(updated.contains("/new/koi-desktop"));
        assert_eq!(updated.matches(super::HYPR_AUTOSTART_BEGIN).count(), 1);
    }

    #[test]
    fn hyprland_autostart_refuses_a_partial_managed_block() {
        let broken = format!(
            "{}\no.launch_on_start(\"koi\")\n",
            super::HYPR_AUTOSTART_BEGIN
        );
        assert!(super::render_hyprland_autostart(
            &broken,
            false,
            std::path::Path::new("/usr/bin/koi-desktop"),
        )
        .is_err());
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
