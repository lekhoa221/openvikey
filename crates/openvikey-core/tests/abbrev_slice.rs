//! Milestone 5: abbreviation generate → rank → decision.

use openvikey_core::correction::run_correction_slice;
use openvikey_core::decision::{ActionCap, DecisionConfig, DecisionState, decide};
use openvikey_core::generate::abbrev::{ABBREV_SEED_JSONL, ABBREV_SEED_SHA256, AbbrevGenerator};
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::model::{EmptyModel, ModelView, RuleContextKey};
use openvikey_core::rank::{SCORE_CONFIG_V1_HASH, ScoreConfig, rank};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, EngineAction, InputContext, InputMethod,
};
use sha2::{Digest, Sha256};

fn snapshot(text: &str) -> CompositionSnapshot {
    CompositionSnapshot::new(1, text.to_string(), text.to_string())
}

#[test]
fn ko_expands_to_khong_with_source_evidence_and_base_score() {
    let generator = AbbrevGenerator::from_seed();
    let cands = generator.generate(&snapshot("ko"), &LeftContext::default());
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].text, "không");
    assert_eq!(cands[0].source, CandidateSource::Abbreviation);
    assert_eq!(cands[0].evidence, "seed:ko");
    assert!((cands[0].base_score - 0.8).abs() < f64::EPSILON);
}

#[test]
fn rank_dedupes_same_nfc_output_from_two_evidence_paths() {
    let generator = AbbrevGenerator::from_seed();
    let mut cands = generator.generate(&snapshot("ko"), &LeftContext::default());
    let winner_id = cands[0].id;
    cands.push(Candidate {
        id: 99,
        text: "không".to_string(),
        source: CandidateSource::Abbreviation,
        evidence: "alt:ko".to_string(),
        base_score: 0.55,
        final_score: 0.0,
    });
    let ranked = rank(
        cands,
        &EmptyModel::new(),
        0,
        &ScoreConfig::abbrev_v1(),
        None,
    );
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].text, "không");
    assert_eq!(ranked[0].id, winner_id);
    assert!((ranked[0].base_score - 0.8).abs() < f64::EPSILON);
    let parts: Vec<_> = ranked[0].evidence.split('+').collect();
    assert!(parts.contains(&"seed:ko"));
    assert!(parts.contains(&"alt:ko"));
}

#[test]
fn rank_tie_breaks_by_lexical_nfc_and_score_dominates() {
    let cands = vec![
        Candidate {
            id: 1,
            text: "ệa".to_string(),
            source: CandidateSource::Abbreviation,
            evidence: "b".to_string(),
            base_score: 0.8,
            final_score: 0.0,
        },
        Candidate {
            id: 2,
            text: "òa".to_string(),
            source: CandidateSource::Abbreviation,
            evidence: "a".to_string(),
            base_score: 0.8,
            final_score: 0.0,
        },
    ];
    let ranked = rank(
        cands,
        &EmptyModel::new(),
        0,
        &ScoreConfig::abbrev_v1(),
        None,
    );
    assert_eq!(ranked[0].text, "òa");
    assert_eq!(ranked[1].text, "ệa");

    let scored = vec![
        Candidate {
            id: 1,
            text: "ệa".to_string(),
            source: CandidateSource::Abbreviation,
            evidence: "hi".to_string(),
            base_score: 0.9,
            final_score: 0.0,
        },
        Candidate {
            id: 2,
            text: "òa".to_string(),
            source: CandidateSource::Abbreviation,
            evidence: "lo".to_string(),
            base_score: 0.8,
            final_score: 0.0,
        },
    ];
    let ranked = rank(
        scored,
        &EmptyModel::new(),
        0,
        &ScoreConfig::abbrev_v1(),
        None,
    );
    assert_eq!(ranked[0].text, "ệa");
}

#[test]
fn cold_start_abbrev_is_suggestion_only() {
    let generator = AbbrevGenerator::from_seed();
    let slice = run_correction_slice(
        &snapshot("ko"),
        &LeftContext::default(),
        InputContext::default(),
        &[&generator],
        InputMethod::Telex,
        &EmptyModel::new(),
        0,
        &ScoreConfig::abbrev_v1(),
        &DecisionConfig::default(),
    );
    assert_eq!(slice.decision, Some(DecisionState::Suggest));
    assert_ne!(slice.decision, Some(DecisionState::Auto));
    assert_eq!(
        slice.action,
        Some(EngineAction::ShowSuggestions {
            revision: 1,
            candidates: slice.candidates.clone(),
        })
    );
}

struct PromotedVniModel;

impl ModelView for PromotedVniModel {
    fn confidence(&self, key: &RuleContextKey, _evaluate_at_ms: i64) -> f64 {
        assert_eq!(key.input_method, InputMethod::Vni);
        assert_eq!(key.source_rule_id, "strong:rule");
        0.95
    }

    fn positive_mass(&self, key: &RuleContextKey, _evaluate_at_ms: i64) -> f64 {
        assert_eq!(key.input_method, InputMethod::Vni);
        assert_eq!(key.source_rule_id, "strong:rule");
        18.0
    }

    fn state(&self, key: &RuleContextKey, _evaluate_at_ms: i64) -> DecisionState {
        assert_eq!(key.input_method, InputMethod::Vni);
        assert_eq!(key.source_rule_id, "strong:rule");
        DecisionState::Suggest
    }
}

struct DuplicateRuleGenerator;

impl Generator for DuplicateRuleGenerator {
    fn source(&self) -> CandidateSource {
        CandidateSource::Abbreviation
    }

