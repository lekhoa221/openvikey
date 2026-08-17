//! TypingHost (no OS hook) — composition inject + accept/undo visuals.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use openvikey_core::types::InputKind;
use openvikey_win::host::{handle_key_locked, on_try_lock_fail, TypingHost};
use openvikey_win::policy::{HostHotkey, KeyDecision, RawKey};
use openvikey_win::sync::InjectCommand;

fn key(vk: u16) -> RawKey {
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

fn enter_key() -> RawKey {
    key(0x0D)
}

#[test]
fn typed_letter_records_replace() {
    let mut host = TypingHost::new_telex_fixture();
    host.handle_key(key(0x58), 1); // x
    assert!(!host.recorded.is_empty());
    assert!(host.recorded.iter().any(|c| {
        matches!(
            c,
            InjectCommand::Replace { text_nfc, .. } if text_nfc == "x"
        )
    }));
}

#[test]
fn space_commit_updates_last_injected_token() {
    let mut host = TypingHost::new_telex_fixture();
    for vk in [0x58u16, 0x49, 0x4E, 0x20] {
        // x i n space
        host.handle_key(key(vk), 1);
    }
    assert_eq!(host.last_injected_token, "xin");
}

#[test]
fn focus_change_does_not_backspace() {
    let mut host = TypingHost::new_telex_fixture();
    host.handle_key(key(0x41), 1);
    host.recorded.clear();
    host.set_hwnd(99, "Cursor.exe".into(), 2);
    assert!(host.recorded.is_empty());
    assert!(host.last_injected_token.is_empty());
}

#[test]
fn accept_composing_replaces_sent() {
    let mut host = TypingHost::new_telex_fixture();
    host.handle_key(key(0x4B), 1); // k
    host.handle_key(key(0x4F), 2); // o
    host.recorded.clear();
    host.handle_hotkey(HostHotkey::AcceptTop, 3);
    assert_eq!(
        host.recorded,
        vec![
            InjectCommand::Replace {
                backspace_graphemes: 2,
                text_nfc: "không".into(),
            },
            InjectCommand::AppendDelimiter { delimiter: ' ' },
        ]
    );
    assert_eq!(host.last_injected_token, "không");
    assert!(host.sent.is_empty());
}

#[test]
fn accept_after_commit_same_hwnd_replaces_token() {
    let mut host = TypingHost::new_telex_fixture();
    for vk in [0x4Bu16, 0x4F, 0x20] {
        // k o space
        host.handle_key(key(vk), 1);
    }
    assert_eq!(host.last_injected_token, "ko");
    host.recorded.clear();
    host.handle_hotkey(HostHotkey::AcceptTop, 2);
    assert_eq!(
        host.recorded,
        vec![InjectCommand::Replace {
            backspace_graphemes: 2,
            text_nfc: "không".into(),
        }]
    );
    assert_eq!(host.last_injected_token, "không");
}

#[test]
fn accept_after_commit_other_hwnd_is_noop_visual() {
    let mut host = TypingHost::new_telex_fixture();
    for vk in [0x4Bu16, 0x4F, 0x20] {
        host.handle_key(key(vk), 1);
    }
    host.recorded.clear();
    host.set_hwnd(2, "notepad.exe".into(), 3);
    host.handle_hotkey(HostHotkey::AcceptTop, 4);
    assert!(host.recorded.is_empty());
}

#[test]
fn leave_and_return_same_hwnd_does_not_accept_or_undo_without_token() {
    let mut host = TypingHost::new_telex_fixture();
    let original_hwnd = host.hwnd;
    for vk in [0x4Bu16, 0x4F, 0x20] {
        // k o space → commit "ko"
        host.handle_key(key(vk), 1);
    }
    assert_eq!(host.last_injected_token, "ko");
    assert_eq!(host.last_injected_hwnd, original_hwnd);
    let document_after_commit = host.session.document_text();

    host.set_hwnd(2, "Cursor.exe".into(), 2);
    assert!(host.last_injected_token.is_empty());
    host.set_hwnd(original_hwnd, "notepad.exe".into(), 3);
    assert_eq!(host.hwnd, original_hwnd);
    assert!(host.last_injected_token.is_empty());

    host.recorded.clear();
    host.handle_hotkey(HostHotkey::AcceptTop, 4);
    assert!(host.recorded.is_empty());
    assert_eq!(host.session.document_text(), document_after_commit);

    host.handle_hotkey(HostHotkey::UndoLast, 5);
    assert!(host.recorded.is_empty());
    assert_eq!(host.session.document_text(), document_after_commit);
}

#[test]
fn post_commit_accept_and_undo_require_non_empty_token() {
    // HWND still matches last_injected_hwnd, but token was cleared (focus caret-break hole).
    let mut host = TypingHost::new_telex_fixture();
    for vk in [0x4Bu16, 0x4F, 0x20] {
        host.handle_key(key(vk), 1);
    }
    assert_eq!(host.hwnd, host.last_injected_hwnd);
    let document_after_commit = host.session.document_text();
    host.last_injected_token.clear();
    host.recorded.clear();

    host.handle_hotkey(HostHotkey::AcceptTop, 2);
    assert!(
        host.recorded.is_empty(),
        "empty last_injected_token must not inject accept: {:?}",
        host.recorded
    );
    assert_eq!(host.session.document_text(), document_after_commit);

    let mut auto = TypingHost::new_vni_auto_fixture();
    for (i, ch) in ['p', 'a', 'h', 't', '1'].into_iter().enumerate() {
        let vk = if ch == '1' {
            0x31
        } else {
            u16::from(ch.to_ascii_uppercase() as u8)
        };
        auto.handle_key(key(vk), i64::try_from(i).unwrap_or(0));
    }
    auto.handle_key(key(0x20), 10);
    assert_eq!(auto.hwnd, auto.last_injected_hwnd);
    let auto_doc = auto.session.document_text();
    auto.last_injected_token.clear();
    auto.recorded.clear();
    auto.handle_hotkey(HostHotkey::UndoLast, 11);
    assert!(
        auto.recorded.is_empty(),
        "empty last_injected_token must not inject undo: {:?}",
        auto.recorded
    );
    assert_eq!(auto.session.document_text(), auto_doc);
}

#[test]
fn undo_replaces_last_injected() {
    let mut host = TypingHost::new_vni_auto_fixture();
    for (i, ch) in ['p', 'a', 'h', 't', '1'].into_iter().enumerate() {
        let vk = if ch == '1' {
            0x31
        } else {
            u16::from(ch.to_ascii_uppercase() as u8)
        };
        host.handle_key(key(vk), i64::try_from(i).unwrap_or(0));
    }
    host.handle_key(key(0x20), 10);
    let token_before = host.last_injected_token.clone();
    assert_eq!(token_before, "phát");
    host.recorded.clear();
    host.handle_hotkey(HostHotkey::UndoLast, 11);
    assert_eq!(
        host.recorded,
        vec![InjectCommand::Replace {
            backspace_graphemes: openvikey_win::sync::grapheme_len(&token_before),
            text_nfc: "paht1".into(),
        }]
    );
    assert_eq!(host.last_injected_token, "paht1");
}

#[test]
fn try_lock_fail_on_letter_is_pass() {
    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    let held = Arc::clone(&host);
    let blocker = thread::spawn(move || {
        let _guard = held.lock().unwrap();
        thread::sleep(Duration::from_millis(200));
    });
    thread::sleep(Duration::from_millis(20));
    let decision = handle_key_locked(&host, key(0x41), 1);
    assert_eq!(decision, KeyDecision::Pass);
    blocker.join().unwrap();
}

#[test]
fn try_lock_fail_on_enter_eats() {
    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    let held = Arc::clone(&host);
    let blocker = thread::spawn(move || {
        let _guard = held.lock().unwrap();
        thread::sleep(Duration::from_millis(200));
    });
    thread::sleep(Duration::from_millis(20));
    let decision = handle_key_locked(&host, enter_key(), 1);
    assert_eq!(decision, KeyDecision::EatAndIgnore);
    assert_eq!(on_try_lock_fail(&enter_key()), KeyDecision::EatAndIgnore);
    assert_eq!(on_try_lock_fail(&key(0x41)), KeyDecision::Pass);
    blocker.join().unwrap();
}

#[test]
fn enter_runs_after_replace_on_same_stack() {
    let mut host = TypingHost::new_vni_auto_fixture();
    for (i, ch) in ['p', 'a', 'h', 't', '1'].into_iter().enumerate() {
        let vk = if ch == '1' {
            0x31
        } else {
            u16::from(ch.to_ascii_uppercase() as u8)
        };
        host.handle_key(key(vk), i64::try_from(i).unwrap_or(0));
    }
    host.recorded.clear();
    host.stack_trace.clear();
    let decision = host.handle_key(enter_key(), 10);
    assert_eq!(decision, KeyDecision::CommitAndPass { delimiter: '\n' });
    let enter_pos = host
        .stack_trace
        .iter()
        .position(|m| m == "enter")
        .expect("enter marker");
    assert!(
        host.stack_trace[..enter_pos]
            .iter()
            .any(|m| m == "inject"),
        "Replace/inject must run before enter marker on the same stack: {:?}",
        host.stack_trace
    );
    assert!(host.recorded.iter().any(|c| {
        matches!(
            c,
            InjectCommand::Replace { text_nfc, .. } if text_nfc == "phát"
        )
    }));
}

#[test]
fn save_snapshot_via_host_session() {
    let host = TypingHost::new_telex_fixture();
    let snap = host.session.save_snapshot();
    let _ = snap.model.to_json_payload().unwrap();
}

#[test]
fn eat_and_inject_kind_is_forwarded() {
    let mut host = TypingHost::new_telex_fixture();
    let decision = host.handle_key(key(0x58), 1);
    assert_eq!(
        decision,
        KeyDecision::EatAndInject(InputKind::Key {
            logical: 'x',
            physical: None,
        })
    );
}
