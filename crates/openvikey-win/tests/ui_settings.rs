use std::sync::{Arc, Mutex};

use openvikey_core::types::{InputKind, InputMethod, TonePlacement};
use openvikey_win::focus::FocusCache;
use openvikey_win::host::{
    TypingHost, bind_runtime, control_snapshot, set_allow_terminal_runtime,
    set_input_method_runtime, set_mode_runtime, set_show_suggestions_runtime,
    set_tone_placement_runtime, tray_snapshot,
};
use openvikey_win::policy::{HostState, KeyDecision, Mode, RawKey, decide};
use openvikey_win_context::ContextState;

#[test]
fn runtime_setters_mutate_live_host_snapshot() {
    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    let focus = Arc::new(FocusCache::new());
    bind_runtime(Arc::clone(&host), focus);

    set_mode_runtime(Mode::English, 100);
    let snap1 = control_snapshot().unwrap();
    assert_eq!(snap1.mode, Mode::English);

    set_mode_runtime(Mode::Viet, 200);
    let snap2 = control_snapshot().unwrap();
    assert_eq!(snap2.mode, Mode::Viet);

    set_input_method_runtime(InputMethod::Vni, 300);
    let snap3 = control_snapshot().unwrap();
    assert_eq!(snap3.engine_config.method, InputMethod::Vni);

    set_tone_placement_runtime(TonePlacement::Classic, 400);
    let snap4 = control_snapshot().unwrap();
    assert_eq!(snap4.engine_config.tone_placement, TonePlacement::Classic);

    set_show_suggestions_runtime(false);
    let snap5 = control_snapshot().unwrap();
    assert!(!snap5.show_suggestions);

    set_allow_terminal_runtime(true);
    let snap6 = control_snapshot().unwrap();
    assert!(snap6.allow_terminal);

    // Verify lightweight tray snapshot reads atomic state without blocking
    let tray_snap = tray_snapshot().unwrap();
    assert_eq!(tray_snap.mode, Mode::Viet);
    assert_eq!(tray_snap.method, InputMethod::Vni);
    assert!(!tray_snap.show_suggestions);
    assert!(tray_snap.allow_terminal);
}

#[test]
fn terminal_toggle_modifies_real_hook_policy() {
    let raw_a = RawKey {
        vk: 0x41, // 'a'
        down: true,
        control: false,
        shift: false,
        extra_info: 0,
        left_ctrl: false,
        left_shift: false,
    };

    let mut state = HostState {
        mode: Mode::Viet,
        foreground_exe: "cmd.exe".into(),
        is_sending: false,
        allow_terminal: false,
        caps_lock: false,
        alt: false,
        meta: false,
        context_state: ContextState::Unsupported,
    };

    // Terminal disallowed -> passes raw key
    assert_eq!(decide(&raw_a, &state), KeyDecision::Pass);

    // Terminal allowed -> hook transforms
    state.allow_terminal = true;
    assert!(matches!(
        decide(&raw_a, &state),
        KeyDecision::EatAndInject(InputKind::Key { logical: 'a', .. })
    ));

    // Terminal reverted to false -> hook passes raw key
    state.allow_terminal = false;
    assert_eq!(decide(&raw_a, &state), KeyDecision::Pass);
}

#[test]
fn method_and_tone_change_resets_composition_safely() {
    let mut host = TypingHost::new_telex_fixture();
    let raw_a = RawKey {
        vk: 0x41,
        down: true,
        control: false,
        shift: false,
        extra_info: 0,
        left_ctrl: false,
        left_shift: false,
    };
    let _ = host.handle_key(raw_a, 100);
    assert!(!host.sent.is_empty() || !host.session.composition_text().is_empty());

    // Switch method -> clears composing buffer
    let mut config = host.session.engine_config();
    config.method = InputMethod::Vni;
    host.set_engine_config(config, 200);
    assert!(host.sent.is_empty());
    assert!(host.last_injected_token.is_empty());
    assert!(host.session.composition_text().is_empty());

    // Type letter again
    let _ = host.handle_key(raw_a, 300);
    assert!(!host.sent.is_empty() || !host.session.composition_text().is_empty());

    // Switch tone placement -> clears composing buffer
    let mut config2 = host.session.engine_config();
    config2.tone_placement = TonePlacement::Classic;
    host.set_engine_config(config2, 400);
    assert!(host.sent.is_empty());
    assert!(host.last_injected_token.is_empty());
    assert!(host.session.composition_text().is_empty());
}

#[cfg(windows)]
#[test]
fn settings_window_lifecycle_and_tray_interactions() {
    use openvikey_win::control::{active_settings_window_handle, show_settings_window};
    use openvikey_win::persist::HostShutdown;
    use openvikey_win::tray::{WM_OPEN_SETTINGS, WM_TRAYICON, install_host_ui};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        IsWindow, IsWindowVisible, SendMessageW, WM_CLOSE, WM_LBUTTONDBLCLK,
    };

    let shutdown = Arc::new(HostShutdown::new());
    let _ui = install_host_ui(&shutdown, Mode::Viet).unwrap();

    show_settings_window(Some(0));
    let hwnd = active_settings_window_handle();
    assert!(!hwnd.is_invalid());
    assert!(unsafe { IsWindow(Some(hwnd)) }.as_bool());

    // 1. WM_CLOSE hides window without destroying handle
    unsafe {
        let _ = SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    assert!(unsafe { IsWindow(Some(hwnd)) }.as_bool());
    assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());

    // 2. Tray sink receives WM_OPEN_SETTINGS (single-instance signal) -> restores window
    let sink = unsafe {
        windows::Win32::UI::WindowsAndMessaging::FindWindowW(
            windows::core::w!("OpenViKeyTraySink"),
            windows::core::w!("OpenViKeyTraySink"),
        )
    }
    .unwrap();
    assert!(!sink.is_invalid());

    unsafe {
        let _ = SendMessageW(sink, WM_OPEN_SETTINGS, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    assert!(unsafe { IsWindowVisible(hwnd) }.as_bool());

    // Hide window again
    unsafe {
        let _ = SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());

    // 3. Tray icon double-click (WM_LBUTTONDBLCLK) -> opens settings window
    unsafe {
        let _ = SendMessageW(
            sink,
            WM_TRAYICON,
            Some(WPARAM(1)),
            Some(LPARAM(isize::try_from(WM_LBUTTONDBLCLK).unwrap_or(0))),
        );
    }
    assert!(unsafe { IsWindowVisible(hwnd) }.as_bool());
}
