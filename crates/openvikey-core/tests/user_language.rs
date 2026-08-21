use openvikey_core::correction::InterventionConfig;
use openvikey_core::intervention::{InterventionAction, plan_intervention};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::{AdaptiveModel, ModelView, RuleContextKey};
use openvikey_core::rank::{RankingContext, ScoreConfig, rank};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, FeedbackEvent, FeedbackKind, InputContext,
    InputMethod,
};
use openvikey_core::user_language::UserLanguageModel;

#[test]
fn nfc_and_nfd_share_unigram_identity() {
    let mut language = UserLanguageModel::default();

    assert!(language.commit("ơ", None, 0));
    assert!(language.commit("o\u{031b}", None, 1));

    assert_eq!(language.unigram("ơ"), 2);
    assert_eq!(language.unigram_count(), 1);
}

#[test]
fn vietnamese_letter_identities_remain_distinct() {
    let mut language = UserLanguageModel::default();

    assert!(language.commit("a", None, 0));
    assert!(language.commit("ă", None, 1));
    assert!(language.commit("d", None, 2));
    assert!(language.commit("đ", None, 3));

    assert_eq!(language.unigram("a"), 1);
    assert_eq!(language.unigram("ă"), 1);
    assert_eq!(language.unigram("d"), 1);
    assert_eq!(language.unigram("đ"), 1);
    assert_eq!(language.unigram_count(), 4);
}

#[test]
fn unsafe_or_ambiguous_surfaces_are_not_learned() {
    let mut language = UserLanguageModel::default();

    assert!(!language.commit("", None, 0));
    assert!(!language.commit("hai từ", None, 1));
    assert!(!language.commit("https://example.test", None, 2));
    assert!(!language.commit("name@example.test", None, 3));
    assert!(!language.commit("Abc123!", None, 4));
    assert_eq!(language.unigram_count(), 0);
}

#[test]
fn disabled_learning_is_zero_mutation_for_language_commits() {
    let mut model = AdaptiveModel::default();
    let before = model.to_json_payload().unwrap();

    assert!(!model.record_language_commit("Nam", Some("Việt"), 10, 1, false));

    assert_eq!(model.to_json_payload().unwrap(), before);
    assert_eq!(model.unigram_count("Nam"), 0);
}

#[test]
fn unigram_can_rerank_but_cannot_grant_auto_without_exact_evidence() {
    let mut model = AdaptiveModel::default();
    for transaction_id in 1..=16 {
        assert!(model.record_language_commit("Nam", None, 0, transaction_id, true));
    }
    let candidates = vec![
        Candidate {
            id: 1,
            text: "năm".into(),
            source: CandidateSource::Fuzzy,
            evidence: "fuzzy:nam:năm".into(),
            base_score: 0.69,
            final_score: 0.69,
        },
        Candidate {
            id: 2,
            text: "Nam".into(),
            source: CandidateSource::Fuzzy,
            evidence: "fuzzy:nam:Nam".into(),
            base_score: 0.66,
            final_score: 0.66,
        },
    ];
    let ranked = rank(
        candidates,
        &model,
        0,
        &ScoreConfig::abbrev_v1(),
        Some(&RankingContext {
            input_method: InputMethod::Telex,
            original_nfc: "nam".into(),
            left_token_nfc: None,
        }),
    );
    assert_eq!(ranked[0].text, "Nam");

    let plan = plan_intervention(
        &CompositionSnapshot::new(1, "nam".into(), "nam".into()),
        &ranked,
        &Lexicon::empty(),
        &model,
        &LearningConfigV2::product_v2(),
        InterventionConfig::win32(),
        InputContext::default(),
        Some(' '),
        None,
        0,
        true,
        InputMethod::Telex,
        None,
    );
    assert_eq!(plan.action, InterventionAction::DisplaySuggestion);
    assert!(plan.score_breakdown.unigram > 0.0);
}

#[test]
fn prune_when_over_cap_drops_stale_low_count_first() {
    let mut language = UserLanguageModel::default();
    assert!(language.commit_bounded("stale", None, 0, 3, 30));
    assert!(language.commit_bounded("strong", None, 1, 3, 30));
    assert!(language.commit_bounded("strong", None, 2, 3, 30));
    assert!(language.commit_bounded("recent", None, 100, 3, 30));

    assert!(language.commit_bounded("new", None, 200, 3, 30));

    assert_eq!(language.unigram("stale"), 0);
    assert_eq!(language.unigram("strong"), 2);
    assert_eq!(language.unigram("recent"), 1);
    assert_eq!(language.unigram("new"), 1);
    assert_eq!(language.unigram_count(), 3);
}

fn correction_rule() -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "khogn".into(),
        candidate_nfc: "không".into(),
        left_token_nfc: None,
        source_rule_id: "fuzzy:khogn".into(),
    }
}

fn seed_correction(model: &mut AdaptiveModel) {
    model.apply_feedback(
        &correction_rule(),
        &FeedbackEvent {
            seq: 1,
            at_ms: 0,
            kind: FeedbackKind::Accept { candidate_id: 42 },
        },
        true,
    );
}

