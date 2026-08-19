//! Standalone focused-field classification; never reads field text.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use openvikey_win_context::ContextProjection;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::WindowsAndMessaging::{
    ES_PASSWORD, GUITHREADINFO, GWL_STYLE, GetClassNameW, GetGUIThreadInfo, GetWindowLongW,
};
use windows::core::{IUnknown, Result};

use crate::focus::{FocusCache, ForegroundSnapshot};
use crate::host::ContextProjectionSlot;

/// Security verdict for the currently focused field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldVerdict {
    Normal,
    Sensitive,
    Unavailable,
}

/// Already-sampled signals, separated from Win32 so precedence is unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SensitiveSignals {
    pub denied_process: bool,
    pub win32_password: bool,
    /// `Some(false)` is an explicit UI Automation normal-field result.
    pub automation_password: Option<bool>,
}

/// Resolve sensitive signals without ever consulting field value/text patterns.
#[must_use]
pub fn classify_sensitive_signals(signals: SensitiveSignals) -> FieldVerdict {
    if signals.denied_process || signals.win32_password || signals.automation_password == Some(true)
    {
        FieldVerdict::Sensitive
    } else if signals.automation_password == Some(false) {
        FieldVerdict::Normal
    } else {
        FieldVerdict::Unavailable
    }
}

/// In-process standalone context worker. Dropping it stops and joins the worker.
pub struct StandaloneContextGuard {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl StandaloneContextGuard {
    /// Start password classification and wait for the first bounded foreground verdict.
    pub fn start(focus: Arc<FocusCache>, projection: Arc<ContextProjectionSlot>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("openvikey-sensitive-field".to_owned())
            .spawn(move || run_worker(&focus, &projection, &worker_stop, &ready_tx))
            .map_err(|error| {
                windows::core::Error::new(windows::Win32::Foundation::E_FAIL, error.to_string())
            })?;
        ready_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| {
                windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    "standalone field classifier did not become ready",
                )
            })??;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for StandaloneContextGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_worker(
    focus: &FocusCache,
    projection: &ContextProjectionSlot,
    stop: &AtomicBool,
    ready: &mpsc::SyncSender<Result<()>>,
) {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if let Err(error) = initialized.ok() {
        let _ = ready.send(Err(error));
        return;
    }
    let automation: Result<IUIAutomation> =
        unsafe { CoCreateInstance(&CUIAutomation, None::<&IUnknown>, CLSCTX_INPROC_SERVER) };
    let Ok(automation) = automation else {
        let _ = ready.send(Err(automation.expect_err("checked error")));
        unsafe { CoUninitialize() };
        return;
    };

    publish_current(focus, projection, &automation);
    let _ = ready.send(Ok(()));
    while !stop.load(Ordering::Acquire) {
        publish_current(focus, projection, &automation);
        thread::sleep(Duration::from_millis(15));
    }
    unsafe { CoUninitialize() };
}

fn publish_current(
    focus: &FocusCache,
    projection: &ContextProjectionSlot,
    automation: &IUIAutomation,
) {
    let Some(before) = focus.try_get_foreground() else {
        return;
    };
    let verdict = classify_focused_field(&before, automation);
    let Some(after) = focus.try_get_foreground() else {
        return;
    };
    if before.identity != after.identity {
        return;
    }
    let value = match verdict {
        FieldVerdict::Normal => ContextProjection::Normal {
            left_token_nfc: None,
        },
        FieldVerdict::Sensitive => ContextProjection::Sensitive,
        FieldVerdict::Unavailable => ContextProjection::Unavailable,
    };
    projection.publish(before.identity, value);
}

#[allow(clippy::manual_map)] // windows BOOL's concrete crate path is intentionally not a dependency.
fn classify_focused_field(
    foreground: &ForegroundSnapshot,
    automation: &IUIAutomation,
) -> FieldVerdict {
    let denied_process = crate::policy::is_denylisted(&foreground.exe);
    let win32_password = focused_win32_password(foreground.identity.tid);
    let automation_result = unsafe { automation.GetFocusedElement() }
        .ok()
        .filter(|element| {
            unsafe { element.CurrentProcessId() }
                .ok()
                .and_then(|pid| u32::try_from(pid).ok())
                == Some(foreground.identity.pid)
        })
        .and_then(|element| unsafe { element.CurrentIsPassword() }.ok());
    let automation_password = if let Some(value) = automation_result {
        Some(value.as_bool())
    } else {
        None
    };
    classify_sensitive_signals(SensitiveSignals {
        denied_process,
        win32_password,
        automation_password,
    })
}

fn focused_win32_password(thread_id: u32) -> bool {
    if thread_id == 0 {
        return false;
    }
    let mut info = GUITHREADINFO {
        cbSize: u32::try_from(std::mem::size_of::<GUITHREADINFO>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(thread_id, &raw mut info) }.is_err()
        || info.hwndFocus == HWND::default()
    {
        return false;
    }
    let mut class_name = [0_u16; 64];
    let length = unsafe { GetClassNameW(info.hwndFocus, &mut class_name) };
    if length <= 0 {
        return false;
    }
    let name = String::from_utf16_lossy(&class_name[..usize::try_from(length).unwrap_or(0)]);
    name.eq_ignore_ascii_case("Edit")
        && unsafe { GetWindowLongW(info.hwndFocus, GWL_STYLE) } & ES_PASSWORD != 0
}
