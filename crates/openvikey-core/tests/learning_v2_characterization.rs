//! Lát 0 freeze of current learning behavior.
//!
//! Lát 2/9 may replace named tests listed in the v2 implementation plan.
//! Every other test here must stay green unless a later lát explicitly inverts it.
//!
//! Covered elsewhere (do not duplicate):
//! - `telex_fix::boundary_assist_picks_unique_abbrev_on_space`
//! - `telex_fix::boundary_assist_picks_unique_fuzzy_on_space`
//! - `telex_fix::boundary_assist_rejects_diacritics_even_when_unique`
//! - `session_capture::abbrev_boundary_assist_replaces_on_space_without_accept_mass`
//! - `session_capture::fuzzy_boundary_assist_replaces_unique_typo_on_space`
//! - `session_capture::ntn_space_does_not_boundary_assist_a_guess`
//! - `session_capture::diacritics_does_not_boundary_assist_on_space`
//! - `session_capture::two_abbrev_assist_undos_stop_further_space_auto`
//! - `learning_state_machine::promoted_correction_emits_replace_range_and_records_auto_edit`
//! - `learning_state_machine::settled_signals_are_recorded_exactly_once`
//! - `learning_state_machine::learning_disabled_does_not_mutate_model`
//! - `learning_state_machine::canonical_accept_18_promotes_but_17_does_not`
//! - `abbrev_slice::allow_transform_false_skips_generate_rank_and_decide`
//! - `session_capture::composition_rewind_first_word_learns_abbrev_from_keys`
//! - `session_capture::restart_restores_cursor_so_new_feedback_is_not_deduped`

use openvikey_core::correction::{
    InterventionConfig, boundary_assist_candidate, run_correction_slice, unique_telex_fix_candidate,
};
use openvikey_core::correction_memory::PersonalTransaction;
use openvikey_core::decision::{DecisionConfig, DecisionState};
use openvikey_core::generate::telex_fix::TelexFixGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{AdaptiveModel, EmptyModel, ModelView, RuleContextKey};
use openvikey_core::rank::ScoreConfig;
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, EngineAction, FeedbackEvent, FeedbackKind,
    InputContext, InputMethod, TonePlacement,
};

fn snap(raw: &str, rendered: &str) -> CompositionSnapshot {
    CompositionSnapshot::new(1, raw.to_string(), rendered.to_string())
}

fn lex(tokens: &[&str]) -> Lexicon {
    Lexicon::from_entries(
        tokens.iter().map(|token| LexiconEntry {
            token_nfc: (*token).to_string(),
            frequency: 10,
        }),
        [],
        Some("char-v2"),
    )
}

#[test]
fn telex_fix_unique_is_boundary_assist_on_win32_space() {
    let generator = TelexFixGenerator::new(InputMethod::Telex, TonePlacement::Modern);
    let snapshot = snap("chfao", "chfao");
    let candidates = generator.generate(&snapshot, &LeftContext::default());
    let lexicon = lex(&["chào"]);
    assert!(unique_telex_fix_candidate(&candidates).is_some());
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lexicon,
            true,
        )
        .is_some()
    );
}

#[test]
fn diacritics_source_never_boundary_assists() {
    let snapshot = snap("khong", "khong");
    let candidates = vec![Candidate {
        id: 1,
        text: "không".into(),
        source: CandidateSource::Diacritics,
        evidence: "diac:khong".into(),
        base_score: 0.9,
        final_score: 0.9,
    }];
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lex(&["không"]),
            true,
        )
        .is_none()
    );
}

fn ko_rule() -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ko".to_string(),
        candidate_nfc: "không".to_string(),
        left_token_nfc: None,
        source_rule_id: "seed:ko".to_string(),
    }
}

fn accept_event(seq: u64) -> FeedbackEvent {
    FeedbackEvent {
        seq,
        at_ms: 0,
        kind: FeedbackKind::Accept { candidate_id: 1 },
    }
}

