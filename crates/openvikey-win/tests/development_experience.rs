//! Development-mode smoke: useful lexicon, visible candidate source, and real hotkey flow.

use std::path::Path;
use std::sync::{Arc, Mutex};

use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_core::types::InputKind;
use openvikey_session::session::LabSession;
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

#[test]
fn development_lexicon_has_useful_breadth() {
    let lexicon = development_lexicon();
    assert!(lexicon.entries().len() >= 100);
    for token in ["không", "người", "tiếng", "việt", "terminal", "sửa"] {
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
