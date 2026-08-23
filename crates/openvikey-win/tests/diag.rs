use std::path::Path;
use std::sync::Mutex;

use openvikey_core::types::InputKind;
use openvikey_win::diag::{
    Anomaly, DiagEvent, DiagLog, KeyClass, PassReason, classify_key, decision_name,
    default_diag_path, env_value_enables, global, inject_anomalies, key_anomalies, pass_reason,
    reset_for_tests,
};
use openvikey_win::host::TypingHost;
use openvikey_win::policy::{HostState, KeyDecision, Mode, OVK_EXTRA, RawKey};
use openvikey_win::settings::AppTransformPolicy;
use openvikey_win_context::ContextState;

#[test]
fn letter_vk_is_classified_as_letter_without_exposing_the_keycode() {
    assert_eq!(classify_key(0x41), KeyClass::Letter);
    assert_eq!(classify_key(0x5A), KeyClass::Letter);
    assert_ne!(format!("{:?}", classify_key(0x41)), "0x41");
}

#[test]
fn classify_key_uses_coarse_classes_only() {
    assert_eq!(classify_key(0x31), KeyClass::Digit);
    assert_eq!(classify_key(0x08), KeyClass::Backspace);
    assert_eq!(classify_key(0x20), KeyClass::Space);
    assert_eq!(classify_key(0x0D), KeyClass::Enter);
    assert_eq!(classify_key(0x09), KeyClass::Other);
}

fn viet() -> HostState {
    HostState {
        mode: Mode::Viet,
        foreground_exe: "Zalo.exe".into(),
        is_sending: false,
        allow_terminal: false,
        app_transform: AppTransformPolicy::Default,
        caps_lock: false,
        alt: false,
        meta: false,
        context_state: ContextState::Normal,
    }
}

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

#[test]
fn unavailable_field_is_named_as_pass_reason() {
    let mut state = viet();
    state.context_state = ContextState::Unavailable;
    assert_eq!(
        pass_reason(&key(0x41), &state, &KeyDecision::Pass),
        Some(PassReason::Unavailable)
    );
}

#[test]
fn pass_reason_follows_policy_order_without_key_identity() {
    let mut own = key(0x41);
    own.extra_info = OVK_EXTRA;
    assert_eq!(
        pass_reason(&own, &viet(), &KeyDecision::Pass),
        Some(PassReason::OwnInject)
    );

    let mut sensitive = viet();
    sensitive.context_state = ContextState::Sensitive;
    assert_eq!(
        pass_reason(&key(0x41), &sensitive, &KeyDecision::Pass),
        Some(PassReason::Sensitive)
    );

    let mut sending = viet();
    sending.is_sending = true;
    assert_eq!(
        pass_reason(&key(0x41), &sending, &KeyDecision::EatAndIgnore),
        Some(PassReason::Sending)
    );

    let mut english = viet();
    english.mode = Mode::English;
    assert_eq!(
        pass_reason(&key(0x41), &english, &KeyDecision::Pass),
        Some(PassReason::English)
    );

    let mut empty = viet();
    empty.foreground_exe.clear();
    assert_eq!(
        pass_reason(&key(0x41), &empty, &KeyDecision::Pass),
        Some(PassReason::EmptyForeground)
    );

    let mut deny = viet();
    deny.foreground_exe = "1Password.exe".into();
    assert_eq!(
        pass_reason(&key(0x41), &deny, &KeyDecision::Pass),
        Some(PassReason::Denylist)
    );

    let mut blocked = viet();
    blocked.app_transform = AppTransformPolicy::Block;
    assert_eq!(
        pass_reason(&key(0x41), &blocked, &KeyDecision::Pass),
        Some(PassReason::AppBlock)
    );

    let mut terminal = viet();
    terminal.foreground_exe = "WindowsTerminal.exe".into();
    assert_eq!(
        pass_reason(&key(0x41), &terminal, &KeyDecision::Pass),
        Some(PassReason::Terminal)
    );
}

fn sample_key_event() -> DiagEvent {
    DiagEvent::Key {
        at_ms: 10,
        exe: "Zalo.exe".into(),
        inject_profile: "win32".into(),
        context: "unavailable".into(),
        decision: "pass".into(),
        pass_reason: Some(PassReason::Unavailable),
        key_class: KeyClass::Letter,
        down: true,
        sending: false,
        hook_us: 120,
        anomalies: vec![],
    }
}

#[test]
fn disabled_log_drops_events() {
    let log = DiagLog::new(8);
    log.try_push(sample_key_event());
    assert!(log.snapshot().is_empty());
}

#[test]
fn enabled_log_keeps_events() {
    let log = DiagLog::new(8);
    log.set_enabled(true);
    log.try_push(sample_key_event());
    let events = log.snapshot();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        DiagEvent::Key { exe, context, .. } if exe == "Zalo.exe" && context == "unavailable"
    ));
}

