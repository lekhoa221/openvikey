//! Payload-v2 migration and strict schema contracts.

use openvikey_core::decision::{ActionCap, DecisionConfig, DecisionState, decide};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::model::{AdaptiveModel, ModelError, ModelView, RuleContextKey};
use openvikey_core::types::{CandidateSource, InputMethod};

const V1_FIXTURE: &[u8] = include_bytes!("fixtures/model_v1_personal_and_context.json");
const PRE_TRANSACTION_V2_FIXTURE: &[u8] =
    include_bytes!("fixtures/model_v2_pre_personal_transactions.json");

fn migrated_rule(left_token_nfc: Option<&str>) -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "original-migration".into(),
        candidate_nfc: "candidate-migration".into(),
        left_token_nfc: left_token_nfc.map(str::to_string),
        source_rule_id: "rule-migration".into(),
    }
}

#[test]
fn v1_fixture_migrates_deterministically() {
    let first = AdaptiveModel::from_json_payload(V1_FIXTURE).unwrap();
    let second = AdaptiveModel::from_json_payload(V1_FIXTURE).unwrap();

    assert_eq!(
        first.to_json_payload().unwrap(),
        second.to_json_payload().unwrap()
    );
    assert_eq!(first.payload_version(), 2);

    let payload: serde_json::Value =
        serde_json::from_slice(&first.to_json_payload().unwrap()).unwrap();
    assert_eq!(payload["version"], 2);
    assert_eq!(
        payload["config_hash"],
        LearningConfigV2::compatibility_v1().hash()
    );
    assert!(payload.get("correction_memory").is_some());
    assert!(payload.get("user_language_model").is_some());
    assert!(payload.get("maintenance_metadata").is_some());
    assert!(payload.get("entries").is_none());
    assert!(payload.get("personal").is_none());
}

#[test]
fn migration_builds_one_global_summary_and_blends_one_context() {
    let model = AdaptiveModel::from_json_payload(V1_FIXTURE).unwrap();

    assert!((model.positive_mass(&migrated_rule(None), 1_000) - 2.0).abs() < 1e-9);
    assert!((model.positive_mass(&migrated_rule(Some("tôi")), 1_000) - 5.0 / 3.0).abs() < 1e-9);
}

#[test]
fn migration_does_not_let_sibling_auto_promote_other_contexts() {
    let model = AdaptiveModel::from_json_payload(V1_FIXTURE).unwrap();
    let context = migrated_rule(Some("tôi"));

    assert_eq!(
        model.state(&migrated_rule(None), 2_000),
        DecisionState::Suggest
    );
    assert_eq!(model.state(&context, 2_000), DecisionState::Auto);
    assert_eq!(
        model
            .inspection_rows()
            .into_iter()
            .find(|row| row.left_token_nfc.as_deref() == Some("tôi"))
            .unwrap()
            .state,
        DecisionState::Auto,
        "inspection exposes stored state, not effective intervention eligibility"
    );
    assert_eq!(
        decide(
            model.state(&context, 2_000),
            0.95,
            model.confidence(&context, 2_000),
            model.positive_mass(&context, 2_000),
            ActionCap::Auto,
            &DecisionConfig::default(),
        ),
        DecisionState::Suggest,
        "v1 Auto remains stored for hysteresis but fails v2 evidence guards"
    );
    assert_eq!(
        model.state(&migrated_rule(Some("bạn")), 2_000),
        DecisionState::Suggest
    );
    assert_eq!(model.max_recorded_edit_id(), 91);
}

#[test]
fn forgetting_migrated_identity_scrubs_global_context_and_metadata() {
    let mut model = AdaptiveModel::from_json_payload(V1_FIXTURE).unwrap();

    assert!(model.forget_rule(&migrated_rule(Some("tôi"))));

    let payload = String::from_utf8(model.to_json_payload().unwrap()).unwrap();
    assert!(!payload.contains("original-migration"));
    assert!(!payload.contains("candidate-migration"));
    assert!(!payload.contains("tôi"));
    assert_eq!(model.max_recorded_edit_id(), 0);
}

