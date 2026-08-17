//! Map executable paths/names to inject profiles.

use crate::inject::InjectProfile;

/// Classify an executable path or file name into an [`InjectProfile`].
#[must_use]
pub fn profile_for_exe(path_or_name: &str) -> InjectProfile {
    const ELECTRON: &[&str] = &[
        "Cursor.exe",
        "Code.exe",
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "Discord.exe",
        "Slack.exe",
        "WhatsApp.exe",
    ];
    let name = std::path::Path::new(path_or_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path_or_name);
    if ELECTRON.iter().any(|e| name.eq_ignore_ascii_case(e)) {
        InjectProfile::Electron
    } else {
        InjectProfile::Win32
    }
}
