//! M9 tracer: the lab must exercise Engine → snapshot → generators → correction.

use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::types::{CandidateSource, InputContext, InputMethod, TonePlacement};
use openvikey_lab::session::LabSession;

fn lexicon() -> Lexicon {
    Lexicon::from_entries(
        [
            LexiconEntry {
                token_nfc: "phát".to_string(),
                frequency: 10,
            },
            LexiconEntry {
                token_nfc: "bàn".to_string(),
                frequency: 100,
            },
            LexiconEntry {
                token_nfc: "bạn".to_string(),
                frequency: 1,
            },
        ],
        [],
        Some("session-live-test"),
    )
}

#[test]
fn live_vni_keys_flow_through_engine_before_fuzzy_generation() {
    let mut session = LabSession::new(
        EngineConfig {
            method: InputMethod::Vni,
            tone_placement: TonePlacement::Modern,
        },
        lexicon(),
    );

    let observations = session.type_text("paht1", InputContext::default(), 0);
    let last = observations.last().expect("one observation per key");

    assert_eq!(last.snapshot.raw_keys, "paht1");
    assert_eq!(last.candidates[0].text, "phát");
    assert_eq!(last.candidates[0].source, CandidateSource::Fuzzy);
    assert!(last.candidates[0].final_score >= 0.9);
    assert!(last.action.is_some());
}

#[test]
fn committed_engine_token_becomes_left_context_for_next_word() {
    let contextual = Lexicon::from_entries(
        [
            LexiconEntry {
                token_nfc: "xin".to_string(),
                frequency: 1,
            },
            LexiconEntry {
                token_nfc: "bàn".to_string(),
                frequency: 1,
            },
            LexiconEntry {
                token_nfc: "bạn".to_string(),
                frequency: 1,
            },
        ],
        [(("xin".to_string(), "bạn".to_string()), 1.0)],
        Some("session-context-test"),
    );
    let mut session = LabSession::new(EngineConfig::default(), contextual);

    let observations = session.type_text("xin ban", InputContext::default(), 0);
    let last = observations.last().unwrap();

    assert_eq!(last.snapshot.raw_keys, "ban");
    assert_eq!(last.candidates[0].text, "bạn");
}

#[test]
fn sensitive_context_disables_engine_transform_and_correction() {
    let mut session = LabSession::new(
        EngineConfig {
            method: InputMethod::Vni,
            tone_placement: TonePlacement::Modern,
        },
        lexicon(),
    );
    let before = session.model_payload().expect("model serializes");
    let observations = session.type_text(
        "paht1",
        InputContext {
            allow_transform: false,
            allow_learning: false,
        },
        0,
    );

    assert!(
        observations
            .iter()
            .all(|observation| observation.candidates.is_empty())
    );
    assert_eq!(session.model_payload().unwrap(), before);
}
