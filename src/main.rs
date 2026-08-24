//! Koi's desktop workbench: a friendly native shell over the local substrate.
//!
//! Visual language and desktop patterns are borrowed from Ghostlight
//! (`sylin-org/browser-mcp`, `crates/orchestrator/src/desktop/mod.rs`) and
//! re-skinned with the Koi identity published on sylin.org (accent `#60a5fa`,
//! light `#93c5fd`, ground `#0f0e12`). The daemon stays headless; this shell
//! is one more intake adapter over the same loopback HTTP surface.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, RunEvent, WebviewWindowBuilder, WindowEvent};

const MAIN_WINDOW: &str = "main";
const DAEMON_ORIGIN: &str = "http://127.0.0.1:5641";

struct Workbench {
    tray_available: Arc<AtomicBool>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Koi workbench failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let workbench = Workbench {
        tray_available: Arc::new(AtomicBool::new(false)),
    };
    let setup_tray_available = Arc::clone(&workbench.tray_available);

    let app = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tauri::Builder::default()
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
            .on_window_event(|window, event| {
                // Closing the window keeps the tray alive — Quit is the only exit,
                // mirroring Ghostlight's disposable-view lifecycle.
                if let WindowEvent::Destroyed = event {
                    eprintln!("Koi workbench window ended; the tray can recreate it");
                }
                let _ = window;
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
