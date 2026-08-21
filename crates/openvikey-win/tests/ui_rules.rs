use std::sync::{Arc, Mutex};

use openvikey_core::decision::DecisionState;
use openvikey_core::model::RuleContextKey;
use openvikey_core::types::{CandidateSource, FeedbackEvent, FeedbackKind, InputMethod};
use openvikey_win::focus::FocusCache;
use openvikey_win::host::{
    TypingHost, bind_runtime, control_snapshot, forget_last_rule_runtime, forget_rule_runtime,
    set_mode_runtime,
};
use openvikey_win::persist::{load_open_personal_store, save_open_snapshot};
use openvikey_win::policy::{HostHotkey, Mode, RawKey};
use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows::Win32::UI::Controls::TCM_GETITEMRECT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetDlgItem, IsWindow, IsWindowVisible, SendMessageW, WM_CLOSE,
};

fn learned_key(left: &str) -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "ko".into(),
        candidate_nfc: "không".into(),
        left_token_nfc: Some(left.into()),
        source_rule_id: format!("fuzzy-{left}"),
    }
}

fn accept(seq: u64) -> FeedbackEvent {
    FeedbackEvent {
        seq,
        at_ms: i64::try_from(seq).unwrap_or(i64::MAX),
        kind: FeedbackKind::Accept { candidate_id: seq },
    }
}

fn raw(vk: u16) -> RawKey {
    RawKey {
        vk,
        down: true,
        control: false,
        shift: false,
        extra_info: 0,
        left_ctrl: false,
        left_shift: false,
    }
}

fn test_root(name: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("openvikey-{name}-{}-{unique}", std::process::id()))
}

#[test]
fn learned_rule_commands_are_exact_durable_and_independent_of_mode() {
    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    let focus = Arc::new(FocusCache::new());
    bind_runtime(Arc::clone(&host), focus);
    let first = learned_key("một");
    let second = learned_key("hai");
    {
        let mut guard = host.lock().unwrap();
        guard
            .session
            .model_mut()
            .apply_feedback(&first, &accept(1), true);
        guard
            .session
            .model_mut()
            .apply_feedback(&second, &accept(2), true);
    }

    let selected = control_snapshot()
        .unwrap()
        .learned_rows
        .into_iter()
        .find(|row| row.left_token_nfc.as_deref() == Some("một"))
        .expect("selected row");
    assert!(forget_rule_runtime(&selected));
    let rows = control_snapshot().unwrap().learned_rows;
    assert!(
        !rows
            .iter()
            .any(|row| row.left_token_nfc.as_deref() == Some("một"))
    );
    assert!(
        rows.iter()
            .any(|row| row.left_token_nfc.as_deref() == Some("hai"))
    );

    let root = test_root("ui-rule-durable");
    let model_path = root.join("model.ovkdev.json");
    let capture_path = root.join("capture.ovkdev.json");
    let snapshot = host.lock().unwrap().session.save_snapshot();
    save_open_snapshot(&snapshot, &model_path, &capture_path).unwrap();
    let (model, _) = load_open_personal_store(&model_path, &capture_path).unwrap();
    let reloaded = model.inspection_rows();
    assert!(
        !reloaded
            .iter()
            .any(|row| row.left_token_nfc.as_deref() == Some("một"))
    );
    assert!(
        reloaded
            .iter()
            .any(|row| row.left_token_nfc.as_deref() == Some("hai"))
    );

    // Create a real last-learned rule, then prove UI forget works while mode is English.
    {
        let mut guard = host.lock().unwrap();
        guard.session.clear_document_context();
        let _ = guard.handle_key(raw(0x4B), 10); // k
        let _ = guard.handle_key(raw(0x4F), 11); // o
        guard.handle_hotkey(HostHotkey::AcceptTop, 12);
    }
    set_mode_runtime(Mode::English, 13);
    assert!(forget_last_rule_runtime());
    assert!(!forget_last_rule_runtime());
}

#[test]
fn learned_rules_ui_does_not_render_surrounding_context() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(manifest.join("src/control.rs")).unwrap();
    assert!(!source.contains("Ngữ cảnh trước:"));
    assert!(source.contains("DecisionState::Ignore => \"Observed\""));
    assert!(source.contains("WC_LISTVIEWW"));

    // The learning chart must stay context-free as well: it renders the
    // context-free identity only and never reads left-token strings.
    let chart_source = std::fs::read_to_string(manifest.join("src/chart_view.rs")).unwrap();
    assert!(!chart_source.contains("left_token"));
    assert!(chart_source.contains("Kết luận"));
}

#[test]
fn learned_rules_page_remains_responsive_during_repeated_navigation() {
    let mut fixture = TypingHost::new_telex_fixture();
    for index in 0..3_000 {
        let key = RuleContextKey {
            input_method: InputMethod::Telex,
            source: CandidateSource::Fuzzy,
            original_nfc: format!("input-{index}"),
            candidate_nfc: format!("output-{index}"),
            left_token_nfc: Some(format!("left-{index}")),
            source_rule_id: format!("rule-{index}"),
        };
        fixture
            .session
            .model_mut()
            .record_decision(&key, DecisionState::Suggest, true);
    }
    bind_runtime(Arc::new(Mutex::new(fixture)), Arc::new(FocusCache::new()));

    openvikey_win::control::init_dpi_awareness();
    openvikey_win::control::show_settings_window(Some(0));

    let hwnd = openvikey_win::control::active_settings_window_handle();
    assert!(!hwnd.is_invalid());
    assert!(unsafe { IsWindow(Some(hwnd)) }.as_bool());
    assert!(unsafe { IsWindowVisible(hwnd) }.as_bool());
    let sidebar = unsafe { GetDlgItem(Some(hwnd), 100) }.unwrap();

    let representative_ids = [203, 302, 401, 501, 601, 701];
    let started = std::time::Instant::now();
    for _ in 0..10 {
        for page in 0..6 {
            let mut bounds = RECT::default();
            unsafe {
                let _ = SendMessageW(
                    sidebar,
                    TCM_GETITEMRECT,
                    Some(WPARAM(page)),
                    Some(LPARAM((&raw mut bounds) as isize)),
                );
                let x = i32::midpoint(bounds.left, bounds.right);
                let y = i32::midpoint(bounds.top, bounds.bottom);
                let point = isize::try_from((y << 16) | (x & 0xFFFF)).unwrap_or(0);
                let _ = SendMessageW(sidebar, 0x0201, Some(WPARAM(1)), Some(LPARAM(point)));
                let _ = SendMessageW(sidebar, 0x0202, Some(WPARAM(0)), Some(LPARAM(point)));
                for (index, id) in representative_ids.iter().enumerate() {
                    let control = GetDlgItem(Some(hwnd), *id).unwrap();
                    assert_eq!(IsWindowVisible(control).as_bool(), index == page);
                }
            }
        }
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "repeated page changes must not rebuild thousands of learned-rule rows"
    );

    unsafe {
        let _ = SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
}
