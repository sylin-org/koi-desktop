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
            .invoke_handler(tauri::generate_handler![
                service_status,
                service_start,
                service_stop,
                daemon_run_once,
                daemon_get,
                daemon_status,
                discover_start,
                discover_snapshot,
                discover_ping,
                dns_entries,
                dns_add,
                dns_remove,
                dns_txt_set,
                dns_txt_clear,
                status_events_start,
                debug_log
            ])
            .setup(move |app| {
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
    get_json(&daemon_agent(), url.clone()).ok_or_else(|| format!("{url}: no data"))
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