#[test]
fn migration_moves_promoted_personal_pair_into_v2_namespace() {
    let model = AdaptiveModel::from_json_payload(V1_FIXTURE).unwrap();

    assert_eq!(
        model.personal_promoted(),
        vec![(InputMethod::Vni, "x3uong".into(), "xưởng".into())]
    );
    assert_eq!(
        model.personal_correction_count(InputMethod::Vni, "x3uong", "xưởng"),
        2
    );
}

#[test]
fn migrated_personal_count_is_support_not_fabricated_recency_evidence() {
    let model = AdaptiveModel::from_json_payload(V1_FIXTURE).unwrap();
    let personal = RuleContextKey {
        input_method: InputMethod::Vni,
        source: CandidateSource::Personal,
        original_nfc: "x3uong".into(),
        candidate_nfc: "xưởng".into(),
        left_token_nfc: None,
        source_rule_id: "personal-correction".into(),
    };

    assert!(model.positive_mass(&personal, 0).abs() < f64::EPSILON);
    assert!(model.positive_mass(&personal, 1_750_000_000_000).abs() < f64::EPSILON);
    assert!((model.confidence(&personal, 1_750_000_000_000) - 0.5).abs() < f64::EPSILON);
}

#[test]
fn pre_transaction_v2_fixture_upgrades_to_transaction_records() {
    let model = AdaptiveModel::from_json_payload(PRE_TRANSACTION_V2_FIXTURE).unwrap();

    assert_eq!(
        model.personal_correction_count(
            InputMethod::Vni,
            "legacy-v2-original",
            "legacy-v2-candidate"
        ),
        2
    );
    assert_eq!(
        model.personal_promoted(),
        vec![(
            InputMethod::Vni,
            "legacy-v2-original".into(),
            "legacy-v2-candidate".into()
        )]
    );
    let payload = String::from_utf8(model.to_json_payload().unwrap()).unwrap();
    assert!(!payload.contains("personal_observation_count"));
    assert!(payload.contains("personal_transactions"));
    let legacy_suggestion = RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "legacy-v2-suggestion".into(),
        candidate_nfc: "legacy-v2-expansion".into(),
        left_token_nfc: None,
        source_rule_id: "seed:legacy-v2".into(),
    };
    assert!(model.negative_mass(&legacy_suggestion, 100).abs() < f64::EPSILON);
    assert_eq!(
        model
            .inspection_rows()
            .into_iter()
            .find(|row| row.original_nfc == "legacy-v2-suggestion")
            .unwrap()
            .shown_count,
        1
    );
}

#[test]
fn migration_turns_legacy_suggestion_settlement_into_impression_only() {
    let legacy = r#"{
        "version":1,
        "config":{"half_life_ms":2592000000,"max_events_per_rule":512,"auto_undo_window":10},
        "entries":[{
            "key":{"input_method":"Telex","source":"Abbreviation","original_nfc":"ko","candidate_nfc":"không","left_token_nfc":null,"source_rule_id":"seed:ko"},
            "state":"Suggest",
            "evidence":[{"seq":1,"at_ms":100,"positive_add":0.0,"negative_add":0.2}],
            "recent_auto":[],
            "handled_feedback_seqs":[1],
            "settled_auto_ids":[],
            "settled_suggestion_ids":[9],
            "auto_demoted_at_seq":null
        }]
    }"#;
    let model = AdaptiveModel::from_json_payload(legacy.as_bytes()).unwrap();
    let rule = RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ko".into(),
        candidate_nfc: "không".into(),
        left_token_nfc: None,
        source_rule_id: "seed:ko".into(),
    };

    assert!(model.negative_mass(&rule, 100).abs() < f64::EPSILON);
    assert_eq!(model.inspection_rows()[0].shown_count, 1);
}

#[test]
fn serde_defaults_cannot_smuggle_v1_as_v2_meanings() {
    assert!(matches!(
        AdaptiveModel::from_json_payload(br#"{"version":2}"#),
        Err(ModelError::InvalidPayload(_))
    ));
}
