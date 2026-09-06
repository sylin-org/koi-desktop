//! Native browser-access controls. The daemon alone owns invitations and sessions.
use koi_client::{browser_access::BrowserAccessSettings, KoiClient};
use tauri::http::{Method, Request, Response, StatusCode};

pub fn handles(request: &Request<Vec<u8>>) -> bool {
    request.uri().path() == "/web" || request.uri().path().starts_with("/web/")
}
pub fn allowed(request: &Request<Vec<u8>>) -> bool {
    if request.uri().query().is_some() || request.body().len() > 4096 {
        return false;
    }
    if request
        .headers()
        .get("origin")
        .is_some_and(|origin| origin != "koi-ui://localhost" && origin != "http://koi-ui.localhost")
    {
        return false;
    }
    match (request.method(), request.uri().path()) {
        (&Method::GET, "/web" | "/web/status" | "/web/status.js") => request.body().is_empty(),
        (&Method::POST, "/web/settings" | "/web/open" | "/web/invite") => true,
        (&Method::POST, path) => path.strip_prefix("/web/revoke/").is_some_and(|id| {
            id.len() == 43
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }),
        _ => false,
    }
}
pub fn handle(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match run(&request) {
        Ok((mime, document)) => response(StatusCode::OK, mime, document),
        Err(error) => response(
            StatusCode::SERVICE_UNAVAILABLE,
            "text/html; charset=utf-8",
            koi_ui::browser_access::unavailable_page(&error),
        ),
    }
}
fn run(request: &Request<Vec<u8>>) -> Result<(&'static str, String), String> {
    if request.uri().path() == "/web/status.js" {
        return Ok((
            "text/javascript; charset=utf-8",
            include_str!("../ui/browser-access-status.js").into(),
        ));
    }
    let access = crate::local_daemon::discover()?;
    let client = KoiClient::with_token(&access.endpoint, &access.token);
    let mut message = None;
    let mut invitation = None;
    if request.method() == Method::POST {
        match request.uri().path() {
            "/web/settings" => {
                let fields: Vec<_> = tauri::Url::parse(&format!(
                    "http://koi.invalid/?{}",
                    String::from_utf8_lossy(request.body())
                ))
                .map_err(|_| "Invalid settings")?
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
                if fields
                    .iter()
                    .any(|(k, v)| !matches!(k.as_str(), "enabled" | "phone") || v != "on")
                    || fields.len() > 2
                {
                    return Err("Invalid browser settings".into());
                }
                let enabled = fields.iter().any(|(k, _)| k == "enabled");
                let phone = enabled && fields.iter().any(|(k, _)| k == "phone");
                client
                    .browser_access_settings(&BrowserAccessSettings { enabled, phone })
                    .map_err(|e| e.to_string())?;
                message = Some("Browser access settings saved.");
            }
            "/web/invite" => {
                invitation = Some(client.browser_invitation(true).map_err(|e| e.to_string())?)
            }
            "/web/open" => {
                let invite = client
                    .browser_invitation(false)
                    .map_err(|e| e.to_string())?;
                crate::external::open(&format!("{}&open=1", invite.url))?;
                message = Some("Opening Koi in your browser.");
            }
            path => {
                let id = path
                    .strip_prefix("/web/revoke/")
                    .ok_or("Unknown browser action")?;
                client.browser_disconnect(id).map_err(|e| e.to_string())?;
                message = Some("Browser disconnected.");
            }
        }
    }
    let status = client.browser_access_status().map_err(|e| e.to_string())?;
    if request.uri().path() == "/web/status" {
        return Ok((
            "application/json",
            serde_json::to_string(&status).map_err(|e| e.to_string())?,
        ));
    }
    let qr = invitation
        .as_ref()
        .map(|invite| {
            let code = qrcode::QrCode::new(invite.url.as_bytes())
                .map_err(|_| "Could not render the invitation QR")?;
            let svg = code
                .render::<qrcode::render::svg::Color>()
                .min_dimensions(280, 280)
                .build();
            use base64::Engine;
            Ok::<_, String>(format!(
                "data:image/svg+xml;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(svg)
            ))
        })
        .transpose()?;
    let document =
        koi_ui::browser_access::settings_page(&status, invitation.as_ref(), qr.as_deref(), message);
    Ok(("text/html; charset=utf-8", document))
}
fn response(status: StatusCode, mime: &'static str, document: String) -> Response<Vec<u8>> {
    let mut response = Response::new(document.into_bytes());
    *response.status_mut() = status;
    for (key,value) in [("content-type",mime),("cache-control","no-store"),("content-security-policy","default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'")] {
        response.headers_mut().insert(tauri::http::header::HeaderName::from_static(key),tauri::http::header::HeaderValue::from_static(value));
    }
    response
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_actions_are_exact_bounded_and_reject_foreign_origins() {
        let request = |method, path| {
            Request::builder()
                .method(method)
                .uri(format!("koi-ui://localhost{path}"))
                .body(vec![])
                .unwrap()
        };
        assert!(allowed(&request("GET", "/web")));
        assert!(allowed(&request("POST", "/web/settings")));
        for (method, path) in [
            ("GET", "/web/settings"),
            ("POST", "/web/status"),
            ("GET", "/web?token=x"),
            ("POST", "/web/revoke/../all"),
        ] {
            assert!(!allowed(&request(method, path)));
        }
        let mut foreign = request("POST", "/web/open");
        foreign
            .headers_mut()
            .insert("origin", "https://evil.example".parse().unwrap());
        assert!(!allowed(&foreign));
    }
}
