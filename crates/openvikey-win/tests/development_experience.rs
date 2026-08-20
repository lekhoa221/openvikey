//! Development-mode smoke: useful lexicon, visible candidate source, and real hotkey flow.

#![allow(clippy::float_cmp)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_core::model::{ModelView, RuleContextKey};
use openvikey_core::types::{CandidateSource, InputKind, InputMethod, TonePlacement};
use openvikey_session::session::LabSession;
use openvikey_win::focus::canonical_foreground_exe;
use openvikey_win::hook::dispatch_ll;
use openvikey_win::host::TypingHost;
use openvikey_win::inject::{
    InjectError, InjectProfile, InputSender, ProfilingInjector, SynthesizedEvent,
};
use openvikey_win::policy::{HostHotkey, KeyDecision, RawKey};
use openvikey_win::sync::InjectCommand;

fn development_lexicon() -> Lexicon {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .parent()
        .expect("workspace");
    let bytes = std::fs::read(root.join("data/fixtures/lexicon/development.json"))
        .expect("development lexicon");
    let artifact: LexiconArtifact = serde_json::from_slice(&bytes).expect("valid lexicon artifact");
    Lexicon::from_artifact(artifact)
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

fn type_ascii(host: &mut TypingHost, text: &str) {
    for (index, byte) in text.bytes().enumerate() {
        let vk = if byte.is_ascii_alphabetic() {
            u16::from(byte.to_ascii_uppercase())
        } else {
            u16::from(byte)
        };
        host.handle_key(key(vk), i64::try_from(index).expect("short smoke input"));
    }
}

fn ko_abbrev_rule() -> RuleContextKey {
    ko_abbrev_rule_for(InputMethod::Telex)
}

fn ko_abbrev_rule_for(input_method: InputMethod) -> RuleContextKey {
    RuleContextKey {
        input_method,
        source: CandidateSource::Abbreviation,
        original_nfc: "ko".to_string(),
        candidate_nfc: "không".to_string(),
        left_token_nfc: None,
        source_rule_id: "seed:ko".to_string(),
    }
}

fn vni_engine() -> EngineConfig {
    EngineConfig {
        method: InputMethod::Vni,
        tone_placement: TonePlacement::Modern,
    }
}

#[test]
fn development_lexicon_has_useful_breadth() {
    let lexicon = development_lexicon();
    assert!(lexicon.entries().len() >= 100);
    for token in [
        "không", "người", "tiếng", "việt", "terminal", "sửa", "đó", "chào",
    ] {
        assert!(
            lexicon.contains(token),
            "missing development token: {token}"
        );
    }
}

#[test]
fn ko_then_ctrl_period_replaces_with_khong_through_ll_and_injector() {
    struct RecordingSender {
        batches: Arc<Mutex<Vec<Vec<SynthesizedEvent>>>>,
    }
    impl InputSender for RecordingSender {
        fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
            self.batches.lock().expect("batches").push(events.to_vec());
            Ok(u32::try_from(events.len()).expect("small smoke batch"))
        }
    }

    let batches = Arc::new(Mutex::new(Vec::new()));
    let session = LabSession::new(EngineConfig::default(), development_lexicon());
    let mut typing = TypingHost::new_with_session(session);
    typing.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: RecordingSender {
            batches: Arc::clone(&batches),
        },
        sending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }));
    let host = Mutex::new(typing);

    assert_eq!(dispatch_ll(&host, key(0x4B), 1), 1); // k
    assert_eq!(dispatch_ll(&host, key(0x4F), 2), 1); // o
    assert_eq!(
        host.lock()
            .expect("host")
            .session
            .top_suggestion()
            .as_deref(),
        Some("không")
    );

    batches.lock().expect("batches").clear();
    host.lock().expect("host").recorded.clear();
    let mut accept = key(0xBE);
    accept.control = true;
    assert_eq!(dispatch_ll(&host, accept, 10), 1);

    let guard = host.lock().expect("host");
    assert_eq!(
        guard.recorded,
        vec![
            InjectCommand::Replace {
                backspace_graphemes: 2,
                text_nfc: "không".into(),
            },
            InjectCommand::AppendDelimiter { delimiter: ' ' },
        ]
    );
    assert!(
        guard.session.candidate_texts().is_empty(),
        "accepted suggestions must hide the overlay"
    );
    drop(guard);

    let visible_events: Vec<_> = batches
        .lock()
        .expect("batches")
        .iter()
        .flatten()
        .copied()
        .collect();
    let mut expected = vec![SynthesizedEvent::Backspace, SynthesizedEvent::Backspace];
    expected.extend("không".encode_utf16().map(SynthesizedEvent::Utf16));
    expected.push(SynthesizedEvent::Utf16(u16::from(b' ')));
    assert_eq!(visible_events, expected);
}