#[test]
fn suggestion_settled_still_adds_negative_zero_point_two_in_v1() {
    let mut model = AdaptiveModel::default();
    let key = ko_rule();
    model.apply_feedback(
        &key,
        &FeedbackEvent {
            seq: 1,
            at_ms: 0,
            kind: FeedbackKind::SuggestionSettled { candidate_id: 9 },
        },
        true,
    );
    assert!((model.negative_mass(&key, 0) - 0.2).abs() < 1e-9);
}

#[test]
fn allow_learning_false_is_zero_mutation() {
    let mut model = AdaptiveModel::default();
    let before = model.to_json_payload().unwrap();
    model.apply_feedback(&ko_rule(), &accept_event(1), false);
    assert_eq!(model.to_json_payload().unwrap(), before);
}

fn khogn_rule() -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "khogn".to_string(),
        candidate_nfc: "không".to_string(),
        left_token_nfc: None,
        source_rule_id: "fuzzy:khogn".to_string(),
    }
}

#[test]
fn forget_rule_removes_original_strings_from_payload() {
    let mut model = AdaptiveModel::default();
    let key = khogn_rule();
    model.apply_feedback(&key, &accept_event(1), true);
    assert!(model.forget_rule(&key));
    let payload = String::from_utf8(model.to_json_payload().unwrap()).unwrap();
    assert!(!payload.contains("khogn"), "{payload}");
    assert!(!payload.contains("không"), "{payload}");
}

#[test]
fn context_state_no_longer_uses_max_across_siblings() {
    let global = khogn_rule();
    let mut viet = global.clone();
    viet.left_token_nfc = Some("Việt".to_string());
    let mut model = AdaptiveModel::default();
    model.record_decision(&global, DecisionState::Suggest, true);
    model.record_decision(&viet, DecisionState::Auto, true);
    assert_eq!(model.state(&global, 0), DecisionState::Suggest);
    assert_eq!(model.state(&viet, 0), DecisionState::Auto);
}

#[test]
fn personal_store_evicts_weak_pair_at_512_instead_of_rejecting_new_pair() {
    let mut model = AdaptiveModel::default();
    for index in 0..512_u32 {
        let original = format!("orig{index}");
        let replacement = format!("repl{index}");
        assert!(!model.record_personal_correction(
            InputMethod::Telex,
            &original,
            &replacement,
            PersonalTransaction {
                anchor: u64::from(index) + 1,
                at_ms: 0,
            },
            true
        ));
    }
    assert_eq!(
        model.personal_correction_count(InputMethod::Telex, "orig511", "repl511"),
        1
    );
    assert!(!model.record_personal_correction(
        InputMethod::Telex,
        "orig512",
        "repl512",
        PersonalTransaction {
            anchor: 513,
            at_ms: 0,
        },
        true,
    ));
    assert_eq!(
        model.personal_correction_count(InputMethod::Telex, "orig0", "repl0"),
        0
    );
    assert_eq!(
        model.personal_correction_count(InputMethod::Telex, "orig512", "repl512"),
        1
    );
}

struct OneLetterGenerator;

impl Generator for OneLetterGenerator {
    fn source(&self) -> CandidateSource {
        CandidateSource::Fuzzy
    }

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        _left_context: &LeftContext,
    ) -> Vec<Candidate> {
        if snapshot.normalized != "đ" {
            return Vec::new();
        }
        vec![Candidate {
            id: 7,
            text: "đã".to_string(),
            source: CandidateSource::Fuzzy,
            evidence: "fuzzy:dd".to_string(),
            base_score: 0.92,
            final_score: 0.0,
        }]
    }
}

#[test]
fn token_too_short_dd_is_engine_only() {
    let snapshot = CompositionSnapshot::new(1, "dd".into(), "đ".into());
    assert_eq!(snapshot.normalized, "đ");
    let slice = run_correction_slice(
        &snapshot,
        &LeftContext::default(),
        InputContext::default(),
        &[&OneLetterGenerator],
        InputMethod::Telex,
        &EmptyModel,
        0,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
    );
    assert_ne!(slice.decision, Some(DecisionState::Suggest));
    assert!(!matches!(
        slice.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));
    assert_eq!(
        slice.plan.as_ref().map(|plan| plan.reason),
        Some(openvikey_core::intervention::InterventionReason::TokenTooShort)
    );
    assert!(slice.display_candidates().is_empty());
}
