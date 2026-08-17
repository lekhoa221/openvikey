//! Deterministic corpus manifest loader, split verifier, and lexicon builder.

use crate::provenance::{
    self, ProvenanceManifest, ProvenanceRecord, ProvenanceStatus, sha256_file,
};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// Spec §3.2 release floors. Unit mode does not enforce these.
pub const RELEASE_MIN_CORRECT_TOKENS: u64 = 50_000;
pub const RELEASE_MIN_ERROR_CASES: u64 = 1_000;
pub const RELEASE_MIN_PER_ERROR_TYPE: u64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationMode {
    Unit,
    Release,
}

impl EvaluationMode {
    pub fn parse_cli(value: &str) -> Result<Self, CorpusError> {
        match value {
            "unit" => Ok(Self::Unit),
            "release" => Ok(Self::Release),
            other => Err(CorpusError::Validation(format!(
                "unknown evaluation mode '{other}' (expected unit|release)"
            ))),
        }
    }
}

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("JSON error in {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    Validation(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorpusLabel {
    Correct,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Tone,
    Transpose,
    MissingDiacritic,
    Abbrev,
}

impl ErrorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tone => "tone",
            Self::Transpose => "transpose",
            Self::MissingDiacritic => "missing_diacritic",
            Self::Abbrev => "abbrev",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusItem {
    pub id: String,
    pub label: CorpusLabel,
    #[serde(default)]
    pub error_type: Option<ErrorKind>,
    pub input: String,
    pub gold: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestFile {
    version: String,
    provenance: String,
    split_seed: u64,
    #[serde(default)]
    evaluation: EvaluationThresholds,
    #[serde(default)]
    assets: Vec<ManifestAsset>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::struct_field_names)]
struct EvaluationThresholds {
    #[serde(default = "default_min_correct")]
    min_correct_tokens_release: u64,
    #[serde(default = "default_min_errors")]
    min_error_cases_release: u64,
    #[serde(default = "default_min_per_type")]
    min_per_error_type_release: u64,
}

impl Default for EvaluationThresholds {
    fn default() -> Self {
        Self {
            min_correct_tokens_release: RELEASE_MIN_CORRECT_TOKENS,
            min_error_cases_release: RELEASE_MIN_ERROR_CASES,
            min_per_error_type_release: RELEASE_MIN_PER_ERROR_TYPE,
        }
    }
}