    fn generate(
        &self,
        _snapshot: &CompositionSnapshot,
        _left_context: &LeftContext,
    ) -> Vec<Candidate> {
        vec![
            Candidate {
                id: 1,
                text: "không".to_string(),
                source: CandidateSource::Abbreviation,
                evidence: "strong:rule".to_string(),
                base_score: 0.95,
                final_score: 0.0,
            },
            Candidate {
                id: 2,
                text: "không".to_string(),
                source: CandidateSource::Fuzzy,
                evidence: "alt:weak".to_string(),
                base_score: 0.80,
                final_score: 0.0,
            },
        ]
    }
}

#[test]
fn read_only_correction_uses_caller_method_and_winning_rule_but_degrades_auto() {
    let slice = run_correction_slice(
        &snapshot("ko"),
        &LeftContext::default(),
        InputContext::default(),
        &[&DuplicateRuleGenerator],
        InputMethod::Vni,
        &PromotedVniModel,
        0,
        &ScoreConfig::abbrev_v1(),
        &DecisionConfig::default(),
    );

    assert_eq!(slice.decision, Some(DecisionState::Suggest));
    assert!(matches!(
        slice.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));
    assert_eq!(slice.candidates[0].source, CandidateSource::Abbreviation);
    assert_eq!(
        slice.candidates[0].evidence.split('+').next(),
        Some("strong:rule")
    );
}

#[test]
fn hysteresis_ignore_to_suggest_at_070_and_back_below_060() {
    let config = DecisionConfig::default();
    assert_eq!(
        decide(
            DecisionState::Ignore,
            0.70,
            0.5,
            0.0,
            ActionCap::Auto,
            &config
        ),
        DecisionState::Suggest
    );
    assert_eq!(
        decide(
            DecisionState::Ignore,
            0.69,
            0.5,
            0.0,
            ActionCap::Auto,
            &config
        ),
        DecisionState::Ignore
    );
    assert_eq!(
        decide(
            DecisionState::Suggest,
            0.59,
            0.5,
            0.0,
            ActionCap::Auto,
            &config
        ),
        DecisionState::Ignore
    );
    assert_eq!(
        decide(
            DecisionState::Suggest,
            0.60,
            0.5,
            0.0,
            ActionCap::Auto,
            &config
        ),
        DecisionState::Suggest
    );
}

#[test]
fn auto_hysteresis_uses_085_floor_not_promote_threshold() {
    let config = DecisionConfig::default();
    assert_eq!(
        decide(
            DecisionState::Auto,
            0.95,
            0.90,
            20.0,
            ActionCap::Auto,
            &config
        ),
        DecisionState::Auto
    );
    assert_eq!(
        decide(
            DecisionState::Auto,
            0.95,
            0.84,
            20.0,
            ActionCap::Auto,
            &config
        ),
        DecisionState::Suggest
    );
    assert_eq!(
        decide(
            DecisionState::Suggest,
            0.90,
            0.95,
            18.0,
            ActionCap::Auto,
            &config
        ),
        DecisionState::Auto
    );
    assert_eq!(
        decide(
            DecisionState::Ignore,
            0.99,
            0.99,
            18.0,
            ActionCap::Auto,
            &config
        ),
        DecisionState::Suggest
    );
}

#[test]
fn allow_transform_false_skips_generate_rank_and_decide() {
    let generator = AbbrevGenerator::from_seed();
    let context = InputContext {
        allow_transform: false,
        allow_learning: false,
    };
    let slice = run_correction_slice(
        &snapshot("ko"),
        &LeftContext::default(),
        context,
        &[&generator],
        InputMethod::Telex,
        &EmptyModel::new(),
        0,
        &ScoreConfig::abbrev_v1(),
        &DecisionConfig::default(),
    );
    assert!(slice.candidates.is_empty());
    assert_eq!(slice.decision, None);
}

#[test]
fn seed_fixture_is_the_only_abbrev_table() {
    const EXPECTED_SEED: &str = include_str!("fixtures/abbrev_seed.jsonl");
    let generator = AbbrevGenerator::from_seed();
    let mut seen = 0_u32;
    for line in EXPECTED_SEED.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let input = row["input_nfc"].as_str().unwrap();
        let expansion = row["expansion_nfc"].as_str().unwrap();
        let evidence = row["evidence"].as_str().unwrap();
        let score = row["base_score"].as_f64().unwrap();
        let cands = generator.generate(&snapshot(input), &LeftContext::default());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].text, expansion);
        assert_eq!(cands[0].evidence, evidence);
        assert!((cands[0].base_score - score).abs() < f64::EPSILON);
        seen += 1;
    }
    assert_eq!(seen, 2);
    assert_eq!(ABBREV_SEED_JSONL, EXPECTED_SEED);
    let actual_hash = hex::encode(Sha256::digest(ABBREV_SEED_JSONL.as_bytes()));
    assert_eq!(actual_hash, ABBREV_SEED_SHA256);
    let config = ScoreConfig::abbrev_v1();
    assert_eq!(config.calibration_source_hash, actual_hash);
    assert_eq!(config.hash, SCORE_CONFIG_V1_HASH);
    let canonical =
        format!("version=1\nabbrev_seed_sha256={ABBREV_SEED_SHA256}\npersonal_weight=0.2\n");
    assert_eq!(
        hex::encode(Sha256::digest(canonical.as_bytes())),
        SCORE_CONFIG_V1_HASH
    );
}
