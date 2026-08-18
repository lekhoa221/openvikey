//! Tray icon V/E display; left click maps to toggle mode.

use crate::persist::HostShutdown;
use crate::policy::{HostHotkey, Mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    LeftClick,
    Exit,
}

#[must_use]
pub fn tray_hotkey(event: TrayEvent) -> Option<HostHotkey> {
    match event {
        TrayEvent::LeftClick => Some(HostHotkey::ToggleMode),
        TrayEvent::Exit => None,
    }
}

/// Map a tray event: left-click → toggle; Exit → [`HostShutdown::run`].
pub fn apply_tray_event(event: TrayEvent, shutdown: &HostShutdown) -> Option<HostHotkey> {
    match event {
        TrayEvent::LeftClick => tray_hotkey(event),
        TrayEvent::Exit => {
            shutdown.run();
            None
        }
    }
}

/// Refresh the installed tray tooltip (no-op before [`install_host_ui`]).
pub fn set_tray_mode(mode: Mode) {
    #[cfg(windows)]
    {
        if let Ok(mut guard) = shell_tray::TRAY.lock()
            && let Some(icon) = guard.as_mut()
        {
            unsafe {
                icon.set_mode(mode);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = mode;
    }
}

#[cfg(windows)]
mod shell_tray {
    use std::sync::{Arc, Mutex};

    use windows::core::{w, Result, PCWSTR};
    use windows::Win32::Foundation::{HWND, HINSTANCE, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, NIF_ICON,
        NIF_MESSAGE, NIF_TIP,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        GetCursorPos, LoadIconW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
        TrackPopupMenu, CS_HREDRAW, CS_VREDRAW, HCURSOR, HICON, HWND_MESSAGE, IDI_APPLICATION,
        MF_STRING, TPM_RIGHTBUTTON, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
        WS_POPUP,
    };

    use crate::persist::HostShutdown;
    use crate::policy::Mode;

    pub static TRAY: Mutex<Option<TrayIcon>> = Mutex::new(None);
    static SHUTDOWN: Mutex<Option<Arc<HostShutdown>>> = Mutex::new(None);
    const ID_EXIT: usize = 1;

    /// Custom tray callback message (not unit-tested in CI).
    pub const WM_TRAYICON: u32 = 0x8000;

    /// Shell tray icon showing V/E mode in the tooltip.
    pub struct TrayIcon {
        data: NOTIFYICONDATAW,
    }

    // SAFETY: HWND/HICON inside NOTIFYICONDATAW are opaque Win32 handles; TrayIcon is only
    // touched from the host UI path and `set_tray_mode` under the global mutex.
    unsafe impl Send for TrayIcon {}

    impl TrayIcon {
        /// Register tray icon on `hwnd` message sink.
        ///
        /// # Safety
        ///
        /// Win32 shell notification APIs.
        pub unsafe fn install(hwnd: HWND, mode: Mode) -> Result<Self> {
            let hicon = unsafe { LoadIconW(None, IDI_APPLICATION)? };
            let mut data = NOTIFYICONDATAW {
                cbSize: u32::try_from(std::mem::size_of::<NOTIFYICONDATAW>())
                    .unwrap_or(u32::MAX),
                uID: 1,
                hWnd: hwnd,
                uFlags: NIF_MESSAGE | NIF_ICON,
                uCallbackMessage: WM_TRAYICON,
                hIcon: hicon,
                ..Default::default()
            };
            set_tip(&mut data, mode);
            let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &raw const data) };
            if !ok.as_bool() {
                return Err(windows::core::Error::from_thread());
            }
            Ok(Self { data })
        }

        /// Refresh tray tooltip for the current mode (V/E).
        ///
        /// # Safety
        ///
        /// Calls Win32 `Shell_NotifyIconW`.
        pub unsafe fn set_mode(&mut self, mode: Mode) {
            set_tip(&mut self.data, mode);
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &raw const self.data);
            }
        }
    }

    impl Drop for TrayIcon {
        fn drop(&mut self) {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &raw const self.data);
            }
        }
    }

    fn set_tip(data: &mut NOTIFYICONDATAW, mode: Mode) {
        let tip = tray_tip(mode);
        let wide: Vec<u16> = tip.encode_utf16().chain([0]).collect();
        let len = wide.len().saturating_sub(1);
        let copy_len = len.min(data.szTip.len());
        for (i, ch) in wide[..copy_len].iter().enumerate() {
            data.szTip[i] = *ch;
        }
        if copy_len < data.szTip.len() {
            data.szTip[copy_len] = 0;
        }
        data.uFlags |= NIF_TIP;
    }

    fn tray_tip(mode: Mode) -> String {
        match mode {
            Mode::Viet => "OpenViKey — V".to_owned(),
            Mode::English => "OpenViKey — E".to_owned(),
        }
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
    }

    fn peek_shutdown() -> Option<Arc<HostShutdown>> {
        SHUTDOWN
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(Arc::clone))
    }

    unsafe extern "system" fn tray_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_TRAYICON {
            let mouse = u32::try_from(lparam.0.cast_unsigned()).unwrap_or(0);
            if mouse == WM_LBUTTONUP {
                if let Some(shutdown) = peek_shutdown()
                    && super::apply_tray_event(super::TrayEvent::LeftClick, &shutdown)
                        == Some(crate::policy::HostHotkey::ToggleMode)
                {
                    crate::host::handle_tray_left_click(now_ms());
                }
            } else if mouse == WM_RBUTTONUP {
                show_exit_menu(hwnd);
            }
            return LRESULT(0);
        }
        if msg == WM_COMMAND {
            let id = wparam.0 & 0xFFFF;
            if id == ID_EXIT {
                if let Some(shutdown) = peek_shutdown() {
                    super::apply_tray_event(super::TrayEvent::Exit, &shutdown);
                }
                unsafe {
                    PostQuitMessage(0);
                }
                return LRESULT(0);
            }
        }
        if msg == WM_DESTROY {
            unsafe {
                PostQuitMessage(0);
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn show_exit_menu(hwnd: HWND) {
        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        let _ = unsafe { AppendMenuW(menu, MF_STRING, ID_EXIT, w!("E&xit")) };
        let mut pt = POINT::default();
        let _ = unsafe { GetCursorPos(&raw mut pt) };
        unsafe {
            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
            let _ = DestroyMenu(menu);
        }
    }

    fn register_sink_class() -> Result<()> {
        let class_name = w!("OpenViKeyTraySink");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(tray_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: HINSTANCE::default(),
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_name,
        };
        let atom = unsafe { RegisterClassW(&raw const wc) };
        if atom == 0 {
            let err = windows::core::Error::from_thread();
            if err.code().0 & 0xFFFF == 1410 {
                // class already exists
                return Ok(());
            }
            return Err(err);
        }
        Ok(())
    }

    /// Overlay + tray + Exit sink for the host coordinator.
    pub struct HostUi {
        _overlay: crate::overlay::OverlayWindow,
        sink: HWND,
    }

    /// Install overlay HWND, tray V/E icon, and Exit menu (allowlisted FFI).
    pub fn install_host_ui(shutdown: &Arc<HostShutdown>, mode: Mode) -> Result<HostUi> {
        if let Ok(mut guard) = SHUTDOWN.lock() {
            *guard = Some(Arc::clone(shutdown));
        }
        register_sink_class()?;
        let sink = unsafe {
            CreateWindowExW(
                windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE::default(),
                w!("OpenViKeyTraySink"),
                w!("OpenViKeyTraySink"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )?
        };
        let overlay = unsafe { crate::overlay::OverlayWindow::create()? };
        crate::overlay::bind_overlay_hwnd(overlay.hwnd().0 as isize);
        let tray = unsafe { TrayIcon::install(sink, mode)? };
        if let Ok(mut guard) = TRAY.lock() {
            *guard = Some(tray);
        }
        Ok(HostUi {
            _overlay: overlay,
            sink,
        })
    }

    impl Drop for HostUi {
        fn drop(&mut self) {
            if let Ok(mut guard) = TRAY.lock() {
                *guard = None;
            }
            if let Ok(mut guard) = SHUTDOWN.lock() {
                *guard = None;
            }
            unsafe {
                if !self.sink.is_invalid() {
                    let _ = DestroyWindow(self.sink);
                }
            }
        }
    }
}

#[cfg(windows)]
pub use shell_tray::{install_host_ui, HostUi, TrayIcon, WM_TRAYICON};