fn default_min_correct() -> u64 {
    RELEASE_MIN_CORRECT_TOKENS
}
fn default_min_errors() -> u64 {
    RELEASE_MIN_ERROR_CASES
}
fn default_min_per_type() -> u64 {
    RELEASE_MIN_PER_ERROR_TYPE
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestAsset {
    id: String,
    path: String,
    sha256: String,
    split_role: String,
    provenance_id: String,
}

/// Corpus that passed license, hash, and split checks (and release floors when asked).
#[derive(Debug, Clone)]
pub struct VerifiedCorpus {
    pub version: String,
    splits: BTreeMap<String, Vec<CorpusItem>>,
}

impl VerifiedCorpus {
    #[must_use]
    pub fn items_in(&self, split: &str) -> &[CorpusItem] {
        self.splits
            .get(normalize_split(split))
            .map_or(&[], Vec::as_slice)
    }
}

pub fn load_and_verify(
    manifest_path: &Path,
    workspace_root: &Path,
    mode: EvaluationMode,
) -> Result<VerifiedCorpus, CorpusError> {
    let raw = std::fs::read_to_string(manifest_path)?;
    let file: ManifestFile = toml::from_str(&raw)?;
    if file.split_seed == 0 {
        return Err(CorpusError::Validation(
            "split_seed must be non-zero for a frozen corpus".to_string(),
        ));
    }
    let provenance = ProvenanceManifest::from_file(workspace_root.join(&file.provenance))
        .map_err(|err| CorpusError::Validation(err.to_string()))?;

    let mut splits: BTreeMap<String, Vec<CorpusItem>> = BTreeMap::new();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();

    for asset in &file.assets {
        let record = find_record(&provenance, &asset.provenance_id)?;
        assert_shippable_data(record)?;

        let path = workspace_root.join(&asset.path);
        let actual = sha256_file(&path)?;
        if !actual.eq_ignore_ascii_case(&asset.sha256) {
            return Err(CorpusError::Validation(format!(
                "hash mismatch for asset '{}': declared {}, got {actual}",
                asset.id, asset.sha256
            )));
        }
        if !actual.eq_ignore_ascii_case(&record.sha256) {
            return Err(CorpusError::Validation(format!(
                "hash mismatch versus provenance '{}': expected {}, got {actual}",
                record.id, record.sha256
            )));
        }

        let split = normalize_split(&asset.split_role).to_string();
        let items = read_jsonl(&path)?;
        for item in items {
            if item.id.trim().is_empty() {
                return Err(CorpusError::Validation(format!(
                    "empty item id in {}",
                    asset.path
                )));
            }
            if let Some(previous) = seen.get(&item.id) {
                if previous != &split {
                    return Err(CorpusError::Validation(format!(
                        "overlap: item '{}' appears in '{previous}' and '{split}'",
                        item.id
                    )));
                }
                return Err(CorpusError::Validation(format!(
                    "overlap: duplicate item '{}' in split '{split}'",
                    item.id
                )));
            }
            seen.insert(item.id.clone(), split.clone());
            splits.entry(split.clone()).or_default().push(item);
        }
    }

    let corpus = VerifiedCorpus {
        version: file.version,
        splits,
    };
    if mode == EvaluationMode::Release {
        enforce_release_floors(&corpus, &file.evaluation)?;
    }
    Ok(corpus)
}

pub fn build_lexicon(
    corpus: &VerifiedCorpus,
    manifest_path: &Path,
) -> Result<Lexicon, CorpusError> {
    let source_manifest_hash = sha256_file(manifest_path)?;
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut bigram_counts: BTreeMap<(String, String), u32> = BTreeMap::new();
    let mut left_totals: BTreeMap<String, u32> = BTreeMap::new();

    for item in corpus.items_in("train") {
        let tokens: Vec<String> = item
            .gold
            .split_whitespace()
            .map(|token| token.nfc().collect())
            .collect();
        for token in &tokens {
            *counts.entry(token.clone()).or_insert(0) += 1;
        }
        for pair in tokens.windows(2) {
            let left = pair[0].clone();
            let token = pair[1].clone();
            *bigram_counts.entry((left.clone(), token)).or_insert(0) += 1;
            *left_totals.entry(left).or_insert(0) += 1;
        }
    }

    let entries = counts
        .into_iter()
        .map(|(token_nfc, frequency)| LexiconEntry {
            token_nfc,
            frequency,
        });
    let bigrams = bigram_counts.into_iter().map(|((left, token), count)| {
        let total = left_totals.get(&left).copied().unwrap_or(count);
        ((left, token), f64::from(count) / f64::from(total))
    });
    Ok(Lexicon::from_entries(
        entries,
        bigrams,
        Some(&source_manifest_hash),
    ))
}

/// Writes a byte-deterministic, human-readable lexicon artifact.
pub fn write_lexicon_artifact(lexicon: &Lexicon, path: &Path) -> Result<(), CorpusError> {
    let mut bytes =
        serde_json::to_vec_pretty(&lexicon.to_artifact()).map_err(|source| CorpusError::Json {
            path: path.display().to_string(),
            source,
        })?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)?;
    Ok(())
}

fn find_record<'a>(
    provenance: &'a ProvenanceManifest,
    id: &str,
) -> Result<&'a ProvenanceRecord, CorpusError> {
    provenance
        .records
        .iter()
        .find(|record| record.id == id)
        .ok_or_else(|| CorpusError::Validation(format!("missing provenance record '{id}'")))
}

const ALLOWED_DATA_LICENSES: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "CC0",
    "CC0-1.0",
    "Unicode-DFS",
    "Unicode-DFS-2016",
];

