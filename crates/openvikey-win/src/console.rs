//! Graceful Windows console shutdown so Ctrl+C reaches the final persistence flush.

use crate::persist::HostShutdown;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleControl {
    CtrlC,
    CtrlBreak,
    Close,
    Logoff,
    Shutdown,
    Other,
}

/// Mark cooperative shutdown for console events owned by the Windows host.
#[must_use]
pub fn apply_console_control(control: ConsoleControl, shutdown: &HostShutdown) -> bool {
    if control == ConsoleControl::Other {
        return false;
    }
    shutdown.run();
    true
}

#[cfg(windows)]
mod windows_handler {
    use std::sync::{Arc, Mutex};

    use windows::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
        SetConsoleCtrlHandler,
    };
    use windows::core::{BOOL, Result};

    use super::{ConsoleControl, apply_console_control};
    use crate::persist::HostShutdown;

    static SHUTDOWN: Mutex<Option<Arc<HostShutdown>>> = Mutex::new(None);

    pub struct ConsoleControlHandler;

    impl ConsoleControlHandler {
        /// Install a handler that converts Ctrl+C/console close into cooperative shutdown.
        pub fn install(shutdown: Arc<HostShutdown>) -> Result<Self> {
            if let Ok(mut guard) = SHUTDOWN.lock() {
                *guard = Some(shutdown);
            }
            if let Err(error) = unsafe { SetConsoleCtrlHandler(Some(console_handler), true) } {
                if let Ok(mut guard) = SHUTDOWN.lock() {
                    *guard = None;
                }
                return Err(error);
            }
            Ok(Self)
        }
    }

    impl Drop for ConsoleControlHandler {
        fn drop(&mut self) {
            unsafe {
                let _ = SetConsoleCtrlHandler(Some(console_handler), false);
            }
            if let Ok(mut guard) = SHUTDOWN.lock() {
                *guard = None;
            }
        }
    }

    unsafe extern "system" fn console_handler(control: u32) -> BOOL {
        let control = match control {
            CTRL_C_EVENT => ConsoleControl::CtrlC,
            CTRL_BREAK_EVENT => ConsoleControl::CtrlBreak,
            CTRL_CLOSE_EVENT => ConsoleControl::Close,
            CTRL_LOGOFF_EVENT => ConsoleControl::Logoff,
            CTRL_SHUTDOWN_EVENT => ConsoleControl::Shutdown,
            _ => ConsoleControl::Other,
        };
        let handled = SHUTDOWN
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .map(|shutdown| apply_console_control(control, shutdown))
            })
            .unwrap_or(false);
        handled.into()
    }
}

#[cfg(windows)]
pub use windows_handler::ConsoleControlHandler;
