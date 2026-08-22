//! Explicit per-user Start-with-Windows registration for the standalone executable.

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "OpenViKey";

#[cfg(windows)]
pub fn enabled() -> bool {
    let Ok(key) = windows_registry::CURRENT_USER.open(RUN_KEY) else {
        return false;
    };
    key.get_string(VALUE_NAME).is_ok()
}

#[cfg(not(windows))]
#[must_use]
pub const fn enabled() -> bool {
    false
}

/// Toggle per-user autostart. This never requires elevation.
#[cfg(windows)]
pub fn set_enabled(enable: bool) -> windows::core::Result<()> {
    let key = windows_registry::CURRENT_USER.create(RUN_KEY)?;
    if enable {
        let executable = std::env::current_exe().map_err(|error| {
            windows::core::Error::new(windows::Win32::Foundation::E_FAIL, error.to_string())
        })?;
        let command = format!("\"{}\" --background", executable.display());
        key.set_string(VALUE_NAME, command)?;
    } else if key.get_value(VALUE_NAME).is_ok() {
        key.remove_value(VALUE_NAME)?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_enabled(_enable: bool) -> windows::core::Result<()> {
    Ok(())
}
