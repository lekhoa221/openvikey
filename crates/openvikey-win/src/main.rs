//! OpenViKey Windows hook host coordinator.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use clap::{Parser, ValueEnum};
use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_core::store::passphrase::{KdfConfig, PassphraseProvider};
use openvikey_core::types::{InputMethod, TonePlacement};
use openvikey_session::capture::{ensure_distinct_store_paths, load_personal_store};
use openvikey_session::persistence::DebouncedSaver;
use openvikey_session::session::{LabSession, SessionCursors};
use openvikey_win::focus::{run_host_message_loop, FocusCache};
#[cfg(windows)]
use openvikey_win::focus::HostHooks;
use openvikey_win::host::{bind_persist_notify, bind_runtime, TypingHost};
use openvikey_win::inject::{InjectProfile, ProfilingInjector, SendInputSender};
use openvikey_win::passphrase::{read_hidden_passphrase, release_console};
use openvikey_win::persist::{
    capture_payload_from_host, default_store_paths, model_payload_from_host, HostShutdown,
};
#[cfg(windows)]
use openvikey_win::tray::install_host_ui;

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
    let initial_mode = typing.mode;
    typing.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: SendInputSender,
        sending: Arc::clone(&sending),
    }));
    let host = Arc::new(Mutex::new(typing));
    let focus = Arc::new(FocusCache::new());
    bind_runtime(Arc::clone(&host), Arc::clone(&focus));

    let model_saver = Arc::new(DebouncedSaver::spawn_encrypted(
        model_path.clone(),
        provider.clone(),
        {
            let host = Arc::clone(&host);
            move || model_payload_from_host(&host)
        },
    ));
    let capture_saver = Arc::new(DebouncedSaver::spawn_encrypted(
        capture_path.clone(),
        provider,
        {
            let host = Arc::clone(&host);
            move || capture_payload_from_host(&host)
        },
    ));

    let model_for_notify = Arc::clone(&model_saver);
    let capture_for_notify = Arc::clone(&capture_saver);
    bind_persist_notify(Arc::new(move || {
        let _ = model_for_notify.notify();
        let _ = capture_for_notify.notify();
    }));

    let shutdown = Arc::new(HostShutdown::new());

    #[cfg(windows)]
    let _hooks = HostHooks::install(Arc::clone(&focus))?;

    #[cfg(windows)]
    let _host_ui = install_host_ui(&shutdown, initial_mode)?;
    #[cfg(not(windows))]
    let _ = initial_mode;

    release_console();
    run_host_message_loop(&shutdown);

    let _ = model_saver.flush();
    let _ = capture_saver.flush();
    drop(model_saver);
    drop(capture_saver);
    Ok(())
}
