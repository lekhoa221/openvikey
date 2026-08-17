//! OpenViKey Lab CLI harness.

use clap::{Parser, Subcommand};
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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::ProvenanceVerify { manifest } => {
            println!("Verifying provenance at: {}", manifest.display());
            let manifest_data = provenance::ProvenanceManifest::from_file(&manifest)?;
            println!(
                "Provenance valid: {} records checked.",
                manifest_data.records.len()
            );
        }
    }
    Ok(())
}
