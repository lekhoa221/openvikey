//! Milestone 5: abbreviation generate → rank → decision.

use openvikey_core::decision::{ActionCap, DecisionConfig, DecisionState, decide};
use openvikey_core::generate::abbrev::{ABBREV_SEED_JSONL, ABBREV_SEED_SHA256, AbbrevGenerator};
use openvikey_core::generate::{Generator, LeftContext, run_correction_slice};
use openvikey_core::model::EmptyModel;
use openvikey_core::rank::{ScoreConfig, rank};
use openvikey_core::types::{Candidate, CandidateSource, CompositionSnapshot, InputContext};

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
    let ranked = rank(cands, &EmptyModel::new(), 0, &ScoreConfig::abbrev_v1());
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
    let ranked = rank(cands, &EmptyModel::new(), 0, &ScoreConfig::abbrev_v1());
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
    let ranked = rank(scored, &EmptyModel::new(), 0, &ScoreConfig::abbrev_v1());
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
        &EmptyModel::new(),
        0,
        &ScoreConfig::abbrev_v1(),
        &DecisionConfig::default(),
    );
    assert_eq!(slice.decision, Some(DecisionState::Suggest));
    assert_ne!(slice.decision, Some(DecisionState::Auto));
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
    let generator = AbbrevGenerator::from_seed();
    let mut seen = 0_u32;
    for line in ABBREV_SEED_JSONL.lines() {
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
    assert_eq!(ScoreConfig::abbrev_v1().hash, ABBREV_SEED_SHA256);
}
