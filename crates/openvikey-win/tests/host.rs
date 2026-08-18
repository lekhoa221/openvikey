//! TypingHost (no OS hook) — composition inject + accept/undo visuals.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use openvikey_core::types::InputKind;
use openvikey_win::hook::ll_return;
use openvikey_win::host::{TypingHost, handle_key_locked, on_try_lock_fail};
use openvikey_win::policy::{HostHotkey, KeyDecision, OVK_EXTRA, RawKey};
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
fn plain_typing_appends_without_rewriting_visible_prefix() {
    let mut host = TypingHost::new_telex_fixture();
    host.handle_key(key(0x58), 1); // x

    host.recorded.clear();
    host.handle_key(key(0x49), 2); // i
    assert_eq!(
        host.recorded,
        vec![InjectCommand::Replace {
            backspace_graphemes: 0,
            text_nfc: "i".into(),
        }],
        "an unchanged prefix should remain visible instead of being erased and retyped"
    );
}

#[test]
fn ordinary_composition_backspace_deletes_one_visible_grapheme() {
    let mut host = TypingHost::new_telex_fixture();
    for vk in [0x58u16, 0x49, 0x4E] {
        host.handle_key(key(vk), 1);
    }

    host.recorded.clear();
    host.handle_key(key(0x08), 2);
    assert_eq!(
        host.recorded,
        vec![InjectCommand::Replace {
            backspace_graphemes: 1,
            text_nfc: String::new(),
        }]
    );
}

#[test]
fn backspace_after_commit_passes_through_to_the_focused_app() {
    let mut host = TypingHost::new_telex_fixture();
    for vk in [0x58u16, 0x49, 0x4E, 0x20] {
        host.handle_key(key(vk), 1);
    }

    host.recorded.clear();
    let decision = host.handle_key(key(0x08), 2);
    assert_eq!(decision, KeyDecision::Pass);
    assert!(
        host.recorded.is_empty(),
        "outside composition, the physical Backspace should perform the visible deletion"
    );
    assert!(host.last_injected_token.is_empty());
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
fn mode_toggle_clears_composition_and_candidates_without_backspacing() {
    let mut host = TypingHost::new_telex_fixture();
    host.handle_key(key(0x4B), 1); // k
    host.handle_key(key(0x4F), 2); // o
    assert!(!host.sent.is_empty());
    assert!(!host.session.candidate_texts().is_empty());

    host.recorded.clear();
    host.handle_hotkey(HostHotkey::ToggleMode, 3);
    assert_eq!(host.mode, openvikey_win::policy::Mode::English);
    assert!(host.sent.is_empty());
    assert!(host.session.candidate_texts().is_empty());
    assert!(host.recorded.is_empty());
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
fn foreground_generation_breaks_stale_composition_even_for_same_hwnd() {
    let mut host = TypingHost::new_telex_fixture();
    let hwnd = host.hwnd;
    host.sync_focus(hwnd, "notepad.exe".into(), 1, 1);
    host.handle_key(key(0x4B), 2); // k
    host.handle_key(key(0x4F), 3); // o
    assert!(!host.sent.is_empty());

    // Winevent observed an intermediate blocked/unknown target, then returned to this HWND.
    host.sync_focus(hwnd, "notepad.exe".into(), 3, 4);
    assert!(host.sent.is_empty());
    assert!(host.session.candidate_texts().is_empty());
    assert!(host.session.document_text().is_empty());
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
    host.set_hwnd(2, "Cursor.exe".into(), 2);
    assert!(host.last_injected_token.is_empty());
    host.set_hwnd(original_hwnd, "notepad.exe".into(), 3);
    assert_eq!(host.hwnd, original_hwnd);
    assert!(host.last_injected_token.is_empty());
    assert!(host.session.document_text().is_empty());

    host.recorded.clear();
    host.handle_hotkey(HostHotkey::AcceptTop, 4);
    assert!(host.recorded.is_empty());
    assert!(host.session.document_text().is_empty());

    host.handle_hotkey(HostHotkey::UndoLast, 5);
    assert!(host.recorded.is_empty());
    assert!(host.session.document_text().is_empty());
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
    assert_eq!(ll_return(&decision), 1);
    assert_eq!(
        on_try_lock_fail(&KeyDecision::CommitAndPass { delimiter: '\n' }),
        KeyDecision::EatAndIgnore
    );
    assert_eq!(
        on_try_lock_fail(&KeyDecision::EatAndInject(InputKind::Key {
            logical: 'a',
            physical: None,
        })),
        KeyDecision::Pass
    );
    blocker.join().unwrap();
}

fn hold_mutex(host: &Arc<Mutex<TypingHost>>) -> thread::JoinHandle<()> {
    let held = Arc::clone(host);
    let blocker = thread::spawn(move || {
        let _guard = held.lock().unwrap();
        thread::sleep(Duration::from_millis(200));
    });
    thread::sleep(Duration::from_millis(20));
    blocker
}

#[test]
fn try_lock_fail_hotkey_eats() {
    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    let blocker = hold_mutex(&host);
    let mut acc = key(0xBE);
    acc.control = true;
    let decision = handle_key_locked(&host, acc, 1);
    assert_eq!(decision, KeyDecision::EatAndIgnore);
    assert_eq!(ll_return(&decision), 1);
    assert_eq!(
        on_try_lock_fail(&KeyDecision::Hotkey(HostHotkey::AcceptTop)),
        KeyDecision::EatAndIgnore
    );
    blocker.join().unwrap();
}

#[test]
fn try_lock_fail_ovk_extra_passes() {
    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    let blocker = hold_mutex(&host);
    let mut extra_enter = enter_key();
    extra_enter.extra_info = OVK_EXTRA;
    let decision = handle_key_locked(&host, extra_enter, 1);
    assert_eq!(decision, KeyDecision::Pass);
    assert_eq!(ll_return(&decision), 0);
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
        host.stack_trace[..enter_pos].iter().any(|m| m == "inject"),
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

#[test]
fn apply_commands_drives_injector_sender() {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use openvikey_win::inject::{
        InjectError, InjectProfile, InputSender, ProfilingInjector, SynthesizedEvent,
    };

    struct CountingSender {
        events: Arc<AtomicUsize>,
    }
    impl InputSender for CountingSender {
        fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
            self.events.fetch_add(events.len(), AtomicOrdering::SeqCst);
            Ok(u32::try_from(events.len()).expect("batch len fits u32"))
        }
    }

    let events = Arc::new(AtomicUsize::new(0));
    let sending = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut host = TypingHost::new_telex_fixture();
    host.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: CountingSender {
            events: Arc::clone(&events),
        },
        sending: Arc::clone(&sending),
    }));
    host.handle_key(key(0x58), 1); // x → Replace inject
    assert!(events.load(AtomicOrdering::SeqCst) > 0);
    assert!(!sending.load(AtomicOrdering::SeqCst));
}

