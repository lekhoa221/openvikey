//! Suggestion overlay (display-only): caps candidate lines and owns a Win32 HWND shell.

/// Take the first `max` candidate strings for on-screen overlay display.
#[must_use]
pub fn overlay_lines(candidates: &[String], max: usize) -> Vec<String> {
    candidates.iter().take(max).cloned().collect()
}

#[cfg(windows)]
mod hwnd_overlay {
    use std::sync::OnceLock;

    use windows::core::{w, Result, PCWSTR};
    use windows::Win32::Foundation::{HWND, HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::HBRUSH;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, HCURSOR, HICON, RegisterClassW,
        SetWindowPos,
        SetWindowTextW, ShowWindow, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, SW_HIDE,
        SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WNDCLASSW,
    };

    static CLASS_ATOM: OnceLock<u16> = OnceLock::new();

    fn register_class() -> Result<u16> {
        if let Some(atom) = CLASS_ATOM.get() {
            return Ok(*atom);
        }
        let class_name = w!("OpenViKeyOverlay");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(def_window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: HINSTANCE::default(),
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_name,
        };
        let atom = unsafe { RegisterClassW(&raw const wc) };
        if atom == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let _ = CLASS_ATOM.set(atom);
        Ok(atom)
    }

    unsafe extern "system" fn def_window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// Display-only popup HWND for suggestion candidates (not unit-tested in CI).
    pub struct OverlayWindow {
        hwnd: HWND,
    }

    impl OverlayWindow {
        /// Create the overlay popup (hidden until `show_at`).
        ///
        /// # Safety
        ///
        /// Calls Win32 window APIs.
        pub unsafe fn create() -> Result<Self> {
            register_class()?;
            let class_name = w!("OpenViKeyOverlay");
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                    class_name,
                    class_name,
                    WS_POPUP,
                    0,
                    0,
                    200,
                    80,
                    None,
                    None,
                    None,
                    None,
                )?
            };
            Ok(Self { hwnd })
        }

        #[must_use]
        pub fn hwnd(&self) -> HWND {
            self.hwnd
        }

        /// Update visible candidate lines (capped at three).
        ///
        /// # Safety
        ///
        /// Calls Win32 `SetWindowTextW`.
        pub unsafe fn set_lines(&self, lines: &[String]) {
            let text = super::overlay_lines(lines, 3).join("\n");
            let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
            unsafe {
                let _ = SetWindowTextW(self.hwnd, PCWSTR(wide.as_ptr()));
            }
        }

        /// Show the overlay at screen coordinates without activating.
        ///
        /// # Safety
        ///
        /// Calls Win32 `SetWindowPos` and `ShowWindow`.
        pub unsafe fn show_at(&self, x: i32, y: i32) {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    x,
                    y,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }

        /// Hide the overlay HWND.
        ///
        /// # Safety
        ///
        /// Calls Win32 `ShowWindow`.
        pub unsafe fn hide(&self) {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    impl Drop for OverlayWindow {
        fn drop(&mut self) {
            unsafe {
                if !self.hwnd.is_invalid() {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
        }
    }
}

#[cfg(windows)]
pub use hwnd_overlay::OverlayWindow;
