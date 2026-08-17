//! OpenViKey Windows hook host coordinator.

use std::path::PathBuf;
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
use openvikey_win::focus::{FocusCache, FocusHook};
use openvikey_win::passphrase::read_hidden_passphrase;
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
    let session = LabSession::new_with_model(
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
    let mut session = session;
    session.restore_capture(log.records);
    session.restore_last_at_ms(log.header.last_at_ms);
    let session = Arc::new(Mutex::new(session));

    let model_saver = DebouncedSaver::spawn_encrypted(model_path.clone(), provider.clone(), {
        let session = Arc::clone(&session);
        move || {
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            let snap = guard.save_snapshot();
            snap.model
                .to_json_payload()
                .map_err(|error| error.to_string())
        }
    });
    let capture_saver = DebouncedSaver::spawn_encrypted(capture_path.clone(), provider, {
        let session = Arc::clone(&session);
        move || {
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            let snap = guard.save_snapshot();
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
    let focus = Arc::new(FocusCache::new());

    #[cfg(windows)]
    let _hooks = install_hooks(Arc::clone(&focus))?;

    #[cfg(windows)]
    {
        // Free the console after passphrase so the host is tray/message-loop only.
        unsafe {
            let _ = windows::Win32::System::Console::FreeConsole();
        }
    }

    run_message_loop(&shutdown);

    let _ = model_saver.flush();
    let _ = capture_saver.flush();
    drop(model_saver);
    drop(capture_saver);
    Ok(())
}

#[cfg(windows)]
struct InstalledHooks {
    keyboard: windows::Win32::UI::WindowsAndMessaging::HHOOK,
    mouse: windows::Win32::UI::WindowsAndMessaging::HHOOK,
    _focus: FocusHook,
}

#[cfg(windows)]
impl Drop for InstalledHooks {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(self.keyboard);
            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(self.mouse);
        }
    }
}

#[cfg(windows)]
fn install_hooks(focus: Arc<FocusCache>) -> windows::core::Result<InstalledHooks> {
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowsHookExW, WH_KEYBOARD_LL, WH_MOUSE_LL,
    };

    let keyboard = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_ll_proc),
            Some(HINSTANCE::default()),
            0,
        )?
    };
    let mouse = unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(mouse_ll_proc),
            Some(HINSTANCE::default()),
            0,
        )?
    };
    let focus_hook = unsafe { FocusHook::install(focus)? };
    Ok(InstalledHooks {
        keyboard,
        mouse,
        _focus: focus_hook,
    })
}

#[cfg(windows)]
unsafe extern "system" fn keyboard_ll_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, HC_ACTION};
    if code == i32::try_from(HC_ACTION).unwrap_or(0) {
        // Coordinator stub: full TypingHost wiring is on the hook path modules.
        let _ = (wparam, lparam);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(windows)]
unsafe extern "system" fn mouse_ll_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, HC_ACTION};
    if code == i32::try_from(HC_ACTION).unwrap_or(0) {
        let _ = (wparam, lparam);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn run_message_loop(shutdown: &HostShutdown) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
        };
        while !shutdown.is_requested() {
            let mut msg = MSG::default();
            let has_msg = unsafe { PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE) };
            if has_msg.as_bool() {
                if msg.message == WM_QUIT {
                    shutdown.run();
                    break;
                }
                unsafe {
                    let _ = TranslateMessage(&raw const msg);
                    DispatchMessageW(&raw const msg);
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = shutdown;
    }
}
