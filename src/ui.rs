//! Normal shared shell. The authenticated catalog and rendered document stay in Rust.

use koi_client::KoiClient;
use koi_ui::{Links, View};
use tauri::http::{Method, Request, Response, StatusCode};
use tauri::Manager;

const SCHEME: &str = "koi-ui";
pub const URL: &str = "koi-ui://localhost/";
const CSP: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    let comparisons = crate::comparison::Store::default();
    builder.register_asynchronous_uri_scheme_protocol(SCHEME, move |context, request, responder| {
        if !allowed(context.webview_label(), &request) {
            responder.respond(response(StatusCode::NOT_FOUND, String::new()));
            return;
        }
        if request.uri().path() == "/compare" {
            let peer = comparison_peer(&request).expect("validated comparison intent");
            let status = if comparisons.start(peer) {
                StatusCode::ACCEPTED
            } else {
                StatusCode::CONFLICT
            };
            responder.respond(response(status, String::new()));
            return;
        }
        if request.uri().path() == "/refresh.js" {
            let mut reply = response(StatusCode::OK, koi_ui::REFRESH_JS.into());
            reply.headers_mut().insert(
                "content-type",
                "text/javascript; charset=utf-8".parse().unwrap(),
            );
            responder.respond(reply);
            return;
        }
        // Never block the webview thread on local-control I/O. The existing client
        // owns discovery, authentication, timeouts and schema validation.
        let comparisons = comparisons.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let intent = koi_ui::home::HomeRequest::parse(request.uri().query().unwrap_or(""))
                .expect("allowed request validated Home intent");
            let query = intent.query();
            let refresh = query.href(query.selected);
            let links = Links {
                refresh: Some(&refresh),
                ..links()
            };
            let (status, document) = match read_catalog() {
                Ok(catalog) => (
                    StatusCode::OK,
                    koi_ui::render_workbench(
                        View::Snapshot(&catalog),
                        links,
                        &query,
                        &comparisons.view(query.peer),
                    ),
                ),
                Err(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    koi_ui::render_home(View::Unavailable, links, &query),
                ),
            };
            responder.respond(response(
                status,
                document.replace(
                    "</head>",
                    "<script defer src=\"/refresh.js\"></script></head>",
                ),
            ));
        });
    })
}

// Keep the advanced asset origin unchanged so its local storage/migration survives.
#[cfg(windows)]
const ADVANCED_URL: &str = "http://tauri.localhost/";
#[cfg(not(windows))]
const ADVANCED_URL: &str = "tauri://localhost/";

fn links() -> Links<'static> {
    Links {
        refresh: Some("./"),
        advanced: ADVANCED_URL,
    }
}

fn read_catalog() -> Result<koi_ui::CatalogSnapshot, String> {
    let access = crate::local_daemon::discover()?;
    KoiClient::with_token(&access.endpoint, &access.token)
        .catalog_snapshot()
        .map_err(|_| "catalog unavailable".into())
}

#[tauri::command]
pub fn show_shared_shell(
    window: tauri::WebviewWindow,
    section: Option<String>,
) -> Result<(), String> {
    if window.label() != crate::MAIN_WINDOW {
        return Err("unknown workbench".into());
    }
    let destination = match section.as_deref() {
        None => navigation_url(cfg!(windows)).to_string(),
        Some("comparison") => format!("{}#comparison", navigation_url(cfg!(windows))),
        _ => return Err("unknown shared shell section".into()),
    };
    window
        .navigate(destination.parse().expect("static shared shell URL"))
        .map_err(|_| "cannot open the shared shell".into())
}

fn navigation_url(windows: bool) -> &'static str {
    // Initial custom-protocol setup rewrites the Windows URL, but Wry's later
    // load_url goes straight to WebView2 Navigate. Use its registered HTTP origin
    // here; a raw koi-ui: navigation never reaches that protocol handler.
    if windows {
        "http://koi-ui.localhost/"
    } else {
        URL
    }
}

fn allowed(label: &str, request: &Request<Vec<u8>>) -> bool {
    let uri = request.uri();
    let local_origin = matches!(
        (
            uri.scheme_str(),
            uri.authority().map(|value| value.as_str())
        ),
        (Some("koi-ui"), Some("localhost")) | (Some("http"), Some("koi-ui.localhost"))
    );
    label == crate::MAIN_WINDOW
        && local_origin
        && match uri.path() {
            "/" => {
                request.method() == Method::GET
                    && request.body().is_empty()
                    && koi_ui::home::HomeRequest::parse(uri.query().unwrap_or("")).is_ok()
            }
            "/refresh.js" => {
                request.method() == Method::GET
                    && request.body().is_empty()
                    && uri.query().is_none()
            }
            "/compare" => comparison_peer(request).is_some(),
            _ => false,
        }
}

fn comparison_peer(request: &Request<Vec<u8>>) -> Option<koi_ui::devices::DeviceId> {
    if request.method() != Method::POST
        || request.uri().query().is_some()
        || request.body().len() > 512
    {
        return None;
    }
    let body = std::str::from_utf8(request.body()).ok()?;
    if !body.starts_with("peer=") || body.contains('&') {
        return None;
    }
    koi_ui::home::HomeRequest::parse(body).ok()?.peer
}

