//! Native Linux service controls for the one installed Koi daemon.
//!
//! Detection is capability-based, matching Koi's installer contract: a live
//! systemd runtime wins, then a complete OpenRC toolset, otherwise controls are
//! unavailable.  Distro names are deliberately not part of the decision.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::ServiceStatus;

const SERVICE_NAME: &str = "koi";
const SYSTEMD_RUNTIME: &str = "/run/systemd/system";
const OPENRC_INIT: &str = "/etc/init.d/koi";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceManager {
    Systemd,
    OpenRc,
    Unavailable,
}

pub(crate) fn status() -> ServiceStatus {
    match detect() {
        ServiceManager::Systemd => systemd_status(),
        ServiceManager::OpenRc => openrc_status(),
        ServiceManager::Unavailable => ServiceStatus {
            installed: false,
            running: false,
            detail: Some(
                "no supported Linux service manager is available (systemd or OpenRC)".into(),
            ),
        },
    }
}

pub(crate) fn start() -> Result<String, String> {
    action("start")
}

pub(crate) fn stop() -> Result<String, String> {
    action("stop")
}

fn detect() -> ServiceManager {
    detect_with(
        Path::new(SYSTEMD_RUNTIME).is_dir(),
        command_path("systemctl", &["/usr/bin/systemctl", "/bin/systemctl"]).is_some(),
        command_path(
            "rc-service",
            &[
                "/sbin/rc-service",
                "/usr/sbin/rc-service",
                "/bin/rc-service",
                "/usr/bin/rc-service",
            ],
        )
        .is_some(),
        command_path(
            "rc-update",
            &[
                "/sbin/rc-update",
                "/usr/sbin/rc-update",
                "/bin/rc-update",
                "/usr/bin/rc-update",
            ],
        )
        .is_some(),
    )
}

fn detect_with(
    systemd_runtime: bool,
    systemctl: bool,
    rc_service: bool,
    rc_update: bool,
) -> ServiceManager {
    if systemd_runtime && systemctl {
        ServiceManager::Systemd
    } else if rc_service && rc_update {
        ServiceManager::OpenRc
    } else {
        ServiceManager::Unavailable
    }
}

fn systemd_status() -> ServiceStatus {
    let Some(systemctl) = command_path("systemctl", &["/usr/bin/systemctl", "/bin/systemctl"])
    else {
        return unavailable_command("systemctl");
    };
    match Command::new(systemctl)
        .args(["show", SERVICE_NAME, "--property=LoadState,ActiveState"])
        .output()
    {
        Ok(output) => parse_systemd_status(&output),
        Err(error) => ServiceStatus {
            installed: false,
            running: false,
            detail: Some(format!("systemctl query failed: {error}")),
        },
    }
}

fn parse_systemd_status(output: &Output) -> ServiceStatus {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let installed = stdout.lines().any(|line| line == "LoadState=loaded");
    let running = stdout.lines().any(|line| line == "ActiveState=active");
    ServiceStatus {
        installed,
        running,
        detail: (!output.status.success()).then(|| output_detail(output)),
    }
}

fn openrc_status() -> ServiceStatus {
    let installed = Path::new(OPENRC_INIT).is_file();
    if !installed {
        return ServiceStatus {
            installed: false,
            running: false,
            detail: None,
        };
    }
    let Some(rc_service) = command_path(
        "rc-service",
        &[
            "/sbin/rc-service",
            "/usr/sbin/rc-service",
            "/bin/rc-service",
            "/usr/bin/rc-service",
        ],
    ) else {
        return unavailable_command("rc-service");
    };
    match Command::new(rc_service)
        .args([SERVICE_NAME, "status"])
        .output()
    {
        Ok(output) => ServiceStatus {
            installed: true,
            running: output.status.success(),
            detail: (!output.status.success()).then(|| output_detail(&output)),
        },
        Err(error) => ServiceStatus {
            installed: true,
            running: false,
            detail: Some(format!("OpenRC status query failed: {error}")),
        },
    }
}

fn action(action: &str) -> Result<String, String> {
    match detect() {
        ServiceManager::Systemd => {
            let systemctl = command_path("systemctl", &["/usr/bin/systemctl", "/bin/systemctl"])
                .ok_or_else(|| "systemctl is not available".to_string())?;
            finish_action(
                "systemd",
                action,
                Command::new(systemctl)
                    .args([action, SERVICE_NAME])
                    .output(),
            )
        }
        ServiceManager::OpenRc => {
            let rc_service = command_path(
                "rc-service",
                &[
                    "/sbin/rc-service",
                    "/usr/sbin/rc-service",
                    "/bin/rc-service",
                    "/usr/bin/rc-service",
                ],
            )
            .ok_or_else(|| "rc-service is not available".to_string())?;
            let output = if let Some(pkexec) = command_path("pkexec", &["/usr/bin/pkexec"]) {
                Command::new(pkexec)
                    .arg(&rc_service)
                    .args([SERVICE_NAME, action])
                    .output()
            } else {
                Command::new(&rc_service)
                    .args([SERVICE_NAME, action])
                    .output()
            };
            finish_action("OpenRC", action, output).map_err(|error| {
                if command_path("pkexec", &["/usr/bin/pkexec"]).is_none() {
                    format!("{error}. Run `doas rc-service {SERVICE_NAME} {action}` in a terminal")
                } else {
                    error
                }
            })
        }
        ServiceManager::Unavailable => {
            Err("service controls require a live systemd or OpenRC installation".into())
        }
    }
}

fn finish_action(
    manager: &str,
    action: &str,
    output: std::io::Result<Output>,
) -> Result<String, String> {
    let output = output.map_err(|error| format!("could not run {manager}: {error}"))?;
    if output.status.success() {
        return Ok(format!("{manager} service {action} completed."));
    }
    let detail = output_detail(&output);
    Err(if detail.is_empty() {
        format!("{manager} refused to {action} Koi")
    } else {
        detail
    })
}

fn unavailable_command(command: &str) -> ServiceStatus {
    ServiceStatus {
        installed: false,
        running: false,
        detail: Some(format!("{command} is not available")),
    }
}

fn output_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        stderr
    }
}

fn command_path(name: &str, fixed: &[&str]) -> Option<PathBuf> {
    fixed
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .or_else(|| {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join(name))
                    .find(|candidate| candidate.is_file())
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_systemd_wins_over_openrc_tools() {
        assert_eq!(detect_with(true, true, true, true), ServiceManager::Systemd);
    }

    #[test]
    fn complete_openrc_toolset_is_selected_without_systemd() {
        assert_eq!(
            detect_with(false, false, true, true),
            ServiceManager::OpenRc
        );
    }

    #[test]
    fn partial_or_stale_manager_facts_are_unavailable() {
        assert_eq!(
            detect_with(true, false, true, false),
            ServiceManager::Unavailable
        );
        assert_eq!(
            detect_with(false, true, true, false),
            ServiceManager::Unavailable
        );
    }
}
