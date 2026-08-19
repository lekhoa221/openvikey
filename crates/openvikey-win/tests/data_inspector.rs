//! GĐ2b development Data Inspector contract.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use openvikey_core::decision::DecisionState;
use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::{AdaptiveModel, RuleContextKey};
use openvikey_core::types::{CandidateSource, FeedbackEvent, FeedbackKind, InputMethod};
use openvikey_session::session::{LabSession, SessionCursors};
use openvikey_win::persist::{
    InspectStoreError, InspectionFilter, inspect_open_personal_store, save_open_snapshot,
};

fn test_dir(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gd2b-data-inspector-tests")
        .join(format!("{}-{case}", std::process::id()))
}

fn learned_store(dir: &Path) -> (PathBuf, PathBuf) {
    fs::create_dir_all(dir).unwrap();
    let model_path = dir.join("model.ovkdev.json");
    let capture_path = dir.join("capture.ovkdev.json");
    let mut model = AdaptiveModel::default();
    let key = RuleContextKey {
        input_method: InputMethod::Vni,
        source: CandidateSource::Fuzzy,
        original_nfc: "paht1".into(),
        candidate_nfc: "phát".into(),
        left_token_nfc: Some("đã".into()),
        source_rule_id: "fuzzy-edit".into(),
    };
    model.apply_feedback(
        &key,
        &FeedbackEvent {
            seq: 7,
            at_ms: 1_723_456,
            kind: FeedbackKind::Accept { candidate_id: 12 },
        },
        true,
    );
    model.record_decision(&key, DecisionState::Suggest, true);
    let session = LabSession::new_with_model(
        EngineConfig::default(),
        Lexicon::from_entries([], [], None),
        model,
        SessionCursors::default(),
    );
    save_open_snapshot(&session.save_snapshot(), &model_path, &capture_path).unwrap();
    (model_path, capture_path)
}

#[test]
fn opening_filtering_and_refreshing_are_byte_for_byte_read_only() {
    let (model_path, capture_path) = learned_store(&test_dir("readonly"));
    let before_model = fs::read(&model_path).unwrap();
    let before_capture = fs::read(&capture_path).unwrap();

    let filter = InspectionFilter {
        original: Some("PAHT".into()),
        source: Some("fuzzy".into()),
        left_token: Some("đã".into()),
        ..Default::default()
    };
    let first = inspect_open_personal_store(&model_path, &capture_path, &filter).unwrap();
    let refreshed = inspect_open_personal_store(&model_path, &capture_path, &filter).unwrap();

    assert_eq!(first, refreshed);
    assert_eq!(first.summary.model_rows, 1);
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0].candidate_nfc, "phát");
    assert_eq!(first.rows[0].evidence_count, 1);
    assert_eq!(first.rows[0].last_evidence_at_ms, Some(1_723_456));
    assert_eq!(fs::read(&model_path).unwrap(), before_model);
    assert_eq!(fs::read(&capture_path).unwrap(), before_capture);
}

#[test]
fn missing_and_provenance_mismatch_are_clear_fail_closed_errors() {
    let dir = test_dir("errors");
    fs::create_dir_all(&dir).unwrap();
    let missing = inspect_open_personal_store(
        &dir.join("missing-model.ovkdev.json"),
        &dir.join("missing-capture.ovkdev.json"),
        &InspectionFilter::default(),
    )
    .unwrap_err();
    assert!(matches!(missing, InspectStoreError::MissingModel(_)));

    let (model_path, capture_path) = learned_store(&dir.join("mismatch"));
    let mut model: serde_json::Value =
        serde_json::from_slice(&fs::read(&model_path).unwrap()).unwrap();
    model["config"]["half_life_ms"] = 123.into();
    fs::write(&model_path, serde_json::to_vec(&model).unwrap()).unwrap();
    let mismatch =
        inspect_open_personal_store(&model_path, &capture_path, &InspectionFilter::default())
            .unwrap_err();
    assert!(matches!(mismatch, InspectStoreError::InvalidPair(_)));
    assert!(mismatch.to_string().contains("provenance"));
}

#[test]
fn explicit_path_cli_prints_summary_rows_and_refresh_hint() {
    let (model_path, capture_path) = learned_store(&test_dir("cli"));
    let output = Command::new(env!("CARGO_BIN_EXE_openvikey-data-inspector"))
        .args([
            "--model",
            model_path.to_str().unwrap(),
            "--capture",
            capture_path.to_str().unwrap(),
            "--original",
            "paht1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("model rows: 1"));
    assert!(stdout.contains("paht1 -> phát"));
    assert!(stdout.contains("Run again to refresh"));
}