#[test]
fn forget_token_does_not_delete_correction_row() {
    let mut model = AdaptiveModel::default();
    seed_correction(&mut model);
    assert!(model.record_language_commit("không", None, 1, 2, true));
    let correction_mass = model.positive_mass(&correction_rule(), 1);

    assert!(model.forget_token("không"));

    assert_eq!(model.unigram_count("không"), 0);
    assert!((model.positive_mass(&correction_rule(), 1) - correction_mass).abs() < 1e-12);
}

#[test]
fn forget_correction_does_not_delete_unigram() {
    let mut model = AdaptiveModel::default();
    seed_correction(&mut model);
    assert!(model.record_language_commit("không", None, 1, 2, true));

    assert!(model.forget_rule(&correction_rule()));

    assert_eq!(model.unigram_count("không"), 1);
    assert!(model.positive_mass(&correction_rule(), 1).abs() < 1e-12);
}

#[test]
fn unigram_inspection_and_forget_survive_payload_round_trip() {
    let mut model = AdaptiveModel::default();
    assert!(model.record_language_commit("Nam", None, 10, 1, true));
    assert_eq!(model.language_unigrams()[0].token_nfc, "Nam");

    assert!(model.forget_token("Nam"));
    let payload = model.to_json_payload().unwrap();
    let reloaded = AdaptiveModel::from_json_payload(&payload).unwrap();

    assert!(reloaded.language_unigrams().is_empty());
}

#[test]
fn one_left_token_bigram_is_normalized_and_counted() {
    let mut language = UserLanguageModel::default();

    assert!(language.commit("Nam", Some("Việt"), 0));
    assert!(language.commit("Nam", Some("Vie\u{0323}\u{0302}t"), 1));

    assert_eq!(language.bigram("Việt", "Nam"), 2);
    assert_eq!(language.bigram_count(), 1);

    let payload = serde_json::to_vec(&language).unwrap();
    let reloaded: UserLanguageModel = serde_json::from_slice(&payload).unwrap();
    assert_eq!(reloaded.bigram("Việt", "Nam"), 2);
}

#[test]
fn replayed_language_transaction_is_idempotent() {
    let mut language = UserLanguageModel::default();

    assert!(language.commit_transaction("Nam", Some("Việt"), 0, 7));
    assert!(!language.commit_transaction("Nam", Some("Việt"), 1, 7));

    assert_eq!(language.unigram("Nam"), 1);
    assert_eq!(language.bigram("Việt", "Nam"), 1);
}

#[test]
fn bigram_can_rerank_in_context_but_cannot_grant_auto() {
    let mut model = AdaptiveModel::default();
    for transaction_id in 1..=8 {
        assert!(model.record_language_commit("Nam", Some("Việt"), 0, transaction_id, true));
    }
    for transaction_id in 9..=16 {
        assert!(model.record_language_commit("năm", Some("mỗi"), 0, transaction_id, true));
    }
    let candidates = vec![
        Candidate {
            id: 1,
            text: "năm".into(),
            source: CandidateSource::Fuzzy,
            evidence: "fuzzy:nam:năm".into(),
            base_score: 0.69,
            final_score: 0.69,
        },
        Candidate {
            id: 2,
            text: "Nam".into(),
            source: CandidateSource::Fuzzy,
            evidence: "fuzzy:nam:Nam".into(),
            base_score: 0.66,
            final_score: 0.66,
        },
    ];
    let context = RankingContext {
        input_method: InputMethod::Telex,
        original_nfc: "nam".into(),
        left_token_nfc: Some("Việt".into()),
    };
    let ranked = rank(
        candidates,
        &model,
        0,
        &ScoreConfig::abbrev_v1(),
        Some(&context),
    );
    assert_eq!(ranked[0].text, "Nam");

    let plan = plan_intervention(
        &CompositionSnapshot::new(1, "nam".into(), "nam".into()),
        &ranked,
        &Lexicon::empty(),
        &model,
        &LearningConfigV2::product_v2(),
        InterventionConfig::win32(),
        InputContext::default(),
        Some(' '),
        None,
        0,
        true,
        InputMethod::Telex,
        Some("Việt"),
    );
    assert_eq!(plan.action, InterventionAction::DisplaySuggestion);
    assert!(plan.score_breakdown.bigram > 0.0);
    assert!(plan.score_breakdown.unigram.abs() < f64::EPSILON);
}

#[test]
fn bigram_pruning_is_bounded_and_forget_token_removes_connected_rows() {
    let mut language = UserLanguageModel::default();
    assert!(language.commit_bounded("một", Some("alpha"), 0, 10, 2));
    assert!(language.commit_bounded("hai", Some("beta"), 1, 10, 2));
    assert!(language.commit_bounded("ba", Some("gamma"), 2, 10, 2));

    assert_eq!(language.bigram_count(), 2);
    assert_eq!(language.bigram("alpha", "một"), 0);
    assert!(language.forget_token("gamma"));
    assert_eq!(language.bigram("gamma", "ba"), 0);
}
