//! OpenViKey Lab CLI harness.

use clap::{Parser, Subcommand};
use openvikey_lab::corpus::{self, EvaluationMode};
use openvikey_lab::provenance;
use std::path::PathBuf;

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
    /// Verify data provenance and license compliance
    ProvenanceVerify {
        /// Path to provenance.toml
        #[arg(short, long, default_value = "data/provenance.toml")]
        manifest: PathBuf,
    },
    /// Corpus split, hash, and license gates
    #[command(subcommand)]
    Corpus(CorpusCmd),
}

#[derive(Subcommand, Debug)]
enum CorpusCmd {
    /// Verify a pinned corpus manifest
    Verify {
        #[arg(long, default_value = "data/corpus-manifest.toml")]
        manifest: PathBuf,
        /// `unit` allows authored fixtures; `release` enforces spec sample floors
        #[arg(long, default_value = "unit")]
        mode: String,
    },
    /// Reproducibly build a lexicon/bigram artifact from the train split
    BuildLexicon {
        #[arg(long, default_value = "data/corpus-manifest.toml")]
        manifest: PathBuf,
        #[arg(long, default_value = "data/fixtures/lexicon/authored.json")]
        out: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::ProvenanceVerify { manifest } => {
            println!("Verifying provenance at: {}", manifest.display());
            let manifest_data = provenance::ProvenanceManifest::from_file(&manifest)?;
            let workspace_root = manifest
                .parent()
                .and_then(std::path::Path::parent)
                .ok_or("provenance manifest must live at <workspace>/data/provenance.toml")?;
            manifest_data.verify_artifacts(workspace_root)?;
            println!(
                "Provenance valid: {} records checked.",
                manifest_data.records.len()
            );
        }
        Commands::Corpus(CorpusCmd::Verify { manifest, mode }) => {
            let mode = EvaluationMode::parse_cli(&mode)?;
            let workspace_root = manifest
                .parent()
                .and_then(std::path::Path::parent)
                .ok_or("corpus manifest must live at <workspace>/data/corpus-manifest.toml")?;
            let corpus = corpus::load_and_verify(&manifest, workspace_root, mode)?;
            println!(
                "Corpus valid ({mode:?}): train={} calibration={} held_out={}",
                corpus.items_in("train").len(),
                corpus.items_in("calibration").len(),
                corpus.items_in("held_out").len()
            );
        }
        Commands::Corpus(CorpusCmd::BuildLexicon { manifest, out }) => {
            let workspace_root = manifest
                .parent()
                .and_then(std::path::Path::parent)
                .ok_or("corpus manifest must live at <workspace>/data/corpus-manifest.toml")?;
            let verified =
                corpus::load_and_verify(&manifest, workspace_root, EvaluationMode::Unit)?;
            let lexicon = corpus::build_lexicon(&verified, &manifest)?;
            corpus::write_lexicon_artifact(&lexicon, &out)?;
            println!("Lexicon artifact written to: {}", out.display());
        }
    }
    Ok(())
}
