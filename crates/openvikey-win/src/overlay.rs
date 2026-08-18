//! Suggestion overlay (display-only): caps candidate lines and owns a Win32 HWND shell.

use std::sync::Mutex;

/// Take the first `max` candidate strings for on-screen overlay display.
#[must_use]
pub fn overlay_lines(candidates: &[String], max: usize) -> Vec<String> {
    candidates.iter().take(max).cloned().collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayPresentation {
    Hidden,
    Visible(Vec<String>),
}

#[must_use]
pub fn overlay_presentation(candidates: &[String], max: usize) -> OverlayPresentation {
    let lines = overlay_lines(candidates, max);
    if lines.is_empty() {
        OverlayPresentation::Hidden
    } else {
        OverlayPresentation::Visible(lines)
    }
}

static OVERLAY_HWND: Mutex<Option<isize>> = Mutex::new(None);

/// Bind the live overlay HWND so the host can push lines after releasing the typing mutex.
pub fn bind_overlay_hwnd(hwnd: isize) {
    if let Ok(mut guard) = OVERLAY_HWND.lock() {
        *guard = Some(hwnd);
    }
}

/// Push capped candidate lines to the overlay HWND (no-op if unbound).
pub fn push_overlay_lines(lines: &[String]) {
    #[cfg(windows)]
    {
        let hwnd = OVERLAY_HWND.lock().ok().and_then(|guard| *guard);
        let Some(raw) = hwnd else {
            return;
        };
        if raw == 0 {
            return;
        }
        hwnd_overlay::apply_overlay_presentation(raw, &overlay_presentation(lines, 3));
    }
    #[cfg(not(windows))]
    {
        let _ = lines;
    }
}

#[cfg(windows)]
mod hwnd_overlay {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetCursorPos, GetForegroundWindow, GetWindowRect,
        HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SetWindowPos, SetWindowTextW, ShowWindow, WS_BORDER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP,
    };
    use windows::core::{PCWSTR, Result, w};

    use super::OverlayPresentation;

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
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                    w!("STATIC"),
                    w!(""),
                    WS_POPUP | WS_BORDER,
                    0,
                    0,
                    320,
                    72,
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
            apply_overlay_presentation(
                self.hwnd.0 as isize,
                &super::overlay_presentation(lines, 3),
            );
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
                    SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOSIZE,
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
            super::bind_overlay_hwnd(0);
            unsafe {
                if !self.hwnd.is_invalid() {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
        }
    }

    pub(super) fn apply_overlay_presentation(hwnd_raw: isize, presentation: &OverlayPresentation) {
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        match presentation {
            OverlayPresentation::Hidden => unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            },
            OverlayPresentation::Visible(lines) => {
                let text = lines.join("\n");
                let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
                let (x, y) = overlay_anchor();
                unsafe {
                    let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        x,
                        y,
                        320,
                        72,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
            }
        }
    }

    fn overlay_anchor() -> (i32, i32) {
        unsafe {
            let foreground = GetForegroundWindow();
            let mut rect = RECT::default();
            if !foreground.is_invalid() && GetWindowRect(foreground, &raw mut rect).is_ok() {
                return (
                    rect.left.saturating_add(16),
                    rect.bottom.saturating_sub(112),
                );
            }
            let mut cursor = POINT::default();
            if GetCursorPos(&raw mut cursor).is_ok() {
                return (cursor.x.saturating_add(12), cursor.y.saturating_add(20));
            }
        }
        (16, 16)
    }
}

#[cfg(windows)]
pub use hwnd_overlay::OverlayWindow;
