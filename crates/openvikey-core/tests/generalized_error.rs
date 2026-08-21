//! Observe-only generalized typing-error statistics.

use openvikey_core::correction::InterventionConfig;
use openvikey_core::generalized_error::ErrorOperationClass;
use openvikey_core::intervention::{InterventionPlan, plan_intervention};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::AdaptiveModel;
use openvikey_core::rank::{RankingContext, rank};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, InputContext, InputMethod,
};

#[test]
fn trusted_transposition_observation_increments_only_transpose() {
    let mut model = AdaptiveModel::default();

    assert_eq!(
        model.observe_error_pattern("khogn", "không", InputMethod::Telex, true),
        Some(ErrorOperationClass::Transpose)
    );

    let stats = model.generalized_error_model();
    assert_eq!(stats.count(ErrorOperationClass::Transpose), 1);
    assert_eq!(stats.total_observations(), 1);
}

#[test]
fn trusted_observations_distinguish_the_four_bounded_operation_classes() {
    let mut model = AdaptiveModel::default();

    let cases = [
        (
            "khogn",
            "không",
            InputMethod::Telex,
            ErrorOperationClass::Transpose,
        ),
        (
            "khpng",
            "không",
            InputMethod::Telex,
            ErrorOperationClass::AdjacentKey,
        ),
        (
            "khbong",
            "không",
            InputMethod::Telex,
            ErrorOperationClass::ExtraKey,
        ),
        (
            "chfao",
            "chào",
            InputMethod::Telex,
            ErrorOperationClass::EarlyTone,
        ),
        (
            "ch2ao",
            "chào",
            InputMethod::Vni,
            ErrorOperationClass::EarlyTone,
        ),
    ];
    for (original, replacement, input_method, expected) in cases {
        assert_eq!(
            model.observe_error_pattern(original, replacement, input_method, true),
            Some(expected),
            "{original} -> {replacement}"
        );
    }

    for operation in [
        ErrorOperationClass::Transpose,
        ErrorOperationClass::AdjacentKey,
        ErrorOperationClass::ExtraKey,
        ErrorOperationClass::EarlyTone,
    ] {
        let expected = if operation == ErrorOperationClass::EarlyTone {
            2
        } else {
            1
        };
        assert_eq!(model.generalized_error_model().count(operation), expected);
    }
}

#[test]
fn observations_are_persisted_for_offline_dump_without_token_pairs() {
    let mut model = AdaptiveModel::default();
    model.observe_error_pattern("khogn", "không", InputMethod::Telex, true);

    let payload = model.to_json_payload().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(json["generalized_error_model"]["counts"]["transpose"], 1);
    let payload_text = String::from_utf8(payload.clone()).unwrap();
    assert!(!payload_text.contains("khogn"));
    assert!(!payload_text.contains("không"));

    let restored = AdaptiveModel::from_json_payload(&payload).unwrap();
    assert_eq!(
        restored
            .generalized_error_model()
            .count(ErrorOperationClass::Transpose),
        1
    );
}

#[test]
fn observation_does_not_change_ranked_order_or_plan_action() {
    let mut model = AdaptiveModel::default();
    let before = ranked_and_planned(&model);

    model.observe_error_pattern("khogn", "không", InputMethod::Telex, true);

    assert_eq!(ranked_and_planned(&model), before);
}

#[test]
fn disabled_or_unrecognized_observations_leave_stats_empty() {
    let mut model = AdaptiveModel::default();

    assert_eq!(
        model.observe_error_pattern("khogn", "không", InputMethod::Telex, false),
        None
    );
    assert_eq!(
        model.observe_error_pattern("abc", "xyz", InputMethod::Telex, true),
        None
    );
    assert_eq!(model.generalized_error_model().total_observations(), 0);
}

#[test]
fn ambiguous_extra_key_alignment_is_not_recorded() {
    let mut model = AdaptiveModel::default();

    assert_eq!(
        model.observe_error_pattern("khongg", "không", InputMethod::Telex, true),
        None
    );
    assert_eq!(model.generalized_error_model().total_observations(), 0);
}

#[test]
fn forget_all_clears_generalized_error_counts() {
    let mut model = AdaptiveModel::default();
    model.observe_error_pattern("khogn", "không", InputMethod::Telex, true);
    assert_eq!(model.generalized_error_model().total_observations(), 1);

    assert!(model.forget_all());
    assert_eq!(model.generalized_error_model().total_observations(), 0);
}

fn ranked_and_planned(model: &AdaptiveModel) -> (Vec<Candidate>, InterventionPlan) {
    let snapshot = CompositionSnapshot::new(1, "khogn".into(), "khogn".into());
    let config = LearningConfigV2::product_v2();
    let ranked = rank(
        vec![
            Candidate {
                id: 1,
                text: "không".into(),
                source: CandidateSource::Fuzzy,
                evidence: "fuzzy:khogn".into(),
                base_score: 0.95,
                final_score: 0.0,
            },
            Candidate {
                id: 2,
                text: "khổng".into(),
                source: CandidateSource::Fuzzy,
                evidence: "fuzzy:khogn:second".into(),
                base_score: 0.80,
                final_score: 0.0,
            },
        ],
        model,
        0,
        &config.score,
        Some(&RankingContext {
            input_method: InputMethod::Telex,
            original_nfc: "khogn".into(),
            left_token_nfc: None,
        }),
    );
    let plan = plan_intervention(
        &snapshot,
        &ranked,
        &Lexicon::empty(),
        model,
        &config,
        InterventionConfig::win32(),
        InputContext::default(),
        Some(' '),
        None,
        0,
        true,
        InputMethod::Telex,
        None,
    );
    (ranked, plan)
}
