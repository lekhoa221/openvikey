use openvikey_core::correction::InterventionConfig;
use openvikey_core::intervention::{InterventionAction, plan_intervention};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::AdaptiveModel;
use openvikey_core::rank::{RankingContext, ScoreConfig, rank};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, InputContext, InputMethod,
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
