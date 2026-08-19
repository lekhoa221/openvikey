//! Tray icon V/E display; left click maps to toggle mode; right-click Gợi ý hides overlay.

use crate::persist::HostShutdown;
use crate::policy::{HostHotkey, Mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    LeftClick,
    ToggleSuggestions,
    Exit,
}

#[must_use]
pub fn tray_hotkey(event: TrayEvent) -> Option<HostHotkey> {
    match event {
        TrayEvent::LeftClick => Some(HostHotkey::ToggleMode),
        TrayEvent::ToggleSuggestions => Some(HostHotkey::ToggleSuggestions),
        TrayEvent::Exit => None,
    }
}

/// Map a tray event: left-click → mode; menu Gợi ý → overlay; Exit → [`HostShutdown::run`].
pub fn apply_tray_event(event: TrayEvent, shutdown: &HostShutdown) -> Option<HostHotkey> {
    match event {
        TrayEvent::LeftClick | TrayEvent::ToggleSuggestions => tray_hotkey(event),
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

    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
        Shell_NotifyIconW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CS_HREDRAW, CS_VREDRAW, CreateIcon, CreatePopupMenu, CreateWindowExW,
        DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, GetCursorPos, HCURSOR, HICON,
        MF_CHECKED, MF_STRING, PostQuitMessage, RegisterClassW, SetForegroundWindow,
        TPM_RIGHTBUTTON, TrackPopupMenu, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP,
        WNDCLASSW, WS_POPUP,
    };
    use windows::core::{PCWSTR, Result, w};

    use crate::persist::HostShutdown;
    use crate::policy::Mode;

    pub static TRAY: Mutex<Option<TrayIcon>> = Mutex::new(None);
    static SHUTDOWN: Mutex<Option<Arc<HostShutdown>>> = Mutex::new(None);
    const ID_EXIT: usize = 1;
    const ID_SUGGESTIONS: usize = 2;
    const ID_SETTINGS: usize = 3;
    const ID_TELEX: usize = 4;
    const ID_VNI: usize = 5;
    const ID_AUTOSTART: usize = 6;
    const ID_FORGET_LAST: usize = 7;

    /// Custom tray callback messages (not unit-tested in CI).
    pub const WM_TRAYICON: u32 = 0x8000;
    pub const WM_OPEN_SETTINGS: u32 = 0x8001;

    /// Shell tray icon showing V/E mode in the tooltip.
    pub struct TrayIcon {
        data: NOTIFYICONDATAW,
        icon: HICON,
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
            let hicon = create_mode_icon(mode)?;
            let mut data = NOTIFYICONDATAW {
                cbSize: u32::try_from(std::mem::size_of::<NOTIFYICONDATAW>()).unwrap_or(u32::MAX),
                uID: 1,
                hWnd: hwnd,
                uFlags: NIF_MESSAGE | NIF_ICON,
                uCallbackMessage: WM_TRAYICON,
                hIcon: hicon,
                ..Default::default()
            };
            set_tip(&mut data, mode);
            let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &raw const data) };
            Ok(Self { data, icon: hicon })
        }

        /// Refresh tray tooltip for the current mode (V/E).
        ///
        /// # Safety
        ///
        /// Calls Win32 `Shell_NotifyIconW`.
        pub unsafe fn set_mode(&mut self, mode: Mode) {
            set_tip(&mut self.data, mode);
            if let Ok(icon) = create_mode_icon(mode) {
                let old = self.icon;
                self.icon = icon;
                self.data.hIcon = icon;
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_MODIFY, &raw const self.data);
                    let _ = DestroyIcon(old);
                }
            } else {
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_MODIFY, &raw const self.data);
                }
            }
        }
    }

    impl Drop for TrayIcon {
        fn drop(&mut self) {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &raw const self.data);
                let _ = DestroyIcon(self.icon);
            }
        }
    }

    fn create_mode_icon(mode: Mode) -> Result<HICON> {
        let rows: [u16; 16] = match mode {
            Mode::Viet => [
                0, 0, 0x6006, 0x6006, 0x300C, 0x300C, 0x1818, 0x1818, 0x0C30, 0x0C30, 0x0660,
                0x0660, 0x03C0, 0x0180, 0, 0,
            ],
            Mode::English => [
                0, 0, 0x7FFE, 0x6000, 0x6000, 0x6000, 0x7FF0, 0x6000, 0x6000, 0x6000, 0x6000,
                0x6000, 0x7FFE, 0, 0, 0,
            ],
        };
        let mut xor = [0_u8; 32];
        for (index, row) in rows.iter().enumerate() {
            let [high, low] = row.to_be_bytes();
            xor[index * 2] = high;
            xor[index * 2 + 1] = low;
        }
        let and = [0_u8; 32];
        unsafe { CreateIcon(None, 16, 16, 1, 1, and.as_ptr(), xor.as_ptr()) }
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
        if msg == WM_OPEN_SETTINGS {
            crate::control::show_control_window(hwnd);
            return LRESULT(0);
        }
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
                show_tray_menu(hwnd);
            }
            return LRESULT(0);
        }
        if msg == WM_COMMAND {
            let id = wparam.0 & 0xFFFF;
            if id == ID_SETTINGS {
                crate::control::show_control_window(hwnd);
                return LRESULT(0);
            }
            if id == ID_TELEX || id == ID_VNI {
                let method = if id == ID_TELEX {
                    openvikey_core::types::InputMethod::Telex
                } else {
                    openvikey_core::types::InputMethod::Vni
                };
                crate::host::set_input_method_runtime(method, now_ms());
                return LRESULT(0);
            }
            if id == ID_AUTOSTART {
                let _ = crate::startup::set_enabled(!crate::startup::enabled());
                return LRESULT(0);
            }
            if id == ID_FORGET_LAST {
                crate::host::forget_last_rule_runtime(now_ms());
                return LRESULT(0);
            }
            if id == ID_SUGGESTIONS {
                if let Some(shutdown) = peek_shutdown()
                    && super::apply_tray_event(super::TrayEvent::ToggleSuggestions, &shutdown)
                        == Some(crate::policy::HostHotkey::ToggleSuggestions)
                {
                    crate::host::handle_tray_toggle_suggestions(now_ms());
                }
                return LRESULT(0);
            }
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

    fn show_tray_menu(hwnd: HWND) {
        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        let suggestion_flags = if crate::host::suggestions_visible() {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let current_method =
            crate::host::control_snapshot().map(|value| value.engine_config.method);
        let telex_flags = if current_method == Some(openvikey_core::types::InputMethod::Telex) {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let vni_flags = if current_method == Some(openvikey_core::types::InputMethod::Vni) {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let autostart_flags = if crate::startup::enabled() {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                ID_SETTINGS,
                w!("Cài đặt và rule đã học..."),
            )
        };
        let _ = unsafe { AppendMenuW(menu, telex_flags, ID_TELEX, w!("Kiểu gõ Telex")) };
        let _ = unsafe { AppendMenuW(menu, vni_flags, ID_VNI, w!("Kiểu gõ VNI")) };
        let _ =
            unsafe { AppendMenuW(menu, suggestion_flags, ID_SUGGESTIONS, w!("Hiện gợi ý")) };
        let _ = unsafe {
            AppendMenuW(
                menu,
                autostart_flags,
                ID_AUTOSTART,
                w!("Khởi động cùng Windows"),
            )
        };
        let _ =
            unsafe { AppendMenuW(menu, MF_STRING, ID_FORGET_LAST, w!("Quên rule vừa học")) };
        let _ = unsafe { AppendMenuW(menu, MF_STRING, ID_EXIT, w!("Thoát")) };
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

    /// Overlay + tray + Gợi ý/Exit sink for the host coordinator.
    pub struct HostUi {
        _overlay: crate::overlay::OverlayWindow,
        sink: HWND,
    }

    /// Install overlay HWND, tray V/E icon, and Gợi ý/Exit menu (allowlisted FFI).
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
                None,
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
pub use shell_tray::{HostUi, TrayIcon, WM_OPEN_SETTINGS, WM_TRAYICON, install_host_ui};
