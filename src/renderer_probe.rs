//! Temporary R06 candidate evaluation, never the default/autostart surface.
//! The shared document and authenticated catalog stay entirely in Rust.

use koi_client::KoiClient;
use koi_ui_spike::{maud_view, View};
use tauri::http::{Method, Request, Response, StatusCode};

const SCHEME: &str = "koi-renderer";
pub const URL: &str = "koi-renderer://localhost/";
const CSP: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub fn enabled() -> bool {
    std::env::args().any(|arg| arg == "--renderer-probe")
}

pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    if !enabled() {
        return builder;
    }
    builder.register_asynchronous_uri_scheme_protocol(SCHEME, |context, request, responder| {
        if !allowed(context.webview_label(), &request) {
            responder.respond(response(StatusCode::NOT_FOUND, String::new()));
            return;
        }
        // Never block the webview thread on local-control I/O. The existing client
        // owns discovery, authentication, timeouts and schema validation.
        tauri::async_runtime::spawn_blocking(move || {
            let (status, document) =
                match KoiClient::from_local().and_then(|client| client.catalog_snapshot()) {
                    Ok(catalog) => (StatusCode::OK, maud_view::render(View::Snapshot(&catalog))),
                    Err(_) => (
                        StatusCode::SERVICE_UNAVAILABLE,
                        maud_view::render(View::Unavailable),
                    ),
                };
            responder.respond(response(status, document));
        });
    })
}

fn allowed(label: &str, request: &Request<Vec<u8>>) -> bool {
    let uri = request.uri();
    let local_origin = matches!(
        (
            uri.scheme_str(),
            uri.authority().map(|value| value.as_str())
        ),
        (Some("koi-renderer"), Some("localhost")) | (Some("http"), Some("koi-renderer.localhost"))
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
    fn only_exact_native_evaluation_roots_are_allowed() {
        for url in [URL, "http://koi-renderer.localhost/"] {
            assert!(allowed(crate::MAIN_WINDOW, &request("GET", url)));
        }
        for url in [
            "koi-renderer://remote/",
            "koi-renderer://localhost:80/",
            "koi-renderer://user@localhost/",
            "koi-renderer://localhost/assets/koi.png",
            "koi-renderer://localhost/?path=/etc/passwd",
            "koi-renderer://localhost/../",
            "http://localhost/",
            "https://koi-renderer.localhost/",
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
            maud_view::render(View::Unavailable),
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
