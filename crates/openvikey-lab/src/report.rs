//! Deterministic held-out evaluation reports with locked metric denominators.

use crate::corpus::{
    CorpusError, CorpusLabel, ErrorKind, EvaluationMode, RELEASE_MIN_CORRECT_TOKENS,
    RELEASE_MIN_ERROR_CASES, RELEASE_MIN_PER_ERROR_TYPE, build_lexicon, hash_file, load_and_verify,
};
use crate::metrics::{auto_precision, correct_token_fpr, wilson_interval};
use crate::session::LabSession;
use openvikey_core::decision::{DecisionConfig, DecisionState};
use openvikey_core::engine::EngineConfig;
use openvikey_core::rank::ScoreConfig;
use openvikey_core::types::{InputContext, InputMethod, TonePlacement};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

pub const AUTO_PRECISION_FORMULA: &str =
    "auto_true_positive / (auto_true_positive + auto_false_positive)";
pub const CORRECT_TOKEN_FPR_FORMULA: &str =
    "false_auto_replacements_on_correct / total_correct_tokens";
pub const AUTO_RECALL_FORMULA: &str = "auto_true_positive / total_labeled_errors";
pub const SUGGESTION_TOP1_FORMULA: &str = "suggestion_top1 / supported_labeled_errors";
pub const SUGGESTION_TOP3_FORMULA: &str = "suggestion_top3 / supported_labeled_errors";
pub const CANDIDATE_COVERAGE_FORMULA: &str =
    "errors_with_at_least_one_candidate / supported_labeled_errors";

