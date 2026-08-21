//! Exact-correction memory v2 contracts.

use openvikey_core::correction_memory::{CorrectionEvidence, CorrectionMemory};
use openvikey_core::decision::DecisionState;
use openvikey_core::intervention::CorrectionIdentity;
use openvikey_core::types::{CandidateSource, InputMethod};

fn identity() -> CorrectionIdentity {
    CorrectionIdentity {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "khogn".into(),
        candidate_nfc: "không".into(),
        source_rule_id: "fuzzy:khogn->không".into(),
    }
}

fn evidence(seq: u64, positive: f64, negative: f64) -> CorrectionEvidence {
    CorrectionEvidence {
        seq,
        at_ms: 0,
        positive,
        negative,
    }
}

#[test]
fn empty_context_uses_global_confidence() {
    let id = identity();
    let mut memory = CorrectionMemory::default();
    memory.apply(&id, None, evidence(1, 1.0, 0.0));

    let global = memory.blended_confidence(&id, None, 0, 2.0);
    let with_unseen_left = memory.blended_confidence(&id, Some("Việt"), 0, 2.0);

    assert!((global - 2.0 / 3.0).abs() < 1e-12);
    assert!((global - with_unseen_left).abs() < 1e-12);
}

#[test]
fn low_support_context_shrinks_toward_global() {
    let id = identity();
    let mut memory = CorrectionMemory::default();
    for seq in 1..=9 {
        memory.apply(&id, None, evidence(seq, 1.0, 0.0));
    }
    memory.apply(&id, Some("tôi"), evidence(10, 0.0, 1.0));

    let global = memory.blended_confidence(&id, None, 0, 2.0);
    let context_only = 1.0_f64 / 3.0;
    let blended = memory.blended_confidence(&id, Some("tôi"), 0, 2.0);

    assert!((global - 10.0 / 12.0).abs() < 1e-12);
    assert!((blended - global).abs() < (context_only - global).abs());
    assert!(blended < global);
}

#[test]
fn high_support_context_can_diverge_from_global() {
    let id = identity();
    let mut memory = CorrectionMemory::default();
    for seq in 1..=20 {
        memory.apply(&id, Some("anh"), evidence(seq, 1.0, 0.0));
    }
    for seq in 21..=30 {
        memory.apply(&id, Some("tôi"), evidence(seq, 0.0, 1.0));
    }

    let global = memory.blended_confidence(&id, None, 0, 2.0);
    let negative_context = memory.blended_confidence(&id, Some("tôi"), 0, 2.0);

    assert!(global > 0.60);
    assert!(negative_context < 0.30);
}

#[test]
fn sibling_context_auto_does_not_force_other_context_auto() {
    let id = identity();
    let mut memory = CorrectionMemory::default();
    memory.record_state(&id, Some("Việt"), DecisionState::Auto);

    assert_eq!(memory.query_state(&id, Some("Việt")), DecisionState::Auto);
    assert_eq!(memory.query_state(&id, Some("Nam")), DecisionState::Ignore);
    assert_eq!(memory.query_state(&id, None), DecisionState::Ignore);
}

#[test]
fn source_cap_prevents_personal_auto_state() {
    let mut id = identity();
    id.source = CandidateSource::Personal;
    let mut memory = CorrectionMemory::default();
    memory.record_state(&id, None, DecisionState::Auto);

    assert_eq!(memory.query_state(&id, None), DecisionState::Suggest);
}
