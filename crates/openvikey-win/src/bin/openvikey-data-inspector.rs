//! Development-only, read-only viewer for OpenViKey's plaintext stores.

use std::path::PathBuf;

use clap::Parser;
use openvikey_win::persist::{
    InspectionFilter, default_store_paths, ensure_open_store_cli_path, inspect_open_personal_store,
};

#[derive(Debug, Parser)]
#[command(
    name = "openvikey-data-inspector",
    about = "Read-only development inspection of OpenViKey model/capture JSON"
)]
struct Cli {
    #[arg(long)]
    model: Option<PathBuf>,
    #[arg(long)]
    capture: Option<PathBuf>,
    #[arg(long)]
    original: Option<String>,
    #[arg(long)]
    candidate: Option<String>,
    #[arg(long)]
    source: Option<String>,
    #[arg(long)]
    left_token: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let (default_model, default_capture) =
        default_store_paths(std::env::var_os("LOCALAPPDATA").map(PathBuf::from));
    let model_path = cli.model.unwrap_or(default_model);
    let capture_path = cli.capture.unwrap_or(default_capture);
    ensure_open_store_cli_path(&model_path)?;
    ensure_open_store_cli_path(&capture_path)?;
    let report = inspect_open_personal_store(
        &model_path,
        &capture_path,
        &InspectionFilter {
            original: cli.original,
            candidate: cli.candidate,
            source: cli.source,
            left_token: cli.left_token,
        },
    )?;

    println!("OpenViKey Data Inspector (read-only)");
    println!("model: {}", model_path.display());
    println!("capture: {}", capture_path.display());
    println!("model rows: {}", report.summary.model_rows);
    println!("filtered rows: {}", report.rows.len());
    println!("capture records: {}", report.summary.capture_records);
    for row in report.rows {
        println!(
            "{} -> {} | source={:?} left={} state={:?} evidence={} (+{:.2}/-{:.2}) last_ms={}",
            row.original_nfc,
            row.candidate_nfc,
            row.source,
            row.left_token_nfc.as_deref().unwrap_or("-"),
            row.state,
            row.evidence_count,
            row.positive_evidence,
            row.negative_evidence,
            row.last_evidence_at_ms
                .map_or_else(|| "-".into(), |value| value.to_string()),
        );
    }
    println!("Run again to refresh; this program never writes the input files.");
    Ok(())
}