/// Keep real service URLs inspectable while opening them outside the workbench.
pub fn navigate(app: &tauri::AppHandle, url: &tauri::Url) -> bool {
    if internal_navigation(url) {
        return true;
    }
    if koi_ui::home::BrowserDestination::parse(url.as_str()).is_some() {
        let app = app.clone();
        let url = url.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            let message = match crate::external::open(&url) {
                Ok(()) => {
                    "Browser launch requested. This does not verify the destination.".to_string()
                }
                Err(error) => format!("Could not open the browser: {error}"),
            };
            if let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) {
                let text = serde_json::to_string(&message).expect("string serializes");
                let _ = window.eval(format!("(() => {{ const status = document.getElementById('open-status'); if (status) status.textContent = {text}; }})()"));
            }
        });
    }
    false
}

fn internal_navigation(url: &tauri::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && matches!(
            (url.scheme(), url.host_str()),
            ("koi-ui" | "tauri", Some("localhost"))
                | ("http", Some("koi-ui.localhost" | "tauri.localhost"))
        )
}

fn response(status: StatusCode, document: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/html; charset=utf-8")
        .header("content-security-policy", CSP)
        .header("cache-control", "no-store")
        .header("x-content-type-options", "nosniff")
        .body(document.into_bytes())
        .expect("static response headers are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, url: &str) -> Request<Vec<u8>> {
        Request::builder()
            .method(method)
            .uri(url)
            .body(Vec::new())
            .unwrap()
    }

    #[test]
    fn only_exact_native_shared_shell_roots_are_allowed() {
        for url in [URL, "http://koi-ui.localhost/"] {
            assert!(allowed(crate::MAIN_WINDOW, &request("GET", url)));
            assert!(allowed(
                crate::MAIN_WINDOW,
                &request("GET", &format!("{url}refresh.js"))
            ));
            assert!(!allowed(
                crate::MAIN_WINDOW,
                &request("GET", &format!("{url}refresh.js?path=secret"))
            ));
            assert!(allowed(
                crate::MAIN_WINDOW,
                &request(
                    "GET",
                    &format!("{url}?search=Office+web&selected=notes&favorites=1")
                )
            ));
        }
        for url in [
            "koi-ui://remote/",
            "koi-ui://localhost:80/",
            "koi-ui://user@localhost/",
            "koi-ui://localhost/assets/koi.png",
            "koi-ui://localhost/?path=/etc/passwd",
            "koi-ui://localhost/../",
            "http://localhost/",
            "https://koi-ui.localhost/",
            "/",
        ] {
            assert!(!allowed(crate::MAIN_WINDOW, &request("GET", url)), "{url}");
        }
    }

    #[test]
    fn service_destinations_cannot_replace_the_privileged_workbench() {
        for url in [
            URL,
            ADVANCED_URL,
            "http://koi-ui.localhost/?search=notes#service-details",
        ] {
            assert!(internal_navigation(&url.parse().unwrap()), "{url}");
        }
        for url in [
            "https://notes.local:8443/",
            "http://koi-ui.localhost.evil/",
            "http://user@koi-ui.localhost/",
            "http://koi-ui.localhost:8080/",
            "javascript:alert(1)",
            "file:///tmp/page",
        ] {
            assert!(!internal_navigation(&url.parse().unwrap()), "{url}");
        }
    }

    #[test]
    fn return_navigation_uses_the_platform_registered_protocol_origin() {
        for (windows, scheme, host) in [
            (true, "http", "koi-ui.localhost"),
            (false, "koi-ui", "localhost"),
        ] {
            let url = navigation_url(windows);
            let parsed: tauri::Url = url.parse().unwrap();
            assert_eq!(parsed.scheme(), scheme);
            assert_eq!(parsed.host_str(), Some(host));
            assert!(allowed(crate::MAIN_WINDOW, &request("GET", url)));
        }
    }

    #[test]
    fn other_windows_methods_and_bodies_cannot_read_catalog() {
        assert!(!allowed("other", &request("GET", URL)));
        for method in ["POST", "PUT", "DELETE", "HEAD", "OPTIONS"] {
            assert!(!allowed(crate::MAIN_WINDOW, &request(method, URL)));
        }
        let mut body = request("GET", URL);
        body.body_mut().push(1);
        assert!(!allowed(crate::MAIN_WINDOW, &body));
    }

    #[test]
    fn comparison_action_accepts_only_bounded_peer_intent_on_the_native_origin() {
        let mut action = request("POST", "koi-ui://localhost/compare");
        *action.body_mut() = b"peer=office".to_vec();
        assert!(allowed(crate::MAIN_WINDOW, &action));
        assert!(!allowed("other", &action));
        assert!(!allowed(
            crate::MAIN_WINDOW,
            &request("GET", "koi-ui://localhost/compare")
        ));
        for body in [
            "peer=",
            "peer=office&path=/etc/passwd",
            "peer=office&peer=other",
            "path=/x",
            "peer=UPPER",
        ] {
            *action.body_mut() = body.as_bytes().to_vec();
            assert!(!allowed(crate::MAIN_WINDOW, &action), "{body}");
        }
        *action.body_mut() = format!("peer={}", "x".repeat(512)).into_bytes();
        assert!(!allowed(crate::MAIN_WINDOW, &action));
    }

    #[test]
    fn unavailable_document_is_safe_and_uncached() {
        let reply = response(
            StatusCode::SERVICE_UNAVAILABLE,
            koi_ui::render(View::Unavailable, links()),
        );
        assert_eq!(reply.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(reply.headers()["cache-control"], "no-store");
        assert_eq!(reply.headers()["content-security-policy"], CSP);
        let body = String::from_utf8(reply.into_body()).unwrap();
        assert!(!body.contains("<script"));
        assert!(!body.contains("x-koi-token"));
        assert!(body.contains("Cannot read the local catalog."));
    }
}
