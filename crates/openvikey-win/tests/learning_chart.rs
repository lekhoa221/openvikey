//! Settings Learning chart: snapshot, text alternative, overview, hook-path
//! isolation — Lát 8.
//!
//! `RUNTIME` binds once per process, so every test shares one bound host that
//! is created, bound, and seeded exactly once through [`shared_runtime`]. That
//! keeps the tests order-independent under the parallel libtest harness.

use std::sync::{Arc, Mutex, OnceLock};

use openvikey_core::decision::DecisionState;
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::model::RuleContextKey;
use openvikey_core::types::{CandidateSource, FeedbackEvent, FeedbackKind, InputMethod};
use openvikey_win::chart_view::text_alternative;
use openvikey_win::focus::FocusCache;
use openvikey_win::host::{TypingHost, bind_runtime, control_snapshot, rule_chart_runtime};

fn key(
    original: &str,
    candidate: &str,
    source: CandidateSource,
    rule_id: &str,
    left: Option<&str>,
) -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source,
        original_nfc: original.into(),
        candidate_nfc: candidate.into(),
        left_token_nfc: left.map(ToOwned::to_owned),
        source_rule_id: rule_id.into(),
    }
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

fn accept_at(seq: u64, at_ms: i64) -> FeedbackEvent {
    FeedbackEvent {
        seq,
        at_ms,
        kind: FeedbackKind::Accept { candidate_id: seq },
    }
}

fn reject_at(seq: u64, at_ms: i64) -> FeedbackEvent {
    FeedbackEvent {
        seq,
        at_ms,
        kind: FeedbackKind::ExplicitReject { candidate_id: seq },
    }
}

fn seed_learning(host: &Arc<Mutex<TypingHost>>) {
    let now = current_time_ms();
    let mut guard = host.lock().unwrap();
    let model = guard.session.model_mut();

    // Stored Auto with passing guards -> effective Auto.
    let auto_rule = key(
        "khogn",
        "không",
        CandidateSource::Fuzzy,
        "fuzzy:khogn",
        None,
    );
    for seq in 1..=18u64 {
        model.apply_feedback(
            &auto_rule,
            &accept_at(seq, now - 1000 + i64::try_from(seq).unwrap()),
            true,
        );
    }
    model.record_decision(&auto_rule, DecisionState::Auto, true);

    // Stored Suggest -> effective Suggest.
    let suggest_rule = key("chao", "chào", CandidateSource::Fuzzy, "fuzzy:chao", None);
    for seq in 19..=21u64 {
        model.apply_feedback(
            &suggest_rule,
            &accept_at(seq, now - 1000 + i64::try_from(seq).unwrap()),
            true,
        );
    }
    model.record_decision(&suggest_rule, DecisionState::Suggest, true);

    // Rejected evidence never promotes state -> effective Observe.
    let observe_rule = key(
        "khong",
        "không",
        CandidateSource::Diacritics,
        "diacritics:unigram",
        None,
    );
    model.apply_feedback(&observe_rule, &reject_at(22, now - 1000 + 22), true);

    // Two recent reverts arm the demotion veto -> effective Cooldown even
    // though the raw stored state is plain Suggest.
    let cooldown_rule = key("nham", "nhầm", CandidateSource::Fuzzy, "fuzzy:nham", None);
    for seq in 23..=26u64 {
        model.apply_feedback(
            &cooldown_rule,
            &accept_at(seq, now - 500 + i64::try_from(seq).unwrap()),
            true,
        );
    }
    model.record_auto_emission(&cooldown_rule, 1, now - 400, true);
    model.record_auto_emission(&cooldown_rule, 2, now - 300, true);
    model.record_immediate_revert(&cooldown_rule, 1, 100, true);
    model.record_immediate_revert(&cooldown_rule, 2, 101, true);

    // A context-scoped row used by the privacy assertions. Kept early in time
    // so the most-recent rule stays the cooldown rule above.
    let contextual = key(
        "ko",
        "không",
        CandidateSource::Fuzzy,
        "fuzzy-ko",
        Some("một"),
    );
    model.apply_feedback(&contextual, &accept_at(5, now - 2000), true);
}

fn shared_runtime() -> &'static Arc<Mutex<TypingHost>> {
    static HOST: OnceLock<Arc<Mutex<TypingHost>>> = OnceLock::new();
    HOST.get_or_init(|| {
        let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
        bind_runtime(Arc::clone(&host), Arc::new(FocusCache::new()));
        seed_learning(&host);
        host
    })
}

