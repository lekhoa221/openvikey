//! M10 performance evidence must use a representative stress lexicon, not tiny fixtures.

use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_lab::perf::{PerfConfig, run_benchmarks};
use std::fs;
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
fn report_benchmarks_dp_generation_against_thousands_of_entries() {
    let path = workspace_root().join("data/fixtures/lexicon/authored.json");
    let packaged = fs::read(&path).unwrap();
    let artifact: LexiconArtifact = serde_json::from_slice(&packaged).unwrap();
    let lexicon = Lexicon::from_artifact(artifact);
    let report = run_benchmarks(
        &lexicon,
        packaged.len(),
        PerfConfig {
            warmup_iterations: 2,
            measured_iterations: 12,
            stress_entries: 6_000,
        },
    )
    .unwrap();

    assert_eq!(report.schema_version, 2);
    assert_eq!(report.samples.candidate_generation, 12);
    assert_eq!(report.samples.session_inject, 60);
    assert!(report.lexicon.benchmark_entries >= 5_000);
    assert!(report.lexicon.representative_for_generation);
    assert!(report.candidate_generation_us.p50 <= report.candidate_generation_us.p95);
    assert!(report.candidate_generation_us.p95 <= report.candidate_generation_us.max);
    assert!(report.per_key_us.p50 <= report.per_key_us.p95);
    assert!(report.session_inject_us.p50 <= report.session_inject_us.p95);
    assert_eq!(
        report.gates.session_inject_p95_under_15ms,
        report.session_inject_us.p95 < 15_000
    );
    assert_eq!(
        report.gates.candidate_generation_p95_under_15ms,
        report.candidate_generation_us.p95 < 15_000
    );
    assert!(
        !report.gates.release_build,
        "unit tests are not release evidence"
    );
}