struct FailSender;
impl openvikey_win::inject::InputSender for FailSender {
    fn send(
        &mut self,
        events: &[openvikey_win::inject::SynthesizedEvent],
    ) -> Result<u32, openvikey_win::inject::InjectError> {
        Err(openvikey_win::inject::InjectError::Partial {
            sent: 0,
            expected: events.len(),
        })
    }
}

fn failing_injector() -> Box<dyn openvikey_win::inject::CommandInjector> {
    Box::new(openvikey_win::inject::ProfilingInjector {
        profile: openvikey_win::inject::InjectProfile::Win32,
        sender: FailSender,
        sending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    })
}

#[test]
fn accept_inject_fail_restores_session() {
    let mut host = TypingHost::new_telex_fixture();
    host.handle_key(key(0x4B), 1); // k
    host.handle_key(key(0x4F), 2); // o
    let before_sent = host.sent.clone();
    let before_token = host.last_injected_token.clone();
    let before_comp = host.session.composition_text();
    let before_doc = host.session.document_text();
    let before_model = host.session.clone_model();
    host.set_injector(failing_injector());
    host.handle_hotkey(HostHotkey::AcceptTop, 3);
    assert_eq!(host.sent, before_sent);
    assert_eq!(host.last_injected_token, before_token);
    assert_eq!(host.session.composition_text(), before_comp);
    assert_eq!(host.session.document_text(), before_doc);
    assert_eq!(host.session.clone_model(), before_model);
}

#[test]
fn undo_inject_fail_restores_session() {
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
    let before_token = host.last_injected_token.clone();
    let before_hwnd = host.last_injected_hwnd;
    let before_doc = host.session.document_text();
    let before_model = host.session.clone_model();
    host.set_injector(failing_injector());
    host.handle_hotkey(HostHotkey::UndoLast, 11);
    assert_eq!(host.last_injected_token, before_token);
    assert_eq!(host.last_injected_hwnd, before_hwnd);
    assert_eq!(host.session.document_text(), before_doc);
    assert_eq!(host.session.clone_model(), before_model);
}