#[test]
fn overview_counts_match_guarded_persisted_states_not_raw_stored_states() {
    shared_runtime();
    let snapshot = control_snapshot().unwrap();
    let overview = &snapshot.learning_overview;
    assert_eq!(overview.observe, 1);
    assert_eq!(overview.suggest, 2);
    assert_eq!(overview.auto, 1);
    assert_eq!(overview.cooldown, 1);
    assert_eq!(overview.total(), snapshot.learned_rows.len());
    assert_eq!(overview.model_rows, snapshot.learned_rows.len());
    assert_eq!(
        overview.confidence_buckets.iter().sum::<usize>(),
        snapshot.learned_rows.len()
    );
    assert_eq!(overview.retained_interventions, 0);
    assert_eq!(overview.undone_interventions, 2);
    assert_eq!(overview.top_reverted[0].original_nfc, "nham");
    assert_eq!(overview.pruned_rows, 0);

    // Raw stored states would count three plain Suggest rows and no Cooldown
    // row; the effective bands must differ from that naive tally.
    let raw_suggest_rows = snapshot
        .learned_rows
        .iter()
        .filter(|row| row.state == DecisionState::Suggest)
        .count();
    assert_eq!(raw_suggest_rows, 3);
    assert_ne!(raw_suggest_rows, overview.suggest);

    let line = overview.vietnamese_line();
    assert!(line.contains("Quan sát 1"));
    assert!(line.contains("Gợi ý 2"));
    assert!(line.contains("Tự sửa 1"));
    assert!(line.contains("Cooldown 1"));
    assert!(line.contains("Nguồn:"));
    assert!(line.contains("Model: 5 row"));
}

#[test]
fn selecting_a_rule_exposes_chart_snapshot_and_text_alternative() {
    shared_runtime();

    let snapshot = control_snapshot().unwrap();
    let default_chart = snapshot.chart.expect("default chart for most recent rule");
    assert_eq!(default_chart.identity.original_nfc, "nham");
    assert_eq!(default_chart.conclusion, "Tạm dừng tự sửa");

    let auto_row = snapshot
        .learned_rows
        .iter()
        .find(|row| row.original_nfc == "khogn")
        .expect("auto rule row");
    let chart = rule_chart_runtime(auto_row).expect("chart");
    assert_eq!(chart.conclusion, "Có thể tự sửa");
    assert!(!chart.points.is_empty());

    let text = text_alternative(&chart);
    assert!(text.contains("Biểu đồ học tập: khogn → không · Telex"));
    assert!(text.contains("Có thể tự sửa"));
    assert!(text.contains("Phân rã điểm: chưa có đánh giá planner"));
    assert!(text.contains("Kết luận"));
}

#[test]
fn charts_use_the_bound_sessions_product_policy() {
    shared_runtime();
    let snapshot = control_snapshot().unwrap();
    let expected = LearningConfigV2::product_v2().hash();
    assert_eq!(snapshot.chart.unwrap().config_hash, expected);

    let row = snapshot.learned_rows.first().expect("learned row");
    let selected = rule_chart_runtime(row).expect("selected chart");
    assert_eq!(selected.config_hash, expected);
}

#[test]
fn chart_text_and_json_contain_no_left_context_strings() {
    shared_runtime();

    let row = control_snapshot()
        .unwrap()
        .learned_rows
        .into_iter()
        .find(|row| row.left_token_nfc.as_deref() == Some("một"))
        .expect("contextual row");
    let chart = rule_chart_runtime(&row).expect("chart");
    let text = text_alternative(&chart);
    assert!(!text.contains("một"));
    let json = serde_json::to_string(&chart).unwrap();
    assert!(!json.contains("một"));
    assert!(!json.contains("left"));
}

#[test]
fn chart_build_is_not_invoked_from_inject_path() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for source_file in ["src/hook.rs", "src/inject.rs", "src/ll.rs"] {
        let source = std::fs::read_to_string(manifest.join(source_file)).unwrap();
        for forbidden in [
            "ChartSnapshot",
            "rule_chart_runtime",
            "control_snapshot",
            "chart_view",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_file} must not reference {forbidden}"
            );
        }
    }
    let control_source = std::fs::read_to_string(manifest.join("src/control.rs")).unwrap();
    assert!(control_source.contains("rule_chart_runtime"));
}
