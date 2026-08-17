//! OpenViKey Windows hook host coordinator.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};

use clap::{Parser, ValueEnum};
use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_core::store::passphrase::{KdfConfig, PassphraseProvider};
use openvikey_core::types::{InputMethod, TonePlacement};
use openvikey_session::capture::{
    ensure_distinct_store_paths, load_personal_store, sha256_hex, CaptureHeader, CaptureLog,
    CAPTURE_VERSION,
};
use openvikey_session::persistence::DebouncedSaver;
use openvikey_session::session::{LabSession, SessionCursors};
use openvikey_win::focus::{run_host_message_loop, FocusCache};
#[cfg(windows)]
use openvikey_win::focus::HostHooks;
use openvikey_win::host::{bind_runtime, TypingHost};
use openvikey_win::inject::{InjectProfile, ProfilingInjector, SendInputSender};
use openvikey_win::passphrase::{read_hidden_passphrase, release_console};
use openvikey_win::persist::{default_store_paths, HostShutdown};

#[derive(Parser, Debug)]
#[command(
    name = "openvikey-win",
    author,
    version,
    about = "Windows hook host for OpenViKey"
)]
struct Cli {
    #[arg(long, value_enum, default_value = "telex")]
    method: MethodArg,
    #[arg(long, default_value = "data/fixtures/lexicon/authored.json")]
    lexicon: PathBuf,
    #[arg(long)]
    model: Option<PathBuf>,
    #[arg(long)]
    capture: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    electron_gap_ms: u64,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    // Reserved for Electron SendInput gap (inject path).
    let _ = cli.electron_gap_ms;

    let (default_model, default_capture) =
        default_store_paths(std::env::var_os("LOCALAPPDATA").map(PathBuf::from));
    let model_path = cli.model.unwrap_or(default_model);
    let capture_path = cli.capture.unwrap_or(default_capture);
    ensure_distinct_store_paths(&model_path, &capture_path)?;

    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let passphrase = read_hidden_passphrase()?;
    let provider = PassphraseProvider::new(passphrase.as_str(), KdfConfig::default());

    let artifact: LexiconArtifact = serde_json::from_slice(&std::fs::read(&cli.lexicon)?)?;
    let lexicon = Lexicon::from_artifact(artifact);
    let (model, log) = load_personal_store(&model_path, &capture_path, &provider)?;
    let mut session = LabSession::new_with_model(
        EngineConfig {
            method: cli.method.into(),
            tone_placement: TonePlacement::Modern,
        },
        lexicon,
        model,
        SessionCursors {
            next_seq: log.header.next_seq,
            next_edit_id: log.header.next_edit_id,
        },
    );
    session.restore_capture(log.records);
    session.restore_last_at_ms(log.header.last_at_ms);

    let sending = Arc::new(AtomicBool::new(false));
    let mut typing = TypingHost::new_with_session(session);
    typing.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: SendInputSender,
        sending: Arc::clone(&sending),
    }));
    let host = Arc::new(Mutex::new(typing));
    let focus = Arc::new(FocusCache::new());
    bind_runtime(Arc::clone(&host), Arc::clone(&focus));

    let model_saver = DebouncedSaver::spawn_encrypted(model_path.clone(), provider.clone(), {
        let host = Arc::clone(&host);
        move || {
            let guard = host.lock().unwrap_or_else(PoisonError::into_inner);
            let snap = guard.session.save_snapshot();
            snap.model
                .to_json_payload()
                .map_err(|error| error.to_string())
        }
    });
    let capture_saver = DebouncedSaver::spawn_encrypted(capture_path.clone(), provider, {
        let host = Arc::clone(&host);
        move || {
            let guard = host.lock().unwrap_or_else(PoisonError::into_inner);
            let snap = guard.session.save_snapshot();
            let model_payload = snap
                .model
                .to_json_payload()
                .map_err(|error| error.to_string())?;
            let log = CaptureLog {
                header: CaptureHeader {
                    v: CAPTURE_VERSION,
                    next_seq: snap.cursors.next_seq,
                    next_edit_id: snap.cursors.next_edit_id,
                    last_at_ms: snap.last_at_ms,
                    model_sha256: sha256_hex(&model_payload),
                },
                records: snap.capture_records,
            };
            log.to_payload().map_err(|error| error.to_string())
        }
    });

    let shutdown = Arc::new(HostShutdown::new());

    #[cfg(windows)]
    let _hooks = HostHooks::install(Arc::clone(&focus))?;

    release_console();
    run_host_message_loop(&shutdown);

    let _ = model_saver.flush();
    let _ = capture_saver.flush();
    drop(model_saver);
    drop(capture_saver);
    Ok(())
}
