//! Milestone 7C: per-token, left-context diacritics suggestions.

use openvikey_core::correction::{
    AutoEditContext, InterventionConfig, run_learning_correction_slice,
};
use openvikey_core::decision::{DecisionConfig, DecisionState};
use openvikey_core::feedback::LearningSession;
use openvikey_core::generate::diacritics::DiacriticsGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{AdaptiveModel, RuleContextKey};
use openvikey_core::rank::ScoreConfig;
use openvikey_core::types::{
    CandidateSource, CompositionSnapshot, EditRange, EngineAction, FeedbackEvent, FeedbackKind,
    InputContext, InputMethod, RangeBasis,
};

fn lexicon() -> Lexicon {
    Lexicon::from_entries(
        [
            LexiconEntry {
                token_nfc: "ban".to_string(),
                frequency: 120,
            },
            LexiconEntry {
                token_nfc: "bàn".to_string(),
                frequency: 100,
            },
            LexiconEntry {
                token_nfc: "bán".to_string(),
                frequency: 90,
            },
            LexiconEntry {
                token_nfc: "bạn".to_string(),
                frequency: 80,
            },
            LexiconEntry {
                token_nfc: "năm".to_string(),
                frequency: 70,
            },
            LexiconEntry {
                token_nfc: "sự".to_string(),
                frequency: 60,
            },
        ],
        [(("của".to_string(), "bạn".to_string()), 1.0)],
        Some("diacritics-test"),
    )
}

fn snapshot(input: &str) -> CompositionSnapshot {
    CompositionSnapshot::new(10, input.to_string(), input.to_string())
}

#[test]
fn left_bigram_reranks_ambiguous_unaccented_token() {
    let lexicon = lexicon();
    let generator = DiacriticsGenerator::new(&lexicon, 3);

    let without_context = generator.generate(&snapshot("ban"), &LeftContext::default());
    assert_eq!(without_context[0].text, "bàn");

    let with_context = generator.generate(
        &snapshot("ban"),
        &LeftContext {
            prev_token_nfc: Some("của".to_string()),
        },
    );
    assert_eq!(with_context[0].text, "bạn");
    assert!(with_context[0].base_score > with_context[1].base_score);
}

#[test]
fn output_is_stable_bounded_and_suggestion_only_source() {
    let lexicon = lexicon();
    let generator = DiacriticsGenerator::new(&lexicon, 2);
    let first = generator.generate(&snapshot("ban"), &LeftContext::default());
    let second = generator.generate(&snapshot("ban"), &LeftContext::default());

    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|candidate| candidate.source == CandidateSource::Diacritics)
    );
    assert!(first.iter().all(|candidate| candidate.text != "ban"));
}

#[test]
fn accented_input_and_phrase_level_restoration_are_out_of_scope() {
    let lexicon = lexicon();
    let generator = DiacriticsGenerator::new(&lexicon, 3);

    assert!(
        generator
            .generate(&snapshot("bạn"), &LeftContext::default())
            .is_empty()
    );
    assert_eq!(
        generator.generate(&snapshot("nam"), &LeftContext::default())[0].text,
        "năm"
    );
    assert_eq!(
        generator.generate(&snapshot("su"), &LeftContext::default())[0].text,
        "sự"
    );
    assert!(
        generator
            .generate(&snapshot("toi ban"), &LeftContext::default())
            .is_empty()
    );
}

#[test]
fn promoted_diacritics_rule_still_emits_suggestions_not_auto_replace() {
    let lexicon = lexicon();
    let generator = DiacriticsGenerator::new(&lexicon, 3);
    let left = LeftContext {
        prev_token_nfc: Some("của".to_string()),
    };
    let rule = RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Diacritics,
        original_nfc: "ban".to_string(),
        candidate_nfc: "bạn".to_string(),
        left_token_nfc: Some("của".to_string()),
        source_rule_id: "diacritics:ban->bạn".to_string(),
    };
    let mut model = AdaptiveModel::default();
    for seq in 1..=18 {
        model.apply_feedback(
            &rule,
            &FeedbackEvent {
                seq,
                at_ms: 0,
                kind: FeedbackKind::Accept { candidate_id: 0 },
            },
            true,
        );
    }
    let mut session = LearningSession::new(model, 8);
    let slice = run_learning_correction_slice(
        &snapshot("ban"),
        &left,
        InputContext::default(),
        &[&generator],
        InputMethod::Telex,
        &mut session,
        0,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        Some(AutoEditContext {
            edit_id: 7,
            range: EditRange {
                basis: RangeBasis::CommittedBeforeCaret,
                start_grapheme: 0,
                length_grapheme: 3,
                revision: 10,
            },
            delimiter: None,
        }),
        &lexicon,
        InterventionConfig::default(),
    );

    assert_eq!(slice.decision, Some(DecisionState::Suggest));
    assert!(matches!(
        slice.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));
}
