//! Clap command surface for the observable lab harness.

use crate::corpus::{self, EvaluationMode};
use crate::perf::{PerfConfig, run_benchmarks};
use crate::provenance;
use crate::report::evaluate_manifest;
use crate::script::run_script_jsonl;
use crate::session::LabSession;
use clap::{Parser, Subcommand, ValueEnum};
use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_core::model::AdaptiveModel;
use openvikey_core::store::ModelStore;
use openvikey_core::store::file::FileModelStore;
use openvikey_core::store::passphrase::{KdfConfig, PassphraseProvider};
use openvikey_core::types::{InputContext, InputMethod, TonePlacement};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

#[derive(Parser, Debug)]
#[command(
    name = "openvikey-lab",
    author,
    version,
    about = "Harness and verification tools for OpenViKey"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Stream keys from stdin through Engine and correction as JSONL
    Type {
        #[arg(long, value_enum, default_value = "telex")]
        method: MethodArg,
        #[arg(long, default_value = "data/fixtures/lexicon/authored.json")]
        lexicon: PathBuf,
        #[arg(long, value_enum, default_value = "normal")]
        context: ContextArg,
    },
    /// Replay deterministic learning scripts
    #[command(subcommand)]
    Script(ScriptCmd),
    /// Inspect encrypted adaptive models
    #[command(subcommand)]
    Model(ModelCmd),
    /// Verify data provenance and license compliance
    ProvenanceVerify {
        #[arg(short, long, default_value = "data/provenance.toml")]
        manifest: PathBuf,
    },
    /// Corpus split, hash, build, and evaluation commands
    #[command(subcommand)]
    Corpus(CorpusCmd),
    /// Write a machine-readable performance report
    Perf {
        #[arg(long, default_value = "target/perf.json")]
        out: PathBuf,
        #[arg(long, default_value = "data/fixtures/lexicon/authored.json")]
        lexicon: PathBuf,
        #[arg(long, default_value_t = 20)]
        warmup: usize,
        #[arg(long, default_value_t = 200)]
        iterations: usize,
        #[arg(long, default_value_t = 6_000)]
        stress_entries: usize,
    },
}

#[derive(Subcommand, Debug)]
enum ScriptCmd {
    Run { script: PathBuf },
}

#[derive(Subcommand, Debug)]
enum ModelCmd {
    /// Read passphrase from stdin, authenticate, then dump model JSON
    Dump { encrypted_model: PathBuf },
}

