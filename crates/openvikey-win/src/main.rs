//! OpenViKey Windows hook host coordinator.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use clap::{Parser, ValueEnum};
use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_core::types::{InputMethod, TonePlacement};
use openvikey_session::capture::ensure_distinct_store_paths;
use openvikey_session::session::{LabSession, SessionCursors};
#[cfg(windows)]
use openvikey_win::console::ConsoleControlHandler;
#[cfg(windows)]
use openvikey_win::focus::HostHooks;
use openvikey_win::focus::{FocusCache, run_host_message_loop};
use openvikey_win::host::{TypingHost, bind_persist_notify, bind_runtime};
use openvikey_win::inject::{InjectProfile, ProfilingInjector, SendInputSender};
use openvikey_win::persist::{
    HostShutdown, default_store_paths, ensure_open_store_cli_path, load_open_personal_store,
    spawn_open_pair_saver,
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
    /// Optional lexicon artifact. Debug builds embed the development lexicon by default.
    #[arg(long)]
    lexicon: Option<PathBuf>,
    /// Explicitly enable transformation in local terminal hosts. Learning/capture stay disabled.
    #[arg(long)]
    allow_terminal: bool,
    /// Open development model JSON path.
    #[arg(long)]
    model: Option<PathBuf>,
    /// Open development capture JSON path.
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

    let lexicon_bytes = if let Some(path) = &cli.lexicon {
        std::fs::read(path)?
    } else if cfg!(debug_assertions) {
        include_bytes!("../../../data/fixtures/lexicon/development.json").to_vec()
    } else {
        return Err("release builds require an explicit --lexicon artifact until G3 closes".into());
    };
    let artifact: LexiconArtifact = serde_json::from_slice(&lexicon_bytes)?;
    let lexicon = Lexicon::from_artifact(artifact);

    let (default_model, default_capture) =
        default_store_paths(std::env::var_os("LOCALAPPDATA").map(PathBuf::from));
    let model_path = cli.model.unwrap_or(default_model);
    let capture_path = cli.capture.unwrap_or(default_capture);
    ensure_open_store_cli_path(&model_path)?;
    ensure_open_store_cli_path(&capture_path)?;
    ensure_distinct_store_paths(&model_path, &capture_path)?;

    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (model, log) = load_open_personal_store(&model_path, &capture_path)?;
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
    typing.allow_terminal = cli.allow_terminal;
    let initial_mode = typing.mode;
    typing.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: SendInputSender,
        sending: Arc::clone(&sending),
    }));
    let host = Arc::new(Mutex::new(typing));
    let focus = Arc::new(FocusCache::new());
    bind_runtime(Arc::clone(&host), Arc::clone(&focus));

    let pair_saver = Arc::new(spawn_open_pair_saver(
        model_path.clone(),
        capture_path.clone(),
        Arc::clone(&host),
    ));

    let saver_for_notify = Arc::clone(&pair_saver);
    bind_persist_notify(Arc::new(move || {
        let _ = saver_for_notify.notify();
    }));

    let shutdown = Arc::new(HostShutdown::new());

    #[cfg(windows)]
    let console_control = ConsoleControlHandler::install(Arc::clone(&shutdown))?;
    #[cfg(windows)]
    let hooks = HostHooks::install(Arc::clone(&focus))?;

    #[cfg(windows)]
    let host_ui = install_host_ui(&shutdown, initial_mode)?;
    #[cfg(not(windows))]
    let _ = initial_mode;

    run_host_message_loop(&shutdown);

    #[cfg(windows)]
    drop(host_ui);
    #[cfg(windows)]
    drop(hooks);
    pair_saver.flush()?;
    #[cfg(windows)]
    drop(console_control);
    drop(pair_saver);
    Ok(())
}
