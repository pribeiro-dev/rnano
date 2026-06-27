//! OS clipboard integration via subprocess (#30).
//!
//! Uses platform-native tools rather than a new crate dependency:
//! - Linux: `xclip -selection clipboard` or `xsel --clipboard`
//! - macOS: `pbcopy` / `pbpaste`
//! - Windows: `clip.exe` (write) / `powershell Get-Clipboard` (read)
//!
//! All functions return `None` when the clipboard tool is unavailable — the
//! caller falls back to the internal kill ring in that case.

use std::io::Write;
use std::process::{Command, Stdio};

/// Write `text` to the OS clipboard.  Returns `true` on success.
pub fn write(text: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Ok(mut child) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            return child.wait().map(|s| s.success()).unwrap_or(false);
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(mut child) = Command::new("clip").stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            return child.wait().map(|s| s.success()).unwrap_or(false);
        }
        return false;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Try xclip first, then xsel.
        for (cmd, args) in &[
            ("xclip", &["-selection", "clipboard"] as &[&str]),
            ("xsel", &["--clipboard", "--input"]),
        ] {
            if let Ok(mut child) = Command::new(cmd).args(*args).stdin(Stdio::piped()).spawn() {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                if child.wait().map(|s| s.success()).unwrap_or(false) {
                    return true;
                }
            }
        }
        false
    }
}

/// Read the current OS clipboard contents.  Returns `None` if unavailable.
pub fn read() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("pbpaste").output().ok()?;
        if out.status.success() {
            return Some(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        return None;
    }

    #[cfg(target_os = "windows")]
    {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", "Get-Clipboard"])
            .output()
            .ok()?;
        if out.status.success() {
            return Some(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        return None;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        for (cmd, args) in &[
            ("xclip", &["-selection", "clipboard", "-o"] as &[&str]),
            ("xsel", &["--clipboard", "--output"]),
        ] {
            if let Ok(out) = Command::new(cmd).args(*args).output() {
                if out.status.success() {
                    return Some(String::from_utf8_lossy(&out.stdout).into_owned());
                }
            }
        }
        None
    }
}
