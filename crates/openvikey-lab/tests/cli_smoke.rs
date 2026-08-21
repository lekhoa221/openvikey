//! M9 required-command smoke tests through the compiled CLI boundary.

use openvikey_core::model::AdaptiveModel;
use openvikey_core::store::ModelStore;
use openvikey_core::store::file::FileModelStore;
use openvikey_core::store::passphrase::{KdfConfig, PassphraseProvider};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn artifact(name: &str) -> PathBuf {
    let directory = workspace_root()
        .join("target/m9-cli-tests")
        .join(std::process::id().to_string());
    fs::create_dir_all(&directory).unwrap();
    directory.join(name)
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_openvikey-lab")
}

#[test]
fn help_lists_every_required_command() {
    let output = Command::new(binary()).arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in ["type", "script", "model", "corpus", "perf", "session"] {
        assert!(stdout.contains(command), "missing command {command}");
    }
}

#[test]
fn type_command_emits_machine_readable_live_candidates() {
    let lexicon = workspace_root().join("data/fixtures/lexicon/authored.json");
    let mut child = Command::new(binary())
        .args([
            "type",
            "--method",
            "telex",
            "--lexicon",
            lexicon.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"ko").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let last: Value = serde_json::from_slice(
        output
            .stdout
            .split(|byte| *byte == b'\n')
            .rfind(|line| !line.is_empty())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(last["snapshot"]["raw_keys"], "ko");
    assert_eq!(last["candidates"][0]["text"], "không");
}

#[test]
fn sensitive_context_flags_emit_no_candidates() {
    let lexicon = workspace_root().join("data/fixtures/lexicon/authored.json");
    for context in ["password", "terminal", "denylist"] {
        let mut child = Command::new(binary())
            .args([
                "type",
                "--method",
                "telex",
                "--lexicon",
                lexicon.to_str().unwrap(),
                "--context",
                context,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"ko").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        for line in output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let observation: Value = serde_json::from_slice(line).unwrap();
            assert_eq!(observation["candidates"], serde_json::json!([]));
        }
    }
}

#[test]
fn script_and_corpus_reports_are_reproducible_json() {
    let provenance_path = artifact("provenance-report.json");
    let provenance_manifest = workspace_root().join("data/provenance.toml");
    let provenance = Command::new(binary())
        .args([
            "provenance-verify",
            "--manifest",
            provenance_manifest.to_str().unwrap(),
            "--out",
            provenance_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(provenance.status.success());
    let provenance_report: Value =
        serde_json::from_slice(&fs::read(provenance_path).unwrap()).unwrap();
    assert_eq!(provenance_report["artifacts_verified"], true);

    let script_path = artifact("learning.jsonl");
    fs::write(
        &script_path,
        "{\"op\":\"accept\",\"seq\":1,\"at_ms\":0,\"candidate_id\":1,\"rule\":{\"input_method\":\"Telex\",\"source\":\"Abbreviation\",\"original_nfc\":\"ko\",\"candidate_nfc\":\"không\",\"left_token_nfc\":null,\"source_rule_id\":\"seed:ko->không\"}}\n",
    )
    .unwrap();
    let first = Command::new(binary())
        .args(["script", "run", script_path.to_str().unwrap()])
        .output()
        .unwrap();
    let second = Command::new(binary())
        .args(["script", "run", script_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);

    let report_path = artifact("evaluation.json");
    let manifest = workspace_root().join("data/corpus-manifest.toml");
    let output = Command::new(binary())
        .args([
            "corpus",
            "evaluate",
            "--manifest",
            manifest.to_str().unwrap(),
            "--out",
            report_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["counts"]["error_cases"], 2);

    let perf_path = artifact("perf.json");
    let lexicon = workspace_root().join("data/fixtures/lexicon/authored.json");
    let perf = Command::new(binary())
        .args([
            "perf",
            "--out",
            perf_path.to_str().unwrap(),
            "--lexicon",
            lexicon.to_str().unwrap(),
            "--warmup",
            "1",
            "--iterations",
            "2",
            "--stress-entries",
            "5000",
        ])
        .output()
        .unwrap();
    assert!(
        perf.status.success(),
        "{}",
        String::from_utf8_lossy(&perf.stderr)
    );
    let perf_report: Value = serde_json::from_slice(&fs::read(perf_path).unwrap()).unwrap();
    assert_eq!(
        perf_report["lexicon"]["representative_for_generation"],
        true
    );
}

#[test]
fn model_dump_writes_nothing_before_successful_authentication() {
    let model_path = artifact("model.ovk");
    let provider = PassphraseProvider::new(
        "correct horse",
        KdfConfig {
            memory_kib: 19 * 1_024,
            iterations: 2,
            parallelism: 1,
        },
    );
    FileModelStore::new(&model_path)
        .save(
            &AdaptiveModel::default().to_json_payload().unwrap(),
            &provider,
        )
        .unwrap();

    let wrong = run_dump(&model_path, "wrong\n");
    assert!(!wrong.status.success());
    assert!(wrong.stdout.is_empty());

    let correct = run_dump(&model_path, "correct horse\n");
    assert!(correct.status.success());
    let dumped: Value = serde_json::from_slice(&correct.stdout).unwrap();
    assert_eq!(dumped["version"], 2);
}

fn run_dump(path: &Path, passphrase: &str) -> std::process::Output {
    let mut child = Command::new(binary())
        .args(["model", "dump", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(passphrase.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn session_without_tty_exits_with_terminal_message() {
    let output = Command::new(binary())
        .args(["session"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.to_lowercase().contains("terminal"),
        "stderr was {stderr}"
    );
}
