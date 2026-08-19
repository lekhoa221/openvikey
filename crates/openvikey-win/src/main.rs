//! OpenViKey standalone Windows host coordinator.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use clap::{Parser, ValueEnum};
use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_core::types::InputMethod;
use openvikey_session::capture::ensure_distinct_store_paths;
use openvikey_session::session::{LabSession, SessionCursors};
#[cfg(windows)]
use openvikey_win::console::ConsoleControlHandler;
#[cfg(windows)]
use openvikey_win::focus::HostHooks;
use openvikey_win::focus::{FocusCache, run_host_message_loop, seed_current_foreground};
use openvikey_win::host::{
    ContextProjectionSlot, TypingHost, bind_persist_notify, bind_runtime_with_context_and_settings,
};
use openvikey_win::inject::{InjectProfile, ProfilingInjector, SendInputSender};
#[cfg(windows)]
use openvikey_win::instance::SingleInstance;
use openvikey_win::persist::{
    HostShutdown, default_store_paths, ensure_open_store_cli_path, load_open_personal_store,
    spawn_open_pair_saver,
};
use openvikey_win::policy::Mode;
#[cfg(windows)]
use openvikey_win::sensitive::StandaloneContextGuard;
use openvikey_win::settings::{default_settings_path, load_settings, save_settings};
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
    /// Optional development override; product startup otherwise uses settings.json.
    #[arg(long, value_enum)]
    method: Option<MethodArg>,
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

fn main() {
    if let Err(error) = run() {
        openvikey_win::control::show_startup_error(&error.to_string());
    }
}

fn startup_trace(stage: &str) {
    let Some(path) = std::env::var_os("OPENVIKEY_STARTUP_TRACE") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{stage}");
    }
}

#[allow(clippy::too_many_lines)] // Startup/shutdown ordering stays explicit in one coordinator.
fn run() -> Result<(), Box<dyn std::error::Error>> {
    startup_trace("enter");
    #[cfg(windows)]
    openvikey_win::control::init_dpi_awareness();
    #[cfg(windows)]
    let Some(_instance) = SingleInstance::acquire()? else {
        return Ok(());
    };
    startup_trace("single-instance");
    let cli = Cli::parse();
    startup_trace("cli");
    // Reserved for Electron SendInput gap (inject path).
    let _ = cli.electron_gap_ms;

    let lexicon_bytes = if let Some(path) = &cli.lexicon {
        std::fs::read(path)?
    } else {
        // Standalone preview embeds the project-authored development artifact.
        // It remains explicitly excluded from G3 production-corpus evidence.
        include_bytes!("../../../data/fixtures/lexicon/development.json").to_vec()
    };
    let artifact: LexiconArtifact = serde_json::from_slice(&lexicon_bytes)?;
    let lexicon = Lexicon::from_artifact(artifact);
    startup_trace("lexicon");

    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let settings_path = default_settings_path(local_app_data.as_ref());
    let mut settings = load_settings(&settings_path)?;
    #[cfg(windows)]
    {
        settings.start_with_windows = openvikey_win::startup::enabled();
    }
    openvikey_win::policy::set_runtime_hotkeys(&settings.hotkeys)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let method = cli.method.map_or(settings.input_method, InputMethod::from);

    let (default_model, default_capture) = default_store_paths(local_app_data.as_ref());
    let model_path = cli.model.unwrap_or(default_model);
    let capture_path = cli.capture.unwrap_or(default_capture);
    ensure_open_store_cli_path(&model_path)?;
    ensure_open_store_cli_path(&capture_path)?;
    ensure_distinct_store_paths(&model_path, &capture_path)?;

    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (model, log) = load_open_personal_store(&model_path, &capture_path)?;
    startup_trace("stores");
    let mut session = LabSession::new_with_model(
        EngineConfig {
            method,
            tone_placement: settings.tone_placement,
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
    typing.allow_terminal = cli.allow_terminal || settings.allow_terminal;
    typing
        .allow_terminal_flag
        .store(typing.allow_terminal, std::sync::atomic::Ordering::SeqCst);
    typing.app_policies.clone_from(&settings.app_policies);
    typing.set_show_suggestions(settings.show_suggestions);
    typing.set_initial_mode(if settings.starts_in_vietnamese() {
        Mode::Viet
    } else {
        Mode::English
    });
    let initial_mode = typing.mode;
    typing.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: SendInputSender,
        sending: Arc::clone(&sending),
    }));
    let host = Arc::new(Mutex::new(typing));
    let focus = Arc::new(FocusCache::new());
    seed_current_foreground(&focus);
    let context = Arc::new(ContextProjectionSlot::new());
    bind_runtime_with_context_and_settings(
        Arc::clone(&host),
        Arc::clone(&focus),
        Arc::clone(&context),
        Some(settings_path.clone()),
    );
    #[cfg(windows)]
    let context_guard = StandaloneContextGuard::start(Arc::clone(&focus), context)?;
    startup_trace("context-guard");

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
    let host_ui = install_host_ui(&shutdown, initial_mode)?;
    startup_trace("host-ui");

    #[cfg(windows)]
    let hooks = HostHooks::install(Arc::clone(&focus))?;
    startup_trace("hooks");
    #[cfg(not(windows))]
    let _ = initial_mode;

    startup_trace("message-loop");
    run_host_message_loop(&shutdown);
    startup_trace("shutdown");

    #[cfg(windows)]
    drop(hooks);
    #[cfg(windows)]
    drop(context_guard);
    #[cfg(windows)]
    drop(host_ui);
    pair_saver.flush()?;
    {
        let guard = host
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let engine = guard.session.engine_config();
        settings.input_method = engine.method;
        settings.tone_placement = engine.tone_placement;
        settings.last_mode_viet = guard.mode == Mode::Viet;
        settings.show_suggestions = guard.show_suggestions;
        settings.allow_terminal = guard.allow_terminal;
    }
    #[cfg(windows)]
    {
        settings.start_with_windows = openvikey_win::startup::enabled();
    }
    save_settings(&settings_path, &settings)?;
    #[cfg(windows)]
    drop(console_control);
    drop(pair_saver);
    Ok(())
}
