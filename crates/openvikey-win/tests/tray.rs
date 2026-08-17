//! Tray icon event → host hotkey mapping.

use openvikey_win::policy::HostHotkey;
use openvikey_win::tray::{tray_hotkey, TrayEvent};

#[test]
fn left_click_toggles_mode() {
    assert_eq!(tray_hotkey(TrayEvent::LeftClick), Some(HostHotkey::ToggleMode));
}
