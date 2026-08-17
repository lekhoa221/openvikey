//! M9 corpus runner: pinned inputs produce deterministic machine-readable evidence.

use openvikey_lab::corpus::{ErrorKind, EvaluationMode};
use openvikey_lab::report::evaluate_manifest;
use openvikey_lab::report::{AUTO_PRECISION_FORMULA, CORRECT_TOKEN_FPR_FORMULA};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn held_out_report_uses_locked_denominators_and_is_byte_deterministic() {
    let workspace = workspace_root();
    let manifest = workspace.join("data/corpus-manifest.toml");

    let first = evaluate_manifest(&manifest, &workspace, EvaluationMode::Unit)
        .expect("authored corpus evaluates");
    let second =
        evaluate_manifest(&manifest, &workspace, EvaluationMode::Unit).expect("repeat evaluates");

    assert_eq!(
        first.to_pretty_json().unwrap(),
        second.to_pretty_json().unwrap()
    );
    assert_eq!(first.counts.correct_tokens, 1);
    assert_eq!(first.counts.error_cases, 2);
    assert_eq!(first.counts.auto_true_positive, 0);
    assert_eq!(first.counts.auto_false_positive, 0);
    assert_eq!(first.counts.suggestion_top1, 2);
    assert_eq!(first.counts.suggestion_top3, 2);
    assert_eq!(first.metrics.auto_precision.formula, AUTO_PRECISION_FORMULA);
    assert_eq!(first.metrics.auto_precision.denominator, 0);
    assert_eq!(
        first.metrics.correct_token_fpr.formula,
        CORRECT_TOKEN_FPR_FORMULA
    );
    assert_eq!(first.metrics.correct_token_fpr.denominator, 1);
    assert_eq!(first.metrics.correct_token_fpr.point_estimate, Some(0.0));
    assert!(!first.release_gates.sample_floors);
    assert!(!first.release_gates.all_pass);
}

#[test]
fn report_contains_frozen_corpus_lexicon_and_config_hashes() {
    let workspace = workspace_root();
    let report = evaluate_manifest(
        &workspace.join("data/corpus-manifest.toml"),
        &workspace,
        EvaluationMode::Unit,
    )
    .unwrap();

    assert_eq!(report.schema_version, 1);
    assert_eq!(report.corpus.version, "1.0.0");
    assert_eq!(report.corpus.manifest_sha256.len(), 64);
    assert_eq!(report.corpus.lexicon_sha256.len(), 64);
    assert_eq!(report.config.score_sha256.len(), 64);
    assert_eq!(report.config.decision_sha256.len(), 64);
    assert_eq!(report.taxonomy.len(), 4);
    let transpose = &report.taxonomy[&ErrorKind::Transpose];
    assert_eq!(transpose.counts.labeled_errors, 1);
    assert_eq!(transpose.metrics.auto_recall.denominator, 1);
    assert_eq!(
        transpose.metrics.candidate_coverage.point_estimate,
        Some(1.0)
    );
    assert_eq!(transpose.metrics.suggestion_top1.point_estimate, Some(1.0));
}
