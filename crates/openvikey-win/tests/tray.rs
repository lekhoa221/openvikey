//! Tray icon event → host hotkey mapping.

use openvikey_win::persist::HostShutdown;
use openvikey_win::policy::HostHotkey;
use openvikey_win::tray::{apply_tray_event, tray_hotkey, TrayEvent};

#[test]
fn left_click_toggles_mode() {
    assert_eq!(tray_hotkey(TrayEvent::LeftClick), Some(HostHotkey::ToggleMode));
}

#[test]
fn tray_exit_requests_host_shutdown() {
    let shutdown = HostShutdown::new();
    assert!(apply_tray_event(TrayEvent::Exit, &shutdown).is_none());
    assert!(shutdown.is_requested());
}
