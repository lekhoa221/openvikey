//! Golden ChartSnapshot JSON and guard semantics — Lát 8.

use openvikey_core::chart::{ChartMarker, ChartSnapshot, ChartStateBand, MAX_CHART_EVENTS};
use openvikey_core::correction_memory::{CorrectionEvidence, CorrectionMemory};
use openvikey_core::decision::DecisionState;
use openvikey_core::intervention::CorrectionIdentity;
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::types::{CandidateSource, InputMethod};

const HALF_LIFE_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

fn khogn_identity() -> CorrectionIdentity {
    CorrectionIdentity {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "khogn".into(),
        candidate_nfc: "không".into(),
        source_rule_id: "fuzzy:khogn".into(),
    }
}

fn accept(memory: &mut CorrectionMemory, identity: &CorrectionIdentity, seq: u64, at_ms: i64) {
    memory.apply(
        identity,
        None,
        CorrectionEvidence {
            seq,
            at_ms,
            positive: 1.0,
            negative: 0.0,
        },
    );
}

fn learned_auto_memory() -> (CorrectionMemory, CorrectionIdentity) {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    for seq in 1..=18 {
        accept(&mut memory, &identity, seq, 0);
    }
    memory.record_state(&identity, None, DecisionState::Auto);
    (memory, identity)
}

#[test]
fn chart_snapshot_is_byte_identical_for_same_model_config_time() {
    let (memory, identity) = learned_auto_memory();
    let config = LearningConfigV2::compatibility_v1();
    let a = ChartSnapshot::from_memory(&memory, &identity, &config, 1_000).unwrap();
    let b = ChartSnapshot::from_memory(&memory, &identity, &config, 1_000).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        serde_json::to_vec(&a).unwrap(),
        serde_json::to_vec(&b).unwrap()
    );
}

#[test]
fn golden_json_shape_is_stable_for_one_accept() {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    accept(&mut memory, &identity, 1, 5_000);
    let config = LearningConfigV2::compatibility_v1();
    let snapshot = ChartSnapshot::from_memory(&memory, &identity, &config, 5_000).unwrap();
    let json = serde_json::to_string(&snapshot).unwrap();
    // One accept of mass 1.0: confidence = (1+1)/(2+1) = 2/3.
    let confidence = 2.0_f64 / 3.0;
    let exact = 0.2_f64 * (confidence - 0.5);
    let expected = format!(
        "{{\"identity\":{{\"input_method\":\"Telex\",\"source\":\"Fuzzy\",\"original_nfc\":\"khogn\",\"candidate_nfc\":\"không\",\"source_rule_id\":\"fuzzy:khogn\"}},\"points\":[{{\"at_ms\":5000,\"confidence\":{confidence},\"blended_confidence\":{confidence},\"stored_state\":\"Ignore\",\"state_band\":\"observe\",\"marker\":\"accept\"}}],\"compaction_marker_at_ms\":null,\"breakdown\":{{\"generator_base\":0.0,\"exact_correction\":{exact},\"unigram\":0.0,\"bigram\":0.0,\"recent_revert_penalty\":0.0,\"top1_top2_margin\":1.0,\"final_score\":{confidence}}},\"conclusion\":\"Đang quan sát\",\"config_hash\":\"{}\"}}",
        config.hash()
    );
    assert_eq!(json, expected);
}

#[test]
fn confidence_line_matches_model_query_at_each_recent_event() {
    let (memory, identity) = learned_auto_memory();
    let config = LearningConfigV2::compatibility_v1();
    let snapshot = ChartSnapshot::from_memory(&memory, &identity, &config, 0).unwrap();
    assert_eq!(snapshot.points.len(), 18);
    for point in &snapshot.points {
        let queried = memory.blended_confidence(&identity, None, point.at_ms, 2.0);
        assert!((point.confidence - queried).abs() < 1e-12);
        assert!((point.blended_confidence - queried).abs() < 1e-12);
        assert_eq!(point.marker, Some(ChartMarker::Accept));
    }
}

