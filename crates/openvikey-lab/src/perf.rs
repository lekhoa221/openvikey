//! Repeatable microbenchmark report for the interactive performance budgets.

use crate::session::LabSession;
use openvikey_core::engine::{Engine, EngineConfig};
use openvikey_core::generate::fuzzy::FuzzyGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconArtifact, LexiconBigram, LexiconEntry};
use openvikey_core::model::AdaptiveModel;
use openvikey_core::types::{
    InputContext, InputEvent, InputKind, InputMethod, Modifiers, TonePlacement,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::hint::black_box;
use std::process::Command;
use std::time::Instant;
use thiserror::Error;

const REPRESENTATIVE_LEXICON_ENTRIES: usize = 5_000;
const STRESS_ONSETS: &[&str] = &[
    "", "b", "c", "ch", "d", "đ", "g", "gh", "gi", "h", "k", "kh", "l", "m", "n", "ng", "ngh",
    "nh", "p", "ph", "q", "qu", "r", "s", "t", "th", "tr", "v", "x",
];
const STRESS_NUCLEI: &[&str] = &[
    "a", "ai", "ao", "au", "ay", "ă", "â", "e", "eo", "ê", "êu", "i", "ia", "iê", "iu", "o", "oa",
    "oai", "oă", "oe", "oi", "ô", "ơ", "u", "ua", "uâ", "uê", "ui", "uô", "ươ", "uy", "uya", "uyê",
    "y", "yê",
];
const STRESS_CODAS: &[&str] = &["", "c", "ch", "m", "n", "ng", "nh", "p", "t"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfConfig {
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
    pub stress_entries: usize,
}

impl Default for PerfConfig {
    fn default() -> Self {
        Self {
            warmup_iterations: 20,
            measured_iterations: 200,
            stress_entries: 6_000,
        }
    }
}

#[derive(Debug, Error)]
pub enum PerfError {
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("measured_iterations must be non-zero")]
    NoSamples,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerfReport {
    pub schema_version: u32,
    pub profile: String,
    pub samples: PerfSamples,
    pub lexicon: PerfLexicon,
    pub per_key_us: LatencySummary,
    pub candidate_generation_us: LatencySummary,
    pub session_inject_us: LatencySummary,
    pub startup_load_us: u64,
    pub peak_memory_bytes: Option<u64>,
    pub packaged_lexicon_model_bytes: u64,
    pub gates: PerfGates,
}

impl PerfReport {
    pub fn to_pretty_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerfSamples {
    pub per_key: usize,
    pub candidate_generation: usize,
    pub session_inject: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerfLexicon {
    pub packaged_entries: usize,
    pub benchmark_entries: usize,
    pub stress_target_entries: usize,
    pub representative_for_generation: bool,
    pub benchmark_source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LatencySummary {
    pub p50: u64,
    pub p95: u64,
    pub max: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct PerfGates {
    pub release_build: bool,
    pub representative_lexicon: bool,
    pub per_key_p50_under_1ms: bool,
    pub per_key_p95_under_5ms: bool,
    pub candidate_generation_p95_under_15ms: bool,
    pub session_inject_p95_under_15ms: bool,
    pub startup_under_300ms: bool,
    pub peak_memory_under_150mb: Option<bool>,
    pub packaged_size_under_50mb: bool,
    pub all_pass: bool,
}

pub fn run_benchmarks(
    packaged_lexicon: &Lexicon,
    packaged_lexicon_bytes: usize,
    config: PerfConfig,
) -> Result<PerfReport, PerfError> {
    if config.measured_iterations == 0 {
        return Err(PerfError::NoSamples);
    }
    let packaged_entries = packaged_lexicon.entries().len();
    let benchmark_lexicon = stress_lexicon(packaged_lexicon, config.stress_entries);
    let benchmark_entries = benchmark_lexicon.entries().len();

    let startup_payload = serde_json::to_vec(&benchmark_lexicon.to_artifact())?;
    let model_payload = AdaptiveModel::default()
        .to_json_payload()
        .expect("default model serializes");
    let startup_started = Instant::now();
    let parsed: LexiconArtifact = serde_json::from_slice(black_box(&startup_payload))?;
    let startup_lexicon = Lexicon::from_artifact(parsed);
    let _model = AdaptiveModel::from_json_payload(black_box(&model_payload))
        .expect("benchmark model payload is valid");
    black_box(startup_lexicon);
    let startup_load_us = elapsed_us(startup_started);

    let per_key_samples = benchmark_per_key(config);
    let candidate_samples = benchmark_candidates(&benchmark_lexicon, config);
    let session_samples = benchmark_session_inject(&benchmark_lexicon, config);
    let per_key_us = summarize(&per_key_samples);
    let candidate_generation_us = summarize(&candidate_samples);
    let session_inject_us = summarize(&session_samples);
    let peak_memory_bytes = peak_memory_bytes();
    let packaged_lexicon_model_bytes =
        u64::try_from(packaged_lexicon_bytes.saturating_add(model_payload.len()))
            .unwrap_or(u64::MAX);
    let release_build = !cfg!(debug_assertions);
    let representative = benchmark_entries >= REPRESENTATIVE_LEXICON_ENTRIES;
    let peak_memory_gate = peak_memory_bytes.map(|bytes| bytes < 150 * 1024 * 1024);
    let mut gates = PerfGates {
        release_build,
        representative_lexicon: representative,
        per_key_p50_under_1ms: per_key_us.p50 < 1_000,
        per_key_p95_under_5ms: per_key_us.p95 < 5_000,
        candidate_generation_p95_under_15ms: candidate_generation_us.p95 < 15_000,
        session_inject_p95_under_15ms: session_inject_us.p95 < 15_000,
        startup_under_300ms: startup_load_us < 300_000,
        peak_memory_under_150mb: peak_memory_gate,
        packaged_size_under_50mb: packaged_lexicon_model_bytes < 50 * 1024 * 1024,
        all_pass: false,
    };
    gates.all_pass = gates.release_build
        && gates.representative_lexicon
        && gates.per_key_p50_under_1ms
        && gates.per_key_p95_under_5ms
        && gates.candidate_generation_p95_under_15ms
        && gates.session_inject_p95_under_15ms
        && gates.startup_under_300ms
        && gates.peak_memory_under_150mb == Some(true)
        && gates.packaged_size_under_50mb;

    Ok(PerfReport {
        schema_version: 2,
        profile: if release_build { "release" } else { "debug" }.to_string(),
        samples: PerfSamples {
            per_key: per_key_samples.len(),
            candidate_generation: candidate_samples.len(),
            session_inject: session_samples.len(),
        },
        lexicon: PerfLexicon {
            packaged_entries,
            benchmark_entries,
            stress_target_entries: config.stress_entries,
            representative_for_generation: representative,
            benchmark_source: "packaged_plus_deterministic_syllable_stress",
        },
        per_key_us,
        candidate_generation_us,
        session_inject_us,
        startup_load_us,
        peak_memory_bytes,
        packaged_lexicon_model_bytes,
        gates,
    })
}

fn benchmark_per_key(config: PerfConfig) -> Vec<u64> {
    let keys = "nghieengx";
    for _ in 0..config.warmup_iterations {
        let mut engine = Engine::new(EngineConfig::default());
        for (seq, logical) in keys.chars().enumerate() {
            black_box(engine.process(&key_event(seq, logical)));
        }
    }
    let mut samples = Vec::with_capacity(config.measured_iterations.saturating_mul(keys.len()));
    for iteration in 0..config.measured_iterations {
        let mut engine = Engine::new(EngineConfig::default());
        for (index, logical) in keys.chars().enumerate() {
            let seq = iteration.saturating_mul(keys.len()).saturating_add(index);
            let started = Instant::now();
            black_box(engine.process(&key_event(seq, logical)));
            samples.push(elapsed_us(started));
        }
    }
    samples
}

fn benchmark_candidates(lexicon: &Lexicon, config: PerfConfig) -> Vec<u64> {
    let mut engine = Engine::new(EngineConfig {
        method: InputMethod::Vni,
        tone_placement: TonePlacement::Modern,
    });
    for (seq, logical) in "paht1".chars().enumerate() {
        black_box(engine.process(&key_event(seq, logical)));
    }
    let snapshot = engine.snapshot();
    let generator = FuzzyGenerator::new(lexicon, 5);
    for _ in 0..config.warmup_iterations {
        black_box(generator.generate(&snapshot, &LeftContext::default()));
    }
    (0..config.measured_iterations)
        .map(|_| {
            let started = Instant::now();
            black_box(generator.generate(&snapshot, &LeftContext::default()));
            elapsed_us(started)
        })
        .collect()
}

fn benchmark_session_inject(lexicon: &Lexicon, config: PerfConfig) -> Vec<u64> {
    let keys = "paht1";
    let engine_config = EngineConfig {
        method: InputMethod::Vni,
        tone_placement: TonePlacement::Modern,
    };
    for _ in 0..config.warmup_iterations {
        let mut session = LabSession::new(engine_config, lexicon.clone());
        for (seq, logical) in keys.chars().enumerate() {
            black_box(session.inject(
                InputKind::Key {
                    logical,
                    physical: None,
                },
                InputContext::default(),
                i64::try_from(seq).unwrap_or(0),
            ));
        }
    }
    let mut samples = Vec::with_capacity(config.measured_iterations.saturating_mul(keys.len()));
    for iteration in 0..config.measured_iterations {
        let mut session = LabSession::new(engine_config, lexicon.clone());
        for (index, logical) in keys.chars().enumerate() {
            let seq = iteration.saturating_mul(keys.len()).saturating_add(index);
            let started = Instant::now();
            black_box(session.inject(
                InputKind::Key {
                    logical,
                    physical: None,
                },
                InputContext::default(),
                i64::try_from(seq).unwrap_or(0),
            ));
            samples.push(elapsed_us(started));
        }
    }
    samples
}

fn key_event(seq: usize, logical: char) -> InputEvent {
    InputEvent {
        seq: u64::try_from(seq).unwrap_or(u64::MAX),
        at_ms: i64::try_from(seq).unwrap_or(i64::MAX),
        kind: InputKind::Key {
            logical,
            physical: None,
        },
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    }
}

fn stress_lexicon(packaged: &Lexicon, target_entries: usize) -> Lexicon {
    let artifact = packaged.to_artifact();
    let mut entries: BTreeMap<String, LexiconEntry> = artifact
        .entries
        .into_iter()
        .map(|entry| (entry.token_nfc.clone(), entry))
        .collect();
    for token in ["phát", "không"] {
        entries.entry(token.to_string()).or_insert(LexiconEntry {
            token_nfc: token.to_string(),
            frequency: 100,
        });
    }
    'outer: for onset in STRESS_ONSETS {
        for nucleus in STRESS_NUCLEI {
            for coda in STRESS_CODAS {
                if entries.len() >= target_entries {
                    break 'outer;
                }
                let token = format!("{onset}{nucleus}{coda}");
                entries.entry(token.clone()).or_insert(LexiconEntry {
                    token_nfc: token,
                    frequency: 1,
                });
            }
        }
    }
    let bigrams = artifact.bigrams.into_iter().map(
        |LexiconBigram {
             left_nfc,
             token_nfc,
             score,
         }| ((left_nfc, token_nfc), score),
    );
    Lexicon::from_entries(entries.into_values(), bigrams, Some("perf-stress-v1"))
}

fn summarize(samples: &[u64]) -> LatencySummary {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    LatencySummary {
        p50: percentile(&sorted, 50),
        p95: percentile(&sorted, 95),
        max: sorted.last().copied().unwrap_or(0),
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = (sorted.len() - 1).saturating_mul(percentile) / 100;
    sorted[index]
}

fn elapsed_us(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

#[cfg(target_os = "linux")]
fn peak_memory_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    kib.checked_mul(1024)
}

#[cfg(target_os = "windows")]
fn peak_memory_bytes() -> Option<u64> {
    let script = format!("(Get-Process -Id {}).PeakWorkingSet64", std::process::id());
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn peak_memory_bytes() -> Option<u64> {
    None
}
