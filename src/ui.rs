//! Normal shared shell. The authenticated catalog and rendered document stay in Rust.

use koi_client::KoiClient;
use koi_ui::{Links, View};
use tauri::http::{Method, Request, Response, StatusCode};

const SCHEME: &str = "koi-ui";
pub const URL: &str = "koi-ui://localhost/";
const CSP: &str = koi_ui::DOCUMENT_CSP;

pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.register_asynchronous_uri_scheme_protocol(SCHEME, |context, request, responder| {
        if !allowed(context.webview_label(), &request) {
            responder.respond(response(StatusCode::NOT_FOUND, String::new()));
            return;
        }
        // Never block the webview thread on local-control I/O. The existing client
        // owns discovery, authentication, timeouts and schema validation.
        tauri::async_runtime::spawn_blocking(move || {
            let (status, document) = match read_catalog() {
                Ok(catalog) => (
                    StatusCode::OK,
                    koi_ui::render(View::Snapshot(&catalog), links()),
                ),
                Err(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    koi_ui::render(View::Unavailable, links()),
                ),
            };
            responder.respond(response(status, document));
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
pub fn show_shared_shell(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != crate::MAIN_WINDOW {
        return Err("unknown workbench".into());
    }
    window
        .navigate(URL.parse().expect("static shared shell URL"))
        .map_err(|_| "cannot open the shared shell".into())
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
        && request.method() == Method::GET
        && local_origin
        && uri.path() == "/"
        && uri.query().is_none()
        && request.body().is_empty()
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
