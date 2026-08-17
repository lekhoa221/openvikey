//! Wave 0: lock core seams later waves must not reshape.
//!
//! Each test names the production change that would make it fail.

use openvikey_core::decision::{ActionCap, DecisionConfig, DecisionState};
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{EmptyModel, ModelView, RuleContextKey};
use openvikey_core::store::{InMemorySecretProvider, InMemoryStore, ModelStore, SecretProvider};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, FeedbackEvent, FeedbackKind, InputMethod,
};

#[test]
fn lexicon_lookup_is_nfc_key_and_unknown_is_none() {
    let lexicon = Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "không".to_string(),
            frequency: 42,
        }],
        [],
        Some("manifest-hash-example"),
    );

    assert!(lexicon.contains("không"));
    assert_eq!(
        lexicon.lookup("không").map(|entry| entry.frequency),
        Some(42)
    );
    assert_eq!(lexicon.lookup("khong"), None);
    assert_eq!(
        lexicon.source_manifest_hash(),
        Some("manifest-hash-example")
    );
}

#[test]
fn lexicon_bigram_lookup_is_left_then_token() {
    let lexicon = Lexicon::from_entries([], [(("xin".to_string(), "chào".to_string()), 0.8)], None);

    assert!((lexicon.bigram("xin", "chào").unwrap() - 0.8).abs() < f64::EPSILON);
    assert_eq!(lexicon.bigram("chào", "xin"), None);
}

struct SeedAbbrev;

impl Generator for SeedAbbrev {
    fn source(&self) -> CandidateSource {
        CandidateSource::Abbreviation
    }

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        _left_context: &LeftContext,
    ) -> Vec<Candidate> {
        if snapshot.normalized == "ko" {
            vec![Candidate {
                id: 1,
                text: "không".to_string(),
                source: self.source(),
                evidence: "seed:ko".to_string(),
                base_score: 0.8,
                final_score: 0.0,
            }]
        } else {
            Vec::new()
        }
    }
}

#[test]
fn generator_reads_snapshot_and_left_context_not_a_model() {
    let snapshot = CompositionSnapshot::new(3, "ko".to_string(), "ko".to_string());
    let left = LeftContext {
        prev_token_nfc: Some("xin".to_string()),
    };
    let candidates = SeedAbbrev.generate(&snapshot, &left);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].text, "không");
    assert_eq!(candidates[0].source, CandidateSource::Abbreviation);
    assert_eq!(candidates[0].evidence, "seed:ko");
    assert!((candidates[0].base_score - 0.8).abs() < f64::EPSILON);
    // final_score is rank's job; generators leave it unset.
    assert!(candidates[0].final_score.abs() < f64::EPSILON);
}

#[test]
fn diacritics_source_cannot_exceed_suggest() {
    assert_eq!(CandidateSource::Diacritics.max_action(), ActionCap::Suggest);
    assert_eq!(CandidateSource::Abbreviation.max_action(), ActionCap::Auto);
}

fn sample_key(original: &str, candidate: &str) -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: original.to_string(),
        candidate_nfc: candidate.to_string(),
        left_token_nfc: None,
        source_rule_id: "seed:ko".to_string(),
    }
}

#[test]
fn empty_model_is_beta_prior_and_ignore() {
    let model = EmptyModel::new();
    let key = sample_key("ko", "không");
    assert!((model.confidence(&key, 0) - 0.5).abs() < f64::EPSILON);
    assert_eq!(model.state(&key, 0), DecisionState::Ignore);
}

#[test]
fn distinct_original_candidate_pairs_are_distinct_keys() {
    let a = sample_key("ko", "không");
    let b = sample_key("ko", "kể");
    assert_ne!(a, b);
}

#[test]
fn empty_model_ignores_feedback_and_serializes_versioned_json() {
    let mut model = EmptyModel::new();
    model.apply_feedback(
        &FeedbackEvent {
            seq: 1,
            at_ms: 10,
            kind: FeedbackKind::Accept { candidate_id: 1 },
        },
        &sample_key("ko", "không"),
    );
    assert!((model.confidence(&sample_key("ko", "không"), 10) - 0.5).abs() < f64::EPSILON);

    let bytes = model.to_json_payload().expect("serialize empty model");
    let as_text = String::from_utf8(bytes.clone()).expect("utf8 json");
    assert!(as_text.contains("\"version\":1"));
    assert!(as_text.contains("\"entries\""));
    let restored = EmptyModel::from_json_payload(&bytes).expect("deserialize");
    assert_eq!(model, restored);
}

#[test]
fn decision_config_defaults_match_spec() {
    let config = DecisionConfig::default();
    assert_eq!(config.version, 1);
    assert!((config.suggest_on - 0.70).abs() < f64::EPSILON);
    assert!((config.suggest_off - 0.60).abs() < f64::EPSILON);
    assert!((config.auto_score - 0.90).abs() < f64::EPSILON);
    assert!((config.auto_confidence - 0.95).abs() < f64::EPSILON);
    assert!((config.promote_positive_mass - 18.0).abs() < f64::EPSILON);
    assert!(config.suggest_off < config.suggest_on);
}

#[test]
fn store_round_trips_opaque_json_bytes_not_a_typed_model() {
    let provider = InMemorySecretProvider::new();
    let mut store = InMemoryStore::new();
    let model = EmptyModel::new();
    let payload = model.to_json_payload().expect("payload");

    store.save(&payload, &provider).expect("save");
    let loaded = store.load(&provider).expect("load");
    assert_eq!(loaded, payload);

    let restored =
        EmptyModel::from_json_payload(&loaded).expect("typed decode is the caller's job");
    assert_eq!(restored, model);
}

#[test]
fn secret_provider_wrap_then_unwrap_recovers_dek() {
    let provider = InMemorySecretProvider::new();
    let dek = openvikey_core::store::Dek::from_bytes(vec![1, 2, 3, 4]);
    let wrapped = provider.wrap(&dek).expect("wrap");
    let opened = provider.unwrap(&wrapped).expect("unwrap");
    assert_eq!(opened.as_bytes(), dek.as_bytes());
}

#[test]
fn store_error_kinds_are_distinct() {
    use openvikey_core::store::StoreError;
    assert_ne!(StoreError::WrongPassphrase, StoreError::CorruptHeader);
    assert_ne!(StoreError::CorruptHeader, StoreError::CorruptCiphertext);
    assert_ne!(
        StoreError::CorruptCiphertext,
        StoreError::UnsupportedVersion
    );
}