fn assert_shippable_data(record: &ProvenanceRecord) -> Result<(), CorpusError> {
    if record.status != ProvenanceStatus::Approved {
        return Err(CorpusError::Validation(format!(
            "record '{}' is {:?}, only approved assets may ship",
            record.id, record.status
        )));
    }
    let revision = record.revision.trim();
    if revision.is_empty() || revision.eq_ignore_ascii_case("unknown") {
        return Err(CorpusError::Validation(format!(
            "missing or unknown revision for '{}'",
            record.id
        )));
    }
    let hash = record.sha256.trim();
    if hash.is_empty() || hash.eq_ignore_ascii_case("unknown") {
        return Err(CorpusError::Validation(format!(
            "missing or unknown hash for '{}'",
            record.id
        )));
    }
    let data_license = record.data_license.trim();
    if !ALLOWED_DATA_LICENSES
        .iter()
        .any(|allowed| data_license.eq_ignore_ascii_case(allowed))
    {
        return Err(CorpusError::Validation(format!(
            "data license '{data_license}' is not allowlisted for '{}' (code license alone is insufficient)",
            record.id
        )));
    }
    if !record.redistribution.eq_ignore_ascii_case("allowed") {
        return Err(CorpusError::Validation(format!(
            "redistribution '{}' is not allowed for '{}'",
            record.redistribution, record.id
        )));
    }
    Ok(())
}

fn read_jsonl(path: &Path) -> Result<Vec<CorpusItem>, CorpusError> {
    let text = std::fs::read_to_string(path)?;
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let item: CorpusItem = serde_json::from_str(line).map_err(|source| CorpusError::Json {
            path: format!("{}:{}", path.display(), idx + 1),
            source,
        })?;
        items.push(item);
    }
    Ok(items)
}

fn normalize_split(role: &str) -> &str {
    match role {
        "test" | "held_out" | "held-out" => "held_out",
        other => other,
    }
}

fn enforce_release_floors(
    corpus: &VerifiedCorpus,
    floors: &EvaluationThresholds,
) -> Result<(), CorpusError> {
    let min_correct = floors
        .min_correct_tokens_release
        .max(RELEASE_MIN_CORRECT_TOKENS);
    let min_errors = floors.min_error_cases_release.max(RELEASE_MIN_ERROR_CASES);
    let min_per_type = floors
        .min_per_error_type_release
        .max(RELEASE_MIN_PER_ERROR_TYPE);

    let mut correct = 0_u64;
    let mut errors = 0_u64;
    let mut per_type: BTreeMap<ErrorKind, u64> = BTreeMap::new();
    for item in corpus.items_in("held_out") {
        match item.label {
            CorpusLabel::Correct => {
                correct += u64::try_from(item.gold.split_whitespace().count()).unwrap_or(u64::MAX);
            }
            CorpusLabel::Error => {
                errors += 1;
                if let Some(kind) = item.error_type {
                    *per_type.entry(kind).or_insert(0) += 1;
                }
            }
        }
    }

    if correct < min_correct {
        return Err(CorpusError::Validation(format!(
            "release minimum sample: held-out correct tokens {correct} < {min_correct}"
        )));
    }
    if errors < min_errors {
        return Err(CorpusError::Validation(format!(
            "release minimum sample: held-out error cases {errors} < {min_errors}"
        )));
    }

    let required = [
        ErrorKind::Tone,
        ErrorKind::Transpose,
        ErrorKind::MissingDiacritic,
        ErrorKind::Abbrev,
    ];
    let mut missing = BTreeSet::new();
    for kind in required {
        let count = per_type.get(&kind).copied().unwrap_or(0);
        if count < min_per_type {
            missing.insert(format!("{}={count}<{min_per_type}", kind.as_str()));
        }
    }
    if !missing.is_empty() {
        return Err(CorpusError::Validation(format!(
            "release minimum sample per error type: {}",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(())
}

/// Re-export so callers can hash fixtures the same way verify does.
pub use provenance::sha256_file as hash_file;
