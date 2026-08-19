//! Development-only read-only probe for the GĐ2b TSF bridge.

use std::sync::Arc;
use std::time::{Duration, Instant};

use openvikey_win::context_bridge::ContextBridgeServer;
use openvikey_win::focus::{FocusCache, FocusHook};
use openvikey_win::host::ContextProjectionSlot;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
};

fn main() -> windows::core::Result<()> {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30);
    let focus = Arc::new(FocusCache::new());
    let projection = Arc::new(ContextProjectionSlot::new());
    let _focus_hook = unsafe { FocusHook::install(Arc::clone(&focus))? };
    let _server = ContextBridgeServer::start(focus, Arc::clone(&projection))?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut previous = None;
    while Instant::now() < deadline {
        pump_messages();
        if let Some(current) = projection.try_projection()
            && previous.as_ref() != Some(&current)
        {
            println!("{current:?}");
            previous = Some(current);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn pump_messages() {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
}