#[test]
fn opted_in_terminal_transforms_without_learning_or_capture() {
    let session = LabSession::new(EngineConfig::default(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    host.allow_terminal = true;
    host.set_hwnd(10, "WindowsTerminal.exe".into(), 1);
    let before = host.session.save_snapshot();

    type_ascii(&mut host, "ko");
    let mut accept = key(0xBE);
    accept.control = true;
    assert_eq!(
        host.handle_key(accept, 10),
        KeyDecision::Hotkey(HostHotkey::AcceptTop),
        "terminal should still transform"
    );

    let after = host.session.save_snapshot();
    assert_eq!(after.model, before.model);
    assert_eq!(after.cursors, before.cursors);
    assert_eq!(after.last_at_ms, before.last_at_ms);
    assert!(after.capture_records.is_empty());
}

#[test]
fn electron_browser_learns_on_accept() {
    let session = LabSession::new(EngineConfig::default(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    host.set_hwnd(10, "chrome.exe".into(), 1);
    let before = host.session.save_snapshot();

    type_ascii(&mut host, "ko");
    let mut accept = key(0xBE);
    accept.control = true;
    assert_eq!(
        host.handle_key(accept, 10),
        KeyDecision::Hotkey(HostHotkey::AcceptTop)
    );

    let after = host.session.save_snapshot();
    assert_ne!(after.model, before.model);
    assert!(!after.capture_records.is_empty());
}

#[test]
fn hosted_terminal_provider_exe_still_transforms_without_capture() {
    let session = LabSession::new(EngineConfig::default(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    host.allow_terminal = true;
    let canonical = canonical_foreground_exe("pi.exe", "CASCADIA_HOSTING_WINDOW_CLASS");
    host.set_hwnd(10, canonical, 1);
    let before = host.session.save_snapshot();

    type_ascii(&mut host, "secretmarker");
    host.handle_key(key(0x20), 20);

    let after = host.session.save_snapshot();
    assert_eq!(after.model, before.model);
    assert_eq!(after.capture_records, before.capture_records);
}

#[test]
fn terminal_context_cannot_leak_into_later_learning() {
    let session = LabSession::new(EngineConfig::default(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    host.set_hwnd(10, "WindowsTerminal.exe".into(), 1);
    host.allow_terminal = true;
    type_ascii(&mut host, "secretmarker");
    host.handle_key(key(0x20), 20);

    host.set_hwnd(20, "notepad.exe".into(), 21);
    assert!(host.session.document_text().is_empty());
    type_ascii(&mut host, "ko");
    let mut accept = key(0xBE);
    accept.control = true;
    host.handle_key(accept, 30);

    let payload = String::from_utf8(host.session.model_payload().expect("model payload"))
        .expect("model JSON");
    assert!(!payload.contains("secretmarker"));
}

#[test]
fn vni_misplaced_tone_auto_replaces_and_learns_on_space() {
    let session = LabSession::new(vni_engine(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    type_ascii(&mut host, "ch2ao");
    host.recorded.clear();
    host.handle_key(key(0x20), 10);
    assert!(
        host.recorded.iter().any(|cmd| matches!(
            cmd,
            InjectCommand::Replace { text_nfc, .. } if text_nfc == "chào"
        )),
        "VNI ch2ao + space must inject chào; got {:?}",
        host.recorded
    );
    assert_eq!(host.last_injected_token, "chào");
    let snap = host.session.save_snapshot();
    assert!(
        snap.cursors.next_edit_id > 1,
        "VNI misplaced-tone Auto must consume an edit id; recorded={:?}",
        host.recorded
    );
    let payload = String::from_utf8(host.session.model_payload().expect("model")).expect("utf8");
    assert!(
        payload.contains("\"original_nfc\":\"ch2ao\"")
            && payload.contains("\"candidate_nfc\":\"chào\"")
            && payload.contains("\"input_method\":\"Vni\"")
            && payload.contains("recent_auto"),
        "VNI Auto must record a TelexFix emission; payload={payload}"
    );
}

#[test]
fn vni_tone_before_vowel_do_auto_replaces_on_space() {
    let session = LabSession::new(vni_engine(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    type_ascii(&mut host, "d91o");
    host.recorded.clear();
    host.handle_key(key(0x20), 10);
    assert_eq!(host.last_injected_token, "đó");
    assert!(
        host.session.save_snapshot().cursors.next_edit_id > 1,
        "d91o + space must Auto-replace; recorded={:?}",
        host.recorded
    );
}

#[test]
fn explicit_accept_emits_a_visible_learning_notice() {
    let session = LabSession::new(vni_engine(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    type_ascii(&mut host, "ko");

    host.handle_hotkey(HostHotkey::AcceptTop, 10);

    let notice = host
        .session
        .take_learning_notice()
        .expect("learning notice");
    assert_eq!(notice.original_nfc, "ko");
    assert_eq!(notice.replacement_nfc, "không");
    assert_eq!(notice.positive_delta, 1.0);
    assert_eq!(notice.negative_delta, 0.0);
    assert_eq!(notice.positive_total, 1.0);
    assert_eq!(notice.negative_total, 0.0);
    assert!(notice.display_text().starts_with("Đã học:"));
}

#[test]
fn vni_accept_top_learns_abbreviation_mass() {
    let session = LabSession::new(vni_engine(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    type_ascii(&mut host, "ko");
    host.handle_hotkey(HostHotkey::AcceptTop, 10);
    assert_eq!(
        host.session
            .model()
            .positive_mass(&ko_abbrev_rule_for(InputMethod::Vni), 10),
        1.0
    );
    assert!(
        host.recorded.iter().any(|cmd| matches!(
            cmd,
            InjectCommand::Replace { text_nfc, .. } if text_nfc == "không"
        )),
        "VNI Ctrl+. must still replace; got {:?}",
        host.recorded
    );
}

#[test]
fn common_typo_produces_a_correction_candidate() {
    let session = LabSession::new(EngineConfig::default(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    type_ascii(&mut host, "khogn");
    assert!(
        host.session
            .candidate_texts()
            .iter()
            .any(|candidate| candidate == "không"),
        "khogn should suggest không; got {:?}",
        host.session.candidate_texts()
    );
    assert!(matches!(
        host.handle_key(key(0x08), 20),
        KeyDecision::EatAndInject(InputKind::Backspace)
    ));
}

#[test]
fn terminal_forget_does_not_clear_notepad_learned_mass() {
    let session = LabSession::new(EngineConfig::default(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    host.set_hwnd(10, "notepad.exe".into(), 1);
    type_ascii(&mut host, "ko");
    host.handle_hotkey(HostHotkey::AcceptTop, 10);
    let rule = ko_abbrev_rule();
    assert_eq!(host.session.model().positive_mass(&rule, 10), 1.0);

    host.allow_terminal = true;
    host.set_hwnd(20, "WindowsTerminal.exe".into(), 20);
    type_ascii(&mut host, "ko");
    host.handle_hotkey(HostHotkey::AcceptTop, 30);
    host.handle_hotkey(HostHotkey::ForgetLastRule, 40);

    assert_eq!(
        host.session.model().positive_mass(&rule, 10),
        1.0,
        "ForgetLastRule on a no-learning surface must not drop mass learned in Notepad"
    );
}

#[test]
fn terminal_accept_does_not_retarget_forget_after_returning_to_win32() {
    let session = LabSession::new(EngineConfig::default(), development_lexicon());
    let mut host = TypingHost::new_with_session(session);
    host.set_hwnd(10, "notepad.exe".into(), 1);
    type_ascii(&mut host, "ko");
    host.handle_hotkey(HostHotkey::AcceptTop, 10);
    let ko = ko_abbrev_rule();
    assert_eq!(host.session.model().positive_mass(&ko, 10), 1.0);

    host.allow_terminal = true;
    host.set_hwnd(20, "WindowsTerminal.exe".into(), 20);
    type_ascii(&mut host, "ntn");
    host.handle_hotkey(HostHotkey::AcceptTop, 30);

    host.set_hwnd(10, "notepad.exe".into(), 40);
    host.handle_hotkey(HostHotkey::ForgetLastRule, 50);

    assert_eq!(
        host.session.model().positive_mass(&ko, 10),
        0.0,
        "returning to Win32 must still forget the last real learn, not a phantom terminal Accept"
    );
}