#[derive(Subcommand, Debug)]
enum CorpusCmd {
    Verify {
        #[arg(long, default_value = "data/corpus-manifest.toml")]
        manifest: PathBuf,
        #[arg(long, default_value = "unit")]
        mode: String,
    },
    BuildLexicon {
        #[arg(long, default_value = "data/corpus-manifest.toml")]
        manifest: PathBuf,
        #[arg(long, default_value = "data/fixtures/lexicon/authored.json")]
        out: PathBuf,
    },
    Evaluate {
        #[arg(long, default_value = "data/corpus-manifest.toml")]
        manifest: PathBuf,
        #[arg(long, default_value = "target/evaluation.json")]
        out: PathBuf,
        #[arg(long, default_value = "unit")]
        mode: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MethodArg {
    Telex,
    Vni,
}

impl From<MethodArg> for InputMethod {
    fn from(value: MethodArg) -> Self {
        match value {
            MethodArg::Telex => Self::Telex,
            MethodArg::Vni => Self::Vni,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ContextArg {
    Normal,
    Password,
    Terminal,
    Denylist,
}

impl ContextArg {
    const fn flags(self) -> InputContext {
        match self {
            Self::Normal => InputContext {
                allow_transform: true,
                allow_learning: true,
            },
            Self::Password | Self::Terminal | Self::Denylist => InputContext {
                allow_transform: false,
                allow_learning: false,
            },
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Commands::Type {
            method,
            lexicon,
            context,
        } => run_type(method.into(), &lexicon, context.flags())?,
        Commands::Script(ScriptCmd::Run { script }) => {
            let input = fs::read_to_string(script)?;
            io::stdout().write_all(&run_script_jsonl(&input)?.to_pretty_json()?)?;
        }
        Commands::Model(ModelCmd::Dump { encrypted_model }) => dump_model(&encrypted_model)?,
        Commands::ProvenanceVerify { manifest } => verify_provenance(&manifest)?,
        Commands::Corpus(CorpusCmd::Verify { manifest, mode }) => {
            let mode = EvaluationMode::parse_cli(&mode)?;
            let root = workspace_root_for_data_manifest(&manifest)?;
            let verified = corpus::load_and_verify(&manifest, root, mode)?;
            println!(
                "Corpus valid ({mode:?}): train={} calibration={} held_out={}",
                verified.items_in("train").len(),
                verified.items_in("calibration").len(),
                verified.items_in("held_out").len()
            );
        }
        Commands::Corpus(CorpusCmd::BuildLexicon { manifest, out }) => {
            let root = workspace_root_for_data_manifest(&manifest)?;
            let verified = corpus::load_and_verify(&manifest, root, EvaluationMode::Unit)?;
            let lexicon = corpus::build_lexicon(&verified, &manifest)?;
            corpus::write_lexicon_artifact(&lexicon, &out)?;
            println!("Lexicon artifact written to: {}", out.display());
        }
        Commands::Corpus(CorpusCmd::Evaluate {
            manifest,
            out,
            mode,
        }) => {
            let mode = EvaluationMode::parse_cli(&mode)?;
            let root = workspace_root_for_data_manifest(&manifest)?;
            let report = evaluate_manifest(&manifest, root, mode)?;
            write_output(&out, &report.to_pretty_json()?)?;
            println!("Evaluation report written to: {}", out.display());
        }
        Commands::Perf {
            out,
            lexicon,
            warmup,
            iterations,
            stress_entries,
        } => {
            let packaged_bytes = fs::read(&lexicon)?;
            let artifact: LexiconArtifact = serde_json::from_slice(&packaged_bytes)?;
            let lexicon = Lexicon::from_artifact(artifact);
            let report = run_benchmarks(
                &lexicon,
                packaged_bytes.len(),
                PerfConfig {
                    warmup_iterations: warmup,
                    measured_iterations: iterations,
                    stress_entries,
                },
            )?;
            write_output(&out, &report.to_pretty_json()?)?;
            println!("Performance report written to: {}", out.display());
        }
    }
    Ok(())
}

fn run_type(
    method: InputMethod,
    lexicon_path: &Path,
    context: InputContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let artifact: LexiconArtifact = serde_json::from_slice(&fs::read(lexicon_path)?)?;
    let mut session = LabSession::new(
        EngineConfig {
            method,
            tone_placement: TonePlacement::Modern,
        },
        Lexicon::from_artifact(artifact),
    );
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for observation in session.type_text(&input, context, 0) {
        serde_json::to_writer(&mut stdout, &observation)?;
        stdout.write_all(b"\n")?;
    }
    stdout.flush()?;
    Ok(())
}

fn dump_model(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    eprint!("Passphrase: ");
    let mut passphrase = Zeroizing::new(String::new());
    io::stdin().lock().read_line(&mut passphrase)?;
    while passphrase.ends_with(['\n', '\r']) {
        passphrase.pop();
    }
    let provider = PassphraseProvider::new(passphrase.as_str(), KdfConfig::default());
    let payload = FileModelStore::new(path).load(&provider)?;
    let model = AdaptiveModel::from_json_payload(&payload)?;
    let authenticated = model.to_json_payload()?;
    io::stdout().write_all(&authenticated)?;
    io::stdout().write_all(b"\n")?;
    Ok(())
}

fn verify_provenance(manifest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_data = provenance::ProvenanceManifest::from_file(manifest)?;
    let workspace_root = workspace_root_for_data_manifest(manifest)?;
    manifest_data.verify_artifacts(workspace_root)?;
    println!(
        "Provenance valid: {} records checked.",
        manifest_data.records.len()
    );
    Ok(())
}

fn workspace_root_for_data_manifest(path: &Path) -> Result<&Path, &'static str> {
    path.parent()
        .and_then(Path::parent)
        .ok_or("manifest must live at <workspace>/data/<manifest>")
}

fn write_output(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}