#[test]
fn ring_buffer_drops_oldest_when_full() {
    let log = DiagLog::new(2);
    log.set_enabled(true);
    for at_ms in [1, 2, 3] {
        let mut event = sample_key_event();
        match &mut event {
            DiagEvent::Key { at_ms: slot, .. } => *slot = at_ms,
            _ => unreachable!("fixture is a key event"),
        }
        log.try_push(event);
    }
    let events = log.snapshot();
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], DiagEvent::Key { at_ms: 2, .. }));
    assert!(matches!(&events[1], DiagEvent::Key { at_ms: 3, .. }));
}

#[test]
fn jsonl_omits_keystroke_and_composition_payloads() {
    let log = DiagLog::new(8);
    log.set_enabled(true);
    log.try_push(sample_key_event());
    let jsonl = log.to_jsonl();
    for forbidden in [
        "vk",
        "text_nfc",
        "composition",
        "logical",
        "original_nfc",
        "replacement",
    ] {
        assert!(
            !jsonl.contains(forbidden),
            "jsonl must not contain {forbidden}: {jsonl}"
        );
    }
    assert!(jsonl.contains("\"type\":\"key\""));
    assert!(jsonl.contains("\"key_class\":\"letter\""));
}

#[test]
fn key_anomalies_flag_slow_hook_unavailable_and_swallowed_keys() {
    let flagged = key_anomalies(15_001, "eat_and_ignore", true, "unavailable", true, false);
    assert!(flagged.contains(&Anomaly::HookSlow));
    assert!(flagged.contains(&Anomaly::EatAndIgnoreWhileSending));
    assert!(flagged.contains(&Anomaly::UnavailableWhileViet));
    assert!(!flagged.contains(&Anomaly::TryLockFail));
}

#[test]
fn inject_anomalies_flag_duration_and_partial() {
    assert!(inject_anomalies(5_001, false).contains(&Anomaly::InjectSlow));
    assert!(inject_anomalies(1_000_001, false).contains(&Anomaly::InjectVerySlow));
    assert!(inject_anomalies(10, true).contains(&Anomaly::InjectPartial));
    assert!(inject_anomalies(10, false).is_empty());
}

#[test]
fn env_value_enables_only_explicit_on_flags() {
    assert!(env_value_enables(Some("1")));
    assert!(env_value_enables(Some("true")));
    assert!(env_value_enables(Some("TRUE")));
    assert!(!env_value_enables(Some("0")));
    assert!(!env_value_enables(Some("")));
    assert!(!env_value_enables(None));
}

#[test]
fn default_diag_path_lives_beside_other_host_files() {
    let path = default_diag_path(Some(Path::new(r"C:\Users\x\AppData\Local")));
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("diag.jsonl")
    );
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    assert_eq!(parent, Some("OpenViKey"));
}

#[test]
fn flush_to_writes_jsonl_lines() {
    let dir = std::env::temp_dir().join(format!("openvikey-diag-{}", std::process::id()));
    let path = dir.join("nested").join("diag.jsonl");
    let log = DiagLog::new(8);
    log.set_enabled(true);
    log.try_push(sample_key_event());
    let written = log.flush_to(&path).expect("flush");
    assert_eq!(written, 1);
    let body = std::fs::read_to_string(&path).expect("read dump");
    assert!(body.contains("\"type\":\"key\""));
    assert!(!body.contains("text_nfc"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn decision_name_hides_injected_character() {
    let decision = KeyDecision::EatAndInject(InputKind::Key {
        logical: 'a',
        physical: None,
    });
    assert_eq!(decision_name(&decision), "eat_and_inject");
    assert!(!format!("{decision:?}").eq(decision_name(&decision)));
}

static GLOBAL_DIAG: Mutex<()> = Mutex::new(());

#[test]
fn typing_host_records_key_and_inject_without_composed_text() {
    let _lock = GLOBAL_DIAG.lock().expect("diag test lock");
    reset_for_tests();
    global().set_enabled(true);
    let mut host = TypingHost::new_telex_fixture();
    host.foreground_exe = "Zalo.exe".into();
    host.handle_key(key(0x41), 1);
    let events = global().snapshot();
    let jsonl = global().to_jsonl();
    reset_for_tests();

    assert!(
        events.iter().any(|event| matches!(
            event,
            DiagEvent::Key {
                exe,
                key_class: KeyClass::Letter,
                decision,
                ..
            } if exe == "Zalo.exe" && decision == "eat_and_inject"
        )),
        "{events:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            DiagEvent::Inject {
                exe,
                utf16_units,
                ..
            } if exe == "Zalo.exe" && *utf16_units >= 1
        )),
        "{events:?}"
    );
    assert!(!jsonl.contains("text_nfc"));
    assert!(!jsonl.contains("\"a\""));
}

#[test]
fn typing_host_does_not_record_when_diag_is_off() {
    let _lock = GLOBAL_DIAG.lock().expect("diag test lock");
    reset_for_tests();
    let mut host = TypingHost::new_telex_fixture();
    host.handle_key(key(0x41), 1);
    assert!(global().snapshot().is_empty());
}

#[test]
fn diag_source_has_no_println() {
    let src = std::fs::read_to_string("src/diag.rs").expect("diag.rs");
    assert!(!src.contains("println!"));
    assert!(!src.contains("eprintln!"));
}
