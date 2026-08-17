//! Tray icon V/E display; left click maps to toggle mode.

use crate::policy::HostHotkey;

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

#[cfg(windows)]
mod shell_tray {
    use windows::core::Result;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, NIF_TIP,
    };

    use crate::policy::Mode;

    /// Custom tray callback message (not unit-tested in CI).
    pub const WM_TRAYICON: u32 = 0x8000;

    /// Shell tray icon showing V/E mode in the tooltip.
    pub struct TrayIcon {
        data: NOTIFYICONDATAW,
    }

    impl TrayIcon {
        /// Register tray icon on `hwnd` message sink.
        ///
        /// # Safety
        ///
        /// Win32 shell notification APIs.
        pub unsafe fn install(hwnd: HWND, mode: Mode) -> Result<Self> {
            let mut data = NOTIFYICONDATAW {
                uID: 1,
                hWnd: hwnd,
                uFlags: NIF_TIP,
                uCallbackMessage: WM_TRAYICON,
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
}

#[cfg(windows)]
pub use shell_tray::{TrayIcon, WM_TRAYICON};
