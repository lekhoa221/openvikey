use std::sync::{Arc, Mutex};

use openvikey_core::types::InputMethod;
use openvikey_win::focus::FocusCache;
use openvikey_win::host::{
    TypingHost, bind_runtime, clear_all_rules_runtime, control_snapshot, forget_rule_runtime,
};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{IsWindow, IsWindowVisible, SendMessageW, WM_CLOSE};

#[test]
fn learned_rules_runtime_management() {
    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    let focus = Arc::new(FocusCache::new());
    bind_runtime(Arc::clone(&host), focus);

    // 1. Manually insert some learned pairs into host session
    {
        let mut guard = host.lock().unwrap();
        guard.session.model_mut().record_personal_correction(
            InputMethod::Telex,
            "ko",
            "không",
            true,
        );
        guard
            .session
            .record_test_personal_learned(InputMethod::Telex, "paht", "phát");
    }

    // 2. Snapshot inspection
    let snap = control_snapshot().unwrap();
    assert!(!snap.learned_rows.is_empty());
    assert!(
        snap.learned_rows
            .iter()
            .any(|r| r.original_nfc == "ko" && r.candidate_nfc == "không")
    );
    assert!(
        snap.learned_rows
            .iter()
            .any(|r| r.original_nfc == "paht" && r.candidate_nfc == "phát")
    );

    // 3. Forget specific rule
    let changed = forget_rule_runtime(InputMethod::Telex, "ko", "không");
    assert!(changed);

    let snap_after_forget = control_snapshot().unwrap();
    assert!(
        !snap_after_forget
            .learned_rows
            .iter()
            .any(|r| r.original_nfc == "ko" && r.candidate_nfc == "không")
    );
    assert!(
        snap_after_forget
            .learned_rows
            .iter()
            .any(|r| r.original_nfc == "paht" && r.candidate_nfc == "phát")
    );

    // 4. Forget last rule
    openvikey_win::host::forget_last_rule_runtime(1_050);
    let snap_after_forget_last = control_snapshot().unwrap();
    assert!(
        !snap_after_forget_last
            .learned_rows
            .iter()
            .any(|r| r.original_nfc == "paht" && r.candidate_nfc == "phát")
    );

    // 5. Clear all rules
    {
        let mut guard = host.lock().unwrap();
        guard.session.model_mut().record_personal_correction(
            InputMethod::Telex,
            "test",
            "thử",
            true,
        );
    }
    clear_all_rules_runtime();
    let snap_empty = control_snapshot().unwrap();
    assert!(snap_empty.learned_rows.is_empty());
}

#[test]
fn learned_rules_page_ui_navigation() {
    openvikey_win::control::init_dpi_awareness();
    openvikey_win::control::show_settings_window(Some(1));

    let hwnd = openvikey_win::control::active_settings_window_handle();
    assert!(!hwnd.is_invalid());
    assert!(unsafe { IsWindow(Some(hwnd)) }.as_bool());
    assert!(unsafe { IsWindowVisible(hwnd) }.as_bool());

    // Send WM_CLOSE to hide cleanly
    unsafe {
        let _ = SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
}
