//! Physical deletion contracts for learned personal data.

use openvikey_core::model::{AdaptiveModel, ModelConfig, RuleContextKey};
use openvikey_core::types::{CandidateSource, FeedbackEvent, FeedbackKind, InputMethod};

fn unique_rule() -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "zz-forget-original".into(),
        candidate_nfc: "zz-forget-candidate".into(),
        left_token_nfc: Some("zz-forget-left".into()),
        source_rule_id: "zz-forget-rule".into(),
    }
}

fn accept_event(seq: u64) -> FeedbackEvent {
    FeedbackEvent {
        seq,
        at_ms: 10,
        kind: FeedbackKind::Accept { candidate_id: 7 },
    }
}

#[test]
fn forget_rule_removes_key_and_metadata_from_serialized_model() {
    let mut model = AdaptiveModel::default();
    let key = unique_rule();
    model.apply_feedback(&key, &accept_event(1), true);
    model.record_auto_emission(&key, 99, 10, true);
    let before = String::from_utf8(model.to_json_payload().unwrap()).unwrap();
    assert!(before.contains("zz-forget-original"), "{before}");
    assert!(before.contains("zz-forget-candidate"), "{before}");
    assert!(before.contains("zz-forget-left"), "{before}");

    assert!(model.forget_rule(&key));
    let after = String::from_utf8(model.to_json_payload().unwrap()).unwrap();
    assert!(!after.contains("zz-forget-original"), "{after}");
    assert!(!after.contains("zz-forget-candidate"), "{after}");
    assert!(!after.contains("zz-forget-left"), "{after}");
    assert!(!after.contains("zz-forget-rule"), "{after}");
    assert!(!model.forget_rule(&key));
}

#[test]
fn forget_personal_pair_removes_probation_and_promoted_rows() {
    let mut model = AdaptiveModel::default();
    for _ in 0..2 {
        model.record_personal_correction(
            InputMethod::Telex,
            "zz-personal-original",
            "zz-personal-candidate",
            true,
        );
    }
    assert_eq!(
        model.personal_correction_count(
            InputMethod::Telex,
            "zz-personal-original",
            "zz-personal-candidate"
        ),
        2
    );

    model.forget_personal_pair(
        InputMethod::Telex,
        "zz-personal-original",
        "zz-personal-candidate",
    );
    let payload = String::from_utf8(model.to_json_payload().unwrap()).unwrap();
    assert!(!payload.contains("zz-personal-original"), "{payload}");
    assert!(!payload.contains("zz-personal-candidate"), "{payload}");
}

#[test]
fn forget_all_learning_data_resets_to_cold_start_hash() {
    let config = ModelConfig {
        half_life_ms: 123,
        ..ModelConfig::default()
    };
    let mut model = AdaptiveModel::new(config);
    model.apply_feedback(&unique_rule(), &accept_event(1), true);
    model.record_personal_correction(
        InputMethod::Telex,
        "zz-personal-original",
        "zz-personal-candidate",
        true,
    );
    assert!(model.forget_all());
    assert_eq!(
        model.to_json_payload().unwrap(),
        AdaptiveModel::default().to_json_payload().unwrap()
    );
    assert!(!model.forget_all());
}
