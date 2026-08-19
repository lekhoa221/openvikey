//! Tray icon event → host hotkey mapping.

use openvikey_win::persist::HostShutdown;
use openvikey_win::policy::HostHotkey;
use openvikey_win::tray::{TrayEvent, apply_tray_event, tray_hotkey};

#[test]
fn left_click_toggles_mode() {
    assert_eq!(
        tray_hotkey(TrayEvent::LeftClick),
        Some(HostHotkey::ToggleMode)
    );
}

#[test]
fn tray_exit_requests_host_shutdown() {
    let shutdown = HostShutdown::new();
    assert!(apply_tray_event(TrayEvent::Exit, &shutdown).is_none());
    assert!(shutdown.is_requested());
}

#[cfg(windows)]
#[test]
fn installed_product_ui_has_a_discoverable_single_instance_sink() {
    use openvikey_win::policy::Mode;
    use openvikey_win::tray::install_host_ui;
    use std::sync::Arc;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::core::{PCWSTR, w};

    let shutdown = Arc::new(HostShutdown::new());
    let _ui = install_host_ui(&shutdown, Mode::Viet).unwrap();
    let sink = unsafe { FindWindowW(w!("OpenViKeyTraySink"), PCWSTR::null()) }.unwrap();

    assert!(!sink.is_invalid());
}

#[test]
fn tray_toggle_suggestions_maps_to_hotkey_without_exiting() {
    let shutdown = HostShutdown::new();
    assert_eq!(
        tray_hotkey(TrayEvent::ToggleSuggestions),
        Some(HostHotkey::ToggleSuggestions)
    );
    assert_eq!(
        apply_tray_event(TrayEvent::ToggleSuggestions, &shutdown),
        Some(HostHotkey::ToggleSuggestions)
    );
    assert!(!shutdown.is_requested());
}