#[test]
fn migrated_stored_auto_is_presented_as_effective_suggest_when_guards_fail() {
    let (memory, identity) = learned_auto_memory();
    let config = LearningConfigV2::compatibility_v1();

    let fresh = ChartSnapshot::from_memory(&memory, &identity, &config, 0).unwrap();
    assert_eq!(fresh.conclusion, "Có thể tự sửa");
    assert_eq!(fresh.state_band(), ChartStateBand::Auto);

    // Three half-lives later the decayed confidence drops below
    // auto_off_confidence, so the stored Auto presents as Suggest.
    let decayed =
        ChartSnapshot::from_memory(&memory, &identity, &config, 3 * HALF_LIFE_MS).unwrap();
    assert_eq!(decayed.points[0].stored_state, DecisionState::Auto);
    assert_eq!(decayed.conclusion, "Gợi ý");
    assert_eq!(decayed.state_band(), ChartStateBand::Suggest);
}

#[test]
fn revert_veto_presents_cooldown_band() {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    for seq in 1..=4 {
        accept(&mut memory, &identity, seq, 0);
    }
    memory.record_state(&identity, None, DecisionState::Suggest);
    memory.record_auto_emission(&identity, None, 1, 10, 10);
    memory.record_auto_emission(&identity, None, 2, 11, 10);
    memory.record_immediate_revert(&identity, None, 1, 100);
    memory.record_immediate_revert(&identity, None, 2, 101);
    let config = LearningConfigV2::compatibility_v1();
    let snapshot = ChartSnapshot::from_memory(&memory, &identity, &config, 200).unwrap();
    assert_eq!(snapshot.state_band(), ChartStateBand::Cooldown);
    assert_eq!(snapshot.conclusion, "Tạm dừng tự sửa");
}

#[test]
fn compaction_inserts_checkpoint_marker_without_changing_final_confidence() {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    for seq in 1..=70u64 {
        accept(
            &mut memory,
            &identity,
            seq,
            i64::from(u32::try_from(seq).unwrap()),
        );
    }
    let evaluate_at_ms = 100_000;
    let config = LearningConfigV2::compatibility_v1();
    let before = ChartSnapshot::from_memory(&memory, &identity, &config, evaluate_at_ms).unwrap();
    let confidence_before = before.breakdown.final_score;

    memory.compact_at(evaluate_at_ms, MAX_CHART_EVENTS);

    let after = ChartSnapshot::from_memory(&memory, &identity, &config, evaluate_at_ms).unwrap();
    assert_eq!(after.compaction_marker_at_ms, Some(evaluate_at_ms));
    assert_eq!(after.points.len(), MAX_CHART_EVENTS);
    assert_eq!(after.points[0].at_ms, 7);
    assert!((after.breakdown.final_score - confidence_before).abs() < 1e-9);
}

#[test]
fn physical_forget_makes_chart_none() {
    let (memory, identity) = learned_auto_memory();
    let config = LearningConfigV2::compatibility_v1();
    assert!(ChartSnapshot::from_memory(&memory, &identity, &config, 0).is_some());

    let mut forgotten = memory;
    assert!(forgotten.forget_identity(&identity));
    assert!(ChartSnapshot::from_memory(&forgotten, &identity, &config, 0).is_none());
}

#[test]
fn snapshot_contains_no_left_context_strings_by_default() {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    accept(&mut memory, &identity, 1, 0);
    memory.apply(
        &identity,
        Some("Việt"),
        CorrectionEvidence {
            seq: 2,
            at_ms: 1,
            positive: 1.0,
            negative: 0.0,
        },
    );
    let config = LearningConfigV2::compatibility_v1();
    let snapshot = ChartSnapshot::from_memory(&memory, &identity, &config, 1).unwrap();
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(!json.contains("Việt"));
    assert!(!json.contains("left"));
}

#[test]
fn markers_map_deltas_to_accept_reject_revert_and_weak_settle() {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    let rows = [
        (1, 0, 1.0, 0.0, ChartMarker::Accept),
        (2, 1, 1.5, 0.0, ChartMarker::Accept),
        (3, 2, 0.0, 1.0, ChartMarker::Reject),
        (4, 3, 0.0, 1.5, ChartMarker::Revert),
        (5, 4, 0.3, 0.0, ChartMarker::WeakSettle),
    ];
    for (seq, at_ms, positive, negative, _) in rows {
        memory.apply(
            &identity,
            None,
            CorrectionEvidence {
                seq,
                at_ms,
                positive,
                negative,
            },
        );
    }
    let config = LearningConfigV2::compatibility_v1();
    let snapshot = ChartSnapshot::from_memory(&memory, &identity, &config, 4).unwrap();
    for (point, (_, _, _, _, expected)) in snapshot.points.iter().zip(rows.iter()) {
        assert_eq!(point.marker, Some(*expected));
    }
}
