//! Golden ChartSnapshot JSON and guard semantics — Lát 8.

use openvikey_core::chart::{
    ChartAssessment, ChartMarker, ChartSnapshot, ChartStateBand, MAX_CHART_EVENTS,
};
use openvikey_core::correction_memory::{CorrectionEvidence, CorrectionMemory};
use openvikey_core::decision::DecisionState;
use openvikey_core::intervention::{CorrectionIdentity, ScoreBreakdown};
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
    let expected = format!(
        "{{\"identity\":{{\"input_method\":\"Telex\",\"source\":\"Fuzzy\",\"original_nfc\":\"khogn\",\"candidate_nfc\":\"không\",\"source_rule_id\":\"fuzzy:khogn\"}},\"points\":[{{\"at_ms\":5000,\"confidence\":{confidence},\"blended_confidence\":{confidence},\"stored_state\":null,\"state_band\":\"unavailable\",\"marker\":\"accept\"}}],\"compaction_marker_at_ms\":null,\"breakdown\":null,\"assessment_at_ms\":null,\"current_state_band\":\"observe\",\"conclusion\":\"Đang quan sát\",\"config_hash\":\"{}\"}}",
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
    for (index, point) in snapshot.points.iter().enumerate() {
        let support = f64::from(u32::try_from(index + 1).unwrap());
        let expected = (support + 1.0) / (support + 2.0);
        assert!((point.confidence - expected).abs() < 1e-12);
        assert!((point.blended_confidence - expected).abs() < 1e-12);
        assert_eq!(point.marker, Some(ChartMarker::Accept));
        assert_eq!(point.state_band, ChartStateBand::Unavailable);
    }
}

#[test]
fn stored_auto_is_not_reassessed_without_an_authentic_planner_result() {
    let (memory, identity) = learned_auto_memory();
    let config = LearningConfigV2::compatibility_v1();

    let fresh = ChartSnapshot::from_memory(&memory, &identity, &config, 0).unwrap();
    assert_eq!(fresh.conclusion, "Có thể tự sửa");
    assert_eq!(fresh.state_band(), ChartStateBand::Auto);

    // A chart-only projection must not become a second decision engine. Without
    // a current planner result it keeps the guarded persisted state.
    let decayed =
        ChartSnapshot::from_memory(&memory, &identity, &config, 3 * HALF_LIFE_MS).unwrap();
    assert_eq!(decayed.points[0].stored_state, None);
    assert_eq!(decayed.conclusion, "Có thể tự sửa");
    assert_eq!(decayed.state_band(), ChartStateBand::Auto);
}

#[test]
fn revert_veto_presents_cooldown_band() {
    let identity = khogn_identity();
    let key = openvikey_core::model::RuleContextKey {
        input_method: identity.input_method,
        source: identity.source,
        original_nfc: identity.original_nfc.clone(),
        candidate_nfc: identity.candidate_nfc.clone(),
        left_token_nfc: None,
        source_rule_id: identity.source_rule_id.clone(),
    };
    let mut model = openvikey_core::model::AdaptiveModel::default();
    for seq in 1..=4 {
        model.apply_feedback(
            &key,
            &openvikey_core::types::FeedbackEvent {
                seq,
                at_ms: 0,
                kind: openvikey_core::types::FeedbackKind::Accept { candidate_id: 1 },
            },
            true,
        );
    }
    model.record_auto_emission(&key, 1, 10, true);
    model.record_auto_emission(&key, 2, 11, true);
    model.record_immediate_revert(&key, 1, 100, true);
    model.record_immediate_revert(&key, 2, 101, true);
    let config = LearningConfigV2::compatibility_v1();
    let snapshot =
        ChartSnapshot::from_memory(model.correction_memory(), &identity, &config, 200).unwrap();
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
    let historical_confidence_before = before.points.last().unwrap().confidence;
    let confidence_before = memory.blended_confidence(&identity, None, evaluate_at_ms, 2.0);

    memory.compact_at(evaluate_at_ms, MAX_CHART_EVENTS);

    let after = ChartSnapshot::from_memory(&memory, &identity, &config, evaluate_at_ms).unwrap();
    assert_eq!(after.compaction_marker_at_ms, Some(evaluate_at_ms));
    assert_eq!(after.points.len(), MAX_CHART_EVENTS);
    assert_eq!(after.points[0].at_ms, 7);
    let confidence_after = memory.blended_confidence(&identity, None, evaluate_at_ms, 2.0);
    assert!((confidence_after - confidence_before).abs() < 1e-9);
    assert!((after.points.last().unwrap().confidence - historical_confidence_before).abs() < 1e-9);
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

#[test]
fn timeline_is_strictly_causal_across_future_events() {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    let config = LearningConfigV2::compatibility_v1();

    // Event 3 at t=100. Later timestamps deliberately use lower sequences to
    // prove the chart cutoff follows the full event ordering, not seq alone.
    accept(&mut memory, &identity, 3, 100);
    let snapshot_1 = ChartSnapshot::from_memory(&memory, &identity, &config, 100).unwrap();
    let conf_at_100_before = snapshot_1.points[0].confidence;

    // Event 1 at t=200, Event 2 at t=300.
    accept(&mut memory, &identity, 1, 200);
    accept(&mut memory, &identity, 2, 300);

    let snapshot_3 = ChartSnapshot::from_memory(&memory, &identity, &config, 300).unwrap();
    let conf_at_100_after = snapshot_3.points[0].confidence;

    // Confidence at historical point t=100 must be unchanged by events at t=200 and t=300.
    assert!((conf_at_100_before - conf_at_100_after).abs() < f64::EPSILON);
}

#[test]
fn authentic_assessment_is_preserved_verbatim() {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    accept(&mut memory, &identity, 1, 100);
    let config = LearningConfigV2::compatibility_v1();
    let breakdown = ScoreBreakdown {
        generator_base: 0.71,
        exact_correction: 0.04,
        unigram: 0.02,
        bigram: 0.03,
        recent_revert_penalty: 0.11,
        top1_top2_margin: 0.07,
        final_score: 0.69,
    };
    let assessment = ChartAssessment {
        breakdown: breakdown.clone(),
        state_band: ChartStateBand::Suggest,
        evaluated_at_ms: 95,
    };

    let snapshot = ChartSnapshot::from_memory_with_context(
        &memory,
        &identity,
        &config,
        100,
        None,
        Some(&assessment),
    )
    .unwrap();

    assert_eq!(snapshot.breakdown, Some(breakdown));
    assert_eq!(snapshot.assessment_at_ms, Some(95));
    assert_eq!(snapshot.state_band(), ChartStateBand::Suggest);
}

#[test]
fn private_context_changes_blended_line_without_entering_snapshot() {
    let identity = khogn_identity();
    let mut memory = CorrectionMemory::default();
    memory.apply(
        &identity,
        Some("Việt"),
        CorrectionEvidence {
            seq: 1,
            at_ms: 100,
            positive: 1.0,
            negative: 0.0,
        },
    );
    memory.apply(
        &identity,
        Some("Anh"),
        CorrectionEvidence {
            seq: 2,
            at_ms: 200,
            positive: 0.0,
            negative: 1.0,
        },
    );
    let config = LearningConfigV2::compatibility_v1();

    let snapshot = ChartSnapshot::from_memory_with_context(
        &memory,
        &identity,
        &config,
        200,
        Some("Việt"),
        None,
    )
    .unwrap();

    assert!(snapshot.points[1].blended_confidence > snapshot.points[1].confidence);
    assert!(!serde_json::to_string(&snapshot).unwrap().contains("Việt"));
}
