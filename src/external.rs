//! Open an explicit validated HTTP(S) destination through the native association.
//! Never pass network-supplied text through a command interpreter.
use koi_ui::home::BrowserDestination;

pub fn open(value: &str) -> Result<(), String> {
    open_with(value, launch)
}

fn open_with(value: &str, launcher: impl FnOnce(&str) -> Result<(), String>) -> Result<(), String> {
    let destination = BrowserDestination::parse(value).ok_or(
        "only explicit http(s) URLs without credentials or ambiguous syntax can be opened",
    )?;
    launcher(destination.as_str())
}

#[cfg(windows)]
fn launch(url: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let target: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the validated URL has no NUL; target is terminated and lives across
    // the synchronous call. No command parameters, custom verb or executable.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            windows_sys::core::w!("open"),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(format!(
            "native browser association rejected the URL (code {result})"
        ))
    }
}

#[cfg(not(windows))]
fn launch(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(target_os = "macos"))]
    let program = "xdg-open";
    // One argument, no shell expansion. The browser may intentionally outlive Koi;
    // reap its launcher without waiting for the external application to close.
    let mut child = std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map_err(|error| format!("could not start the native browser launcher: {error}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_never_reaches_the_native_launcher() {
        for value in [
            "javascript:alert(1)",
            "file:///tmp/file",
            "custom://host/",
            "http:host/path",
            "http://user:password@host/",
            "http://@host/",
            "http://host/\n",
            "http://host/\0",
            "http://host\\@other/",
            "http://host:0/",
            "--help",
        ] {
            let result = open_with(value, |_| {
                panic!("unsafe URL reached native launcher: {value:?}")
            });
            assert!(result.is_err(), "{value:?}");
        }
    }

    #[test]
    fn a_url_is_one_canonical_target_not_a_command_line() {
        let input = "https://notes.local:8443/?q=one&literal=%22&var=%25PATH%25&pipe=%7C";
        let mut calls = Vec::new();
        open_with(input, |target| {
            calls.push(target.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, [input]);
        open_with("HTTPS://NOTES.local/", |target| {
            assert_eq!(target, "https://notes.local/");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn launch_failure_is_returned_instead_of_reporting_success() {
        let error = open_with("https://notes.local/", |_| {
            Err("no browser association".into())
        });
        assert_eq!(error, Err("no browser association".into()));
    }
}
