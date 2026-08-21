//! Physical deletion contracts for learned personal data.

use openvikey_core::correction_memory::PersonalTransaction;
use openvikey_core::learning_config::LearningConfigV2;
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

fn named_rule(name: &str) -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: format!("original-{name}"),
        candidate_nfc: format!("candidate-{name}"),
        left_token_nfc: None,
        source_rule_id: format!("rule-{name}"),
    }
}

fn feedback(seq: u64, at_ms: i64, kind: FeedbackKind) -> FeedbackEvent {
    FeedbackEvent { seq, at_ms, kind }
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
    for anchor in 1..=2 {
        model.record_personal_correction(
            InputMethod::Telex,
            "zz-personal-original",
            "zz-personal-candidate",
            PersonalTransaction { anchor, at_ms: 0 },
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
        PersonalTransaction {
            anchor: 1,
            at_ms: 0,
        },
        true,
    );
    assert!(model.forget_all());
    assert_eq!(
        model.to_json_payload().unwrap(),
        AdaptiveModel::default().to_json_payload().unwrap()
    );
    assert!(!model.forget_all());
}

#[test]
fn v1_model_uses_the_versioned_correction_limit() {
    assert_eq!(
        ModelConfig::default().max_rules,
        LearningConfigV2::compatibility_v1().max_corrections
    );
}

#[test]
fn exceeding_max_rules_evicts_oldest_ignore_before_strong_rows() {
    let config = ModelConfig {
        max_rules: 3,
        max_personal_pairs: 2,
        ..ModelConfig::default()
    };
    let mut model = AdaptiveModel::new(config);
    let old_ignore = named_rule("old-ignore");
    let newer_ignore = named_rule("newer-ignore");
    let suggest = named_rule("suggest");
    let newcomer = named_rule("newcomer");
    model.apply_feedback(
        &old_ignore,
        &feedback(1, 1, FeedbackKind::ExplicitReject { candidate_id: 1 }),
        true,
    );
    model.apply_feedback(
        &newer_ignore,
        &feedback(2, 2, FeedbackKind::ExplicitReject { candidate_id: 2 }),
        true,
    );
    model.apply_feedback(
        &suggest,
        &feedback(3, 3, FeedbackKind::Accept { candidate_id: 3 }),
        true,
    );

    model.apply_feedback(
        &newcomer,
        &feedback(4, 4, FeedbackKind::Accept { candidate_id: 4 }),
        true,
    );

    assert_eq!(model.evidence_totals(&old_ignore), (0.0, 0.0));
    assert_eq!(model.evidence_totals(&newer_ignore), (0.0, 1.0));
    assert_eq!(model.evidence_totals(&suggest), (1.0, 0.0));
    assert_eq!(model.evidence_totals(&newcomer), (1.0, 0.0));
    assert_eq!(model.inspection_rows().len(), 3);
}

#[test]
fn inserting_personal_at_both_caps_evicts_only_one_row() {
    let config = ModelConfig {
        max_rules: 2,
        max_personal_pairs: 1,
        ..ModelConfig::default()
    };
    let mut model = AdaptiveModel::new(config);
    let global = named_rule("global-kept");
    model.apply_feedback(
        &global,
        &feedback(1, 0, FeedbackKind::Accept { candidate_id: 1 }),
        true,
    );
    model.record_personal_correction(
        InputMethod::Telex,
        "old-personal",
        "old-value",
        PersonalTransaction {
            anchor: 2,
            at_ms: 0,
        },
        true,
    );

    model.record_personal_correction(
        InputMethod::Telex,
        "new-personal",
        "new-value",
        PersonalTransaction {
            anchor: 3,
            at_ms: 0,
        },
        true,
    );

    assert_eq!(model.inspection_rows().len(), 2);
    assert_eq!(model.evidence_totals(&global), (1.0, 0.0));
    assert_eq!(
        model.personal_correction_count(InputMethod::Telex, "old-personal", "old-value"),
        0
    );
    assert_eq!(
        model.personal_correction_count(InputMethod::Telex, "new-personal", "new-value"),
        1
    );
}

#[test]
fn personal_at_cap_evicts_weak_probation_before_promoted_pair() {
    let config = ModelConfig {
        max_rules: 3,
        max_personal_pairs: 2,
        ..ModelConfig::default()
    };
    let mut model = AdaptiveModel::new(config);
    model.record_personal_correction(
        InputMethod::Telex,
        "weak",
        "w",
        PersonalTransaction {
            anchor: 1,
            at_ms: 0,
        },
        true,
    );
    model.record_personal_correction(
        InputMethod::Telex,
        "kept",
        "k",
        PersonalTransaction {
            anchor: 2,
            at_ms: 0,
        },
        true,
    );
    assert!(model.record_personal_correction(
        InputMethod::Telex,
        "kept",
        "k",
        PersonalTransaction {
            anchor: 3,
            at_ms: 0
        },
        true,
    ));

    model.record_personal_correction(
        InputMethod::Telex,
        "new",
        "n",
        PersonalTransaction {
            anchor: 4,
            at_ms: 0,
        },
        true,
    );

    assert_eq!(
        model.personal_correction_count(InputMethod::Telex, "weak", "w"),
        0
    );
    assert_eq!(
        model.personal_correction_count(InputMethod::Telex, "kept", "k"),
        2
    );
    assert_eq!(
        model.personal_correction_count(InputMethod::Telex, "new", "n"),
        1
    );
    assert_eq!(
        model.personal_promoted(),
        vec![(InputMethod::Telex, "kept".into(), "k".into())]
    );
}
