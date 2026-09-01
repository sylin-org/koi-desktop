//! Discovery of the one real Koi daemon installed on this machine.
//!
//! The private breadcrumb is the fast path for same-owner daemons. A system
//! daemon instead hands credentials to the install-time operator over its
//! authenticated Unix socket or Windows named pipe.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

use serde::{Deserialize, Serialize};

const VERSION: u16 = 1;

#[derive(Clone)]
pub struct DaemonAccess {
    pub endpoint: String,
    pub token: String,
}

impl DaemonAccess {
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.endpoint.trim_end_matches('/'), path)
    }

    pub fn port(&self) -> Option<u16> {
        self.endpoint
            .trim_end_matches('/')
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
    }
}

#[derive(Serialize)]
#[serde(tag = "request", rename_all = "snake_case")]
enum Request {
    Access { version: u16 },
}

#[derive(Deserialize)]
struct AccessResponse {
    version: u16,
    endpoint: String,
    token: String,
}

#[derive(Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
enum Response {
    Access(AccessResponse),
    Error { code: String, message: String },
}

pub fn discover() -> Result<DaemonAccess, String> {
    for path in breadcrumb_candidates() {
        if let Some(access) = read_breadcrumb(&path) {
            return Ok(access);
        }
    }

    let request = serde_json::to_string(&Request::Access { version: VERSION })
        .map_err(|error| format!("local-control request: {error}"))?;
    let mut last_error = None;
    for path in control_candidates() {
        match request_at(&path, &request) {
            Ok(access) => return Ok(access),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "no Koi local-control path is available".to_string()))
}

fn read_breadcrumb(path: &Path) -> Option<DaemonAccess> {
    let body = std::fs::read_to_string(path).ok()?;
    let mut lines = body.lines();
    let endpoint = lines.next()?.trim().to_string();
    let token = lines
        .next()?
        .trim()
        .strip_prefix("dat:")?
        .trim()
        .to_string();
    if endpoint.is_empty() || token.is_empty() {
        return None;
    }
    Some(DaemonAccess { endpoint, token })
}

fn request_at(path: &Path, request: &str) -> Result<DaemonAccess, String> {
    #[cfg(unix)]
    let stream = {
        let stream = std::os::unix::net::UnixStream::connect(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let timeout = Some(Duration::from_secs(2));
        stream
            .set_read_timeout(timeout)
            .map_err(|error| format!("local-control read timeout: {error}"))?;
        stream
            .set_write_timeout(timeout)
            .map_err(|error| format!("local-control write timeout: {error}"))?;
        stream
    };

    #[cfg(windows)]
    let stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;

    #[cfg(not(any(unix, windows)))]
    return Err("Koi local control is unsupported on this platform".to_string());

    #[cfg(any(unix, windows))]
    {
        let mut stream = stream;
        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.write_all(b"\n"))
            .and_then(|()| stream.flush())
            .map_err(|error| format!("local-control write: {error}"))?;
        let mut line = String::new();
        BufReader::new(stream)
            .read_line(&mut line)
            .map_err(|error| format!("local-control read: {error}"))?;
        match serde_json::from_str::<Response>(line.trim())
            .map_err(|error| format!("local-control response: {error}"))?
        {
            Response::Access(access)
                if access.version == VERSION
                    && !access.endpoint.is_empty()
                    && !access.token.is_empty() =>
            {
                Ok(DaemonAccess {
                    endpoint: access.endpoint,
                    token: access.token,
                })
            }
            Response::Access(_) => Err("local-control returned invalid credentials".to_string()),
            Response::Error { code, message } => Err(format!("{code}: {message}")),
        }
    }
}

fn breadcrumb_candidates() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let program_data =
            std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
        vec![PathBuf::from(program_data).join("koi").join("koi.endpoint")]
    }
    #[cfg(unix)]
    {
        let mut paths = Vec::new();
        if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
            paths.push(PathBuf::from(runtime).join("koi.endpoint"));
        }
        paths.push(PathBuf::from("/var/run/koi.endpoint"));
        paths
    }
    #[cfg(not(any(unix, windows)))]
    {
        vec![PathBuf::from("koi.endpoint")]
    }
}

fn control_candidates() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![PathBuf::from(r"\\.\pipe\koi")]
    }
    #[cfg(unix)]
    {
        let mut paths = Vec::new();
        if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
            paths.push(PathBuf::from(runtime).join("koi.sock"));
        }
        paths.push(PathBuf::from("/var/run/koi.sock"));
        paths
    }
    #[cfg(not(any(unix, windows)))]
    {
        vec![PathBuf::from("koi.sock")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_request_matches_koi_wire_contract() {
        assert_eq!(
            serde_json::to_string(&Request::Access { version: VERSION }).unwrap(),
            r#"{"request":"access","version":1}"#
        );
    }

    #[test]
    fn endpoint_port_tracks_a_shifted_install() {
        let access = DaemonAccess {
            endpoint: "http://127.0.0.1:5741".to_string(),
            token: "secret".to_string(),
        };
        assert_eq!(access.port(), Some(5741));
        assert_eq!(access.url("/healthz"), "http://127.0.0.1:5741/healthz");
    }
}