#[derive(Debug, Error)]
pub enum ReportError {
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvaluationReport {
    pub schema_version: u32,
    pub mode: String,
    pub corpus: CorpusEvidence,
    pub config: ConfigEvidence,
    pub counts: EvaluationCounts,
    pub metrics: EvaluationMetrics,
    pub taxonomy: BTreeMap<ErrorKind, TaxonomyEvidence>,
    pub release_gates: ReleaseGates,
}

impl EvaluationReport {
    pub fn to_pretty_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CorpusEvidence {
    pub version: String,
    pub manifest_sha256: String,
    pub lexicon_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigEvidence {
    pub score_version: u32,
    pub score_sha256: String,
    pub decision_version: u32,
    pub decision_sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EvaluationCounts {
    pub correct_tokens: u64,
    pub error_cases: u64,
    pub supported_error_cases: u64,
    pub auto_true_positive: u64,
    pub auto_false_positive: u64,
    pub false_auto_on_correct: u64,
    pub candidate_covered: u64,
    pub suggestion_top1: u64,
    pub suggestion_top3: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricEvidence {
    pub formula: &'static str,
    pub numerator: u64,
    pub denominator: u64,
    pub point_estimate: Option<f64>,
    pub wilson_95: Option<WilsonInterval>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WilsonInterval {
    pub lower: f64,
    pub upper: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvaluationMetrics {
    pub auto_precision: MetricEvidence,
    pub correct_token_fpr: MetricEvidence,
    pub auto_recall: MetricEvidence,
    pub suggestion_top1: MetricEvidence,
    pub suggestion_top3: MetricEvidence,
    pub candidate_coverage: MetricEvidence,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TaxonomyCounts {
    pub labeled_errors: u64,
    pub auto_true_positive: u64,
    pub candidate_covered: u64,
    pub suggestion_top1: u64,
    pub suggestion_top3: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaxonomyEvidence {
    pub counts: TaxonomyCounts,
    pub metrics: TaxonomyMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaxonomyMetrics {
    pub auto_recall: MetricEvidence,
    pub candidate_coverage: MetricEvidence,
    pub suggestion_top1: MetricEvidence,
    pub suggestion_top3: MetricEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ReleaseGates {
    pub sample_floors: bool,
    pub auto_precision_99: bool,
    pub correct_token_fpr_001: bool,
    pub suggestion_top1_85: bool,
    pub suggestion_top3_95: bool,
    pub all_pass: bool,
}

pub fn evaluate_manifest(
    manifest_path: &Path,
    workspace_root: &Path,
    mode: EvaluationMode,
) -> Result<EvaluationReport, ReportError> {
    let corpus = load_and_verify(manifest_path, workspace_root, mode)?;
    let lexicon = build_lexicon(&corpus, manifest_path)?;
    let lexicon_bytes = serde_json::to_vec(&lexicon.to_artifact())?;
    let score_config = ScoreConfig::default();
    let decision_config = DecisionConfig::default();
    let mut counts = EvaluationCounts::default();
    let mut taxonomy = all_taxonomy_rows();

    for item in corpus.items_in("held_out") {
        match item.label {
            CorpusLabel::Correct => {
                for token in item.input.split_whitespace() {
                    counts.correct_tokens = counts.correct_tokens.saturating_add(1);
                    let outcome = evaluate_token(token, &lexicon);
                    if outcome.decision == Some(DecisionState::Auto) {
                        counts.auto_false_positive = counts.auto_false_positive.saturating_add(1);
                        counts.false_auto_on_correct =
                            counts.false_auto_on_correct.saturating_add(1);
                    }
                }
            }
            CorpusLabel::Error => {
                counts.error_cases = counts.error_cases.saturating_add(1);
                let Some(kind) = item.error_type else {
                    continue;
                };
                counts.supported_error_cases = counts.supported_error_cases.saturating_add(1);
                let outcome = evaluate_token(&item.input, &lexicon);
                let position = outcome
                    .candidates
                    .iter()
                    .position(|candidate| candidate.text == item.gold);
                let row = taxonomy.entry(kind).or_default();
                row.labeled_errors = row.labeled_errors.saturating_add(1);
                if !outcome.candidates.is_empty() {
                    counts.candidate_covered = counts.candidate_covered.saturating_add(1);
                    row.candidate_covered = row.candidate_covered.saturating_add(1);
                }
                if position == Some(0) {
                    counts.suggestion_top1 = counts.suggestion_top1.saturating_add(1);
                    row.suggestion_top1 = row.suggestion_top1.saturating_add(1);
                }
                if position.is_some_and(|index| index < 3) {
                    counts.suggestion_top3 = counts.suggestion_top3.saturating_add(1);
                    row.suggestion_top3 = row.suggestion_top3.saturating_add(1);
                }
                if outcome.decision == Some(DecisionState::Auto) {
                    if position == Some(0) {
                        counts.auto_true_positive = counts.auto_true_positive.saturating_add(1);
                        row.auto_true_positive = row.auto_true_positive.saturating_add(1);
                    } else {
                        counts.auto_false_positive = counts.auto_false_positive.saturating_add(1);
                    }
                }
            }
        }
    }

    let metrics = make_metrics(&counts);
    let taxonomy = make_taxonomy_evidence(taxonomy);
    let release_gates = make_release_gates(&counts, &metrics, &taxonomy);
    Ok(EvaluationReport {
        schema_version: 1,
        mode: match mode {
            EvaluationMode::Unit => "unit",
            EvaluationMode::Release => "release",
        }
        .to_string(),
        corpus: CorpusEvidence {
            version: corpus.version,
            manifest_sha256: hash_file(manifest_path).map_err(CorpusError::from)?,
            lexicon_sha256: sha256_bytes(&lexicon_bytes),
        },
        config: ConfigEvidence {
            score_version: score_config.version,
            score_sha256: score_config.hash,
            decision_version: decision_config.version,
            decision_sha256: sha256_json(&decision_config)?,
        },
        counts,
        metrics,
        taxonomy,
        release_gates,
    })
}

fn evaluate_token(
    token: &str,
    lexicon: &openvikey_core::lexicon::Lexicon,
) -> crate::session::SessionObservation {
    let mut session = LabSession::new(
        EngineConfig {
            method: InputMethod::Telex,
            tone_placement: TonePlacement::Modern,
        },
        lexicon.clone(),
    );
    session
        .type_text(token, InputContext::default(), 0)
        .pop()
        .expect("corpus tokens are non-empty")
}

fn all_taxonomy_rows() -> BTreeMap<ErrorKind, TaxonomyCounts> {
    [
        ErrorKind::Tone,
        ErrorKind::Transpose,
        ErrorKind::MissingDiacritic,
        ErrorKind::Abbrev,
    ]
    .into_iter()
    .map(|kind| (kind, TaxonomyCounts::default()))
    .collect()
}

fn make_metrics(counts: &EvaluationCounts) -> EvaluationMetrics {
    let auto_trials = counts
        .auto_true_positive
        .saturating_add(counts.auto_false_positive);
    EvaluationMetrics {
        auto_precision: metric(
            AUTO_PRECISION_FORMULA,
            counts.auto_true_positive,
            auto_trials,
            auto_precision(counts.auto_true_positive, counts.auto_false_positive),
        ),
        correct_token_fpr: metric(
            CORRECT_TOKEN_FPR_FORMULA,
            counts.false_auto_on_correct,
            counts.correct_tokens,
            correct_token_fpr(counts.false_auto_on_correct, counts.correct_tokens),
        ),
        auto_recall: ratio_metric(
            AUTO_RECALL_FORMULA,
            counts.auto_true_positive,
            counts.supported_error_cases,
        ),
        suggestion_top1: ratio_metric(
            SUGGESTION_TOP1_FORMULA,
            counts.suggestion_top1,
            counts.supported_error_cases,
        ),
        suggestion_top3: ratio_metric(
            SUGGESTION_TOP3_FORMULA,
            counts.suggestion_top3,
            counts.supported_error_cases,
        ),
        candidate_coverage: ratio_metric(
            CANDIDATE_COVERAGE_FORMULA,
            counts.candidate_covered,
            counts.supported_error_cases,
        ),
    }
}

fn metric(
    formula: &'static str,
    numerator: u64,
    denominator: u64,
    point_estimate: Option<f64>,
) -> MetricEvidence {
    MetricEvidence {
        formula,
        numerator,
        denominator,
        point_estimate,
        wilson_95: wilson_interval(numerator, denominator, 1.96)
            .map(|(lower, upper)| WilsonInterval { lower, upper }),
    }
}

#[allow(clippy::cast_precision_loss)]
fn ratio_metric(formula: &'static str, numerator: u64, denominator: u64) -> MetricEvidence {
    let point = (denominator != 0).then(|| numerator as f64 / denominator as f64);
    metric(formula, numerator, denominator, point)
}

fn make_taxonomy_evidence(
    counts: BTreeMap<ErrorKind, TaxonomyCounts>,
) -> BTreeMap<ErrorKind, TaxonomyEvidence> {
    counts
        .into_iter()
        .map(|(kind, counts)| {
            let denominator = counts.labeled_errors;
            let metrics = TaxonomyMetrics {
                auto_recall: ratio_metric(
                    AUTO_RECALL_FORMULA,
                    counts.auto_true_positive,
                    denominator,
                ),
                candidate_coverage: ratio_metric(
                    CANDIDATE_COVERAGE_FORMULA,
                    counts.candidate_covered,
                    denominator,
                ),
                suggestion_top1: ratio_metric(
                    SUGGESTION_TOP1_FORMULA,
                    counts.suggestion_top1,
                    denominator,
                ),
                suggestion_top3: ratio_metric(
                    SUGGESTION_TOP3_FORMULA,
                    counts.suggestion_top3,
                    denominator,
                ),
            };
            (kind, TaxonomyEvidence { counts, metrics })
        })
        .collect()
}

fn make_release_gates(
    counts: &EvaluationCounts,
    metrics: &EvaluationMetrics,
    taxonomy: &BTreeMap<ErrorKind, TaxonomyEvidence>,
) -> ReleaseGates {
    let sample_floors = counts.correct_tokens >= RELEASE_MIN_CORRECT_TOKENS
        && counts.error_cases >= RELEASE_MIN_ERROR_CASES
        && taxonomy
            .values()
            .all(|row| row.counts.labeled_errors >= RELEASE_MIN_PER_ERROR_TYPE);
    let auto_precision_99 = metrics
        .auto_precision
        .point_estimate
        .is_some_and(|value| value >= 0.99);
    let correct_token_fpr_001 = metrics
        .correct_token_fpr
        .point_estimate
        .is_some_and(|value| value <= 0.001);
    let suggestion_top1_85 = metrics
        .suggestion_top1
        .point_estimate
        .is_some_and(|value| value >= 0.85);
    let suggestion_top3_95 = metrics
        .suggestion_top3
        .point_estimate
        .is_some_and(|value| value >= 0.95);
    ReleaseGates {
        sample_floors,
        auto_precision_99,
        correct_token_fpr_001,
        suggestion_top1_85,
        suggestion_top3_95,
        all_pass: sample_floors
            && auto_precision_99
            && correct_token_fpr_001
            && suggestion_top1_85
            && suggestion_top3_95,
    }
}

fn sha256_json(value: &impl Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_vec(value).map(|bytes| sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
