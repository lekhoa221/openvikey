//! Compact, display-only suggestion and learning capsule.

use openvikey_session::session::LearningNotice;

/// Take the first `max` candidate strings for display-related callers.
#[must_use]
pub fn overlay_lines(candidates: &[String], max: usize) -> Vec<String> {
    candidates.iter().take(max).cloned().collect()
}

/// Cap overlay lines, or return empty when the user has hidden suggestions.
#[must_use]
pub fn overlay_display_lines(candidates: &[String], max: usize, show: bool) -> Vec<String> {
    if show {
        overlay_lines(candidates, max)
    } else {
        Vec::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayPresentation {
    Hidden,
    Suggestion {
        candidate: String,
        position: usize,
        total: usize,
    },
    Learning(LearningNotice),
}

#[must_use]
pub fn overlay_presentation(candidates: &[String], max: usize) -> OverlayPresentation {
    overlay_presentation_if(candidates, max, true)
}

#[must_use]
pub fn overlay_presentation_if(
    candidates: &[String],
    max: usize,
    show: bool,
) -> OverlayPresentation {
    let lines = overlay_display_lines(candidates, max, show);
    let Some(candidate) = lines.first() else {
        return OverlayPresentation::Hidden;
    };
    OverlayPresentation::Suggestion {
        candidate: candidate.clone(),
        position: 1,
        total: lines.len(),
    }
}

/// Bind the live overlay HWND so the host can push presentations after releasing its mutex.
pub fn bind_overlay_hwnd(hwnd: isize) {
    #[cfg(windows)]
    hwnd_overlay::bind(hwnd);
    #[cfg(not(windows))]
    let _ = hwnd;
}

/// Push the latest top candidate to the compact capsule (no-op if unbound).
pub fn push_overlay_lines(lines: &[String]) {
    push_overlay_presentation(overlay_presentation(lines, 3));
}

/// Give a learning result priority over ordinary candidate updates.
pub fn push_learning_notice(notice: LearningNotice) {
    push_overlay_presentation(OverlayPresentation::Learning(notice));
}

/// Force-hide the capsule, including a currently timed learning notice.
pub fn dismiss_overlay() {
    #[cfg(windows)]
    hwnd_overlay::queue_dismiss();
}

fn push_overlay_presentation(presentation: OverlayPresentation) {
    #[cfg(windows)]
    hwnd_overlay::queue_presentation(presentation);
    #[cfg(not(windows))]
    let _ = presentation;
}

#[cfg(windows)]
mod hwnd_overlay {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use openvikey_session::session::{LearningNotice, LearningNoticeKind};
    use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap,
        CreateCompatibleDC, CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush,
        DEFAULT_CHARSET, DEFAULT_PITCH, DeleteDC, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE,
        FW_NORMAL, GetMonitorInfoW, HDC, HFONT, InvalidateRect, MONITOR_DEFAULTTONEAREST,
        MONITORINFO, MonitorFromWindow, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect,
        SRCCOPY, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
        GetForegroundWindow, GetWindowRect, HTTRANSPARENT, HWND_TOPMOST, KillTimer, PostMessageW,
        RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetTimer,
        SetWindowPos, SetWindowTextW, ShowWindow, WM_APP, WM_ERASEBKGND, WM_NCHITTEST, WM_PAINT,
        WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        WS_POPUP,
    };
    use windows::core::{PCWSTR, Result, w};

    use super::OverlayPresentation;

    const WM_APPLY_PRESENTATION: u32 = WM_APP + 41;
    const HIDE_TIMER_ID: usize = 1;
    const SUGGESTION_DURATION_MS: u32 = 2_000;
    const LEARNING_DURATION_MS: u32 = 2_500;
    const BASE_SUGGESTION_WIDTH: i32 = 304;
    const BASE_SUGGESTION_HEIGHT: i32 = 40;
    const BASE_LEARNING_WIDTH: i32 = 360;
    const BASE_LEARNING_HEIGHT: i32 = 58;

    #[derive(Debug, Clone)]
    enum OverlayCommand {
        Present(OverlayPresentation),
        Dismiss,
    }

    struct DisplayState {
        current: OverlayPresentation,
        pending_suggestion: Option<OverlayPresentation>,
    }

    impl Default for DisplayState {
        fn default() -> Self {
            Self {
                current: OverlayPresentation::Hidden,
                pending_suggestion: None,
            }
        }
    }

    static OVERLAY_HWND: Mutex<Option<isize>> = Mutex::new(None);
    static COMMANDS: Mutex<VecDeque<OverlayCommand>> = Mutex::new(VecDeque::new());
    static DISPLAY: Mutex<Option<DisplayState>> = Mutex::new(None);

    pub(super) fn bind(hwnd: isize) {
        if let Ok(mut guard) = OVERLAY_HWND.lock() {
            *guard = (hwnd != 0).then_some(hwnd);
        }
    }

    pub(super) fn queue_presentation(presentation: OverlayPresentation) {
        queue(OverlayCommand::Present(presentation));
    }

    pub(super) fn queue_dismiss() {
        queue(OverlayCommand::Dismiss);
    }

    fn queue(command: OverlayCommand) {
        let hwnd = OVERLAY_HWND.lock().ok().and_then(|guard| *guard);
        let Some(raw) = hwnd else {
            return;
        };
        if let Ok(mut commands) = COMMANDS.lock() {
            if commands.len() >= 8 {
                commands.pop_front();
            }
            commands.push_back(command);
        }
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APPLY_PRESENTATION, WPARAM(0), LPARAM(0));
        }
    }

    /// Display-only popup HWND for suggestions and local learning feedback.
    pub struct OverlayWindow {
        hwnd: HWND,
    }

    impl OverlayWindow {
        /// Create the capsule, hidden until the first presentation.
        ///
        /// # Safety
        ///
        /// Calls Win32 window APIs and must run on the product UI thread.
        pub unsafe fn create() -> Result<Self> {
            register_capsule_class()?;
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT,
                    w!("OpenViKeySuggestionCapsule"),
                    w!(""),
                    WS_POPUP,
                    0,
                    0,
                    BASE_SUGGESTION_WIDTH,
                    BASE_SUGGESTION_HEIGHT,
                    None,
                    None,
                    None,
                    None,
                )?
            };
            if let Ok(mut guard) = DISPLAY.lock() {
                *guard = Some(DisplayState::default());
            }
            bind(hwnd.0 as isize);
            Ok(Self { hwnd })
        }

        #[must_use]
        pub fn hwnd(&self) -> HWND {
            self.hwnd
        }

        /// Direct update used by native smoke tests.
        ///
        /// # Safety
        ///
        /// Calls Win32 rendering/window APIs on the owner thread.
        pub unsafe fn set_lines(&self, lines: &[String]) {
            apply_command(
                self.hwnd,
                OverlayCommand::Present(super::overlay_presentation(lines, 3)),
            );
        }

        /// Direct typed presentation used by native smoke tests.
        ///
        /// # Safety
        ///
        /// Calls Win32 rendering/window APIs on the owner thread.
        pub unsafe fn set_presentation(&self, presentation: OverlayPresentation) {
            apply_command(self.hwnd, OverlayCommand::Present(presentation));
        }

        /// Force-hide this capsule.
        ///
        /// # Safety
        ///
        /// Calls Win32 window APIs on the owner thread.
        pub unsafe fn hide(&self) {
            apply_command(self.hwnd, OverlayCommand::Dismiss);
        }
    }

    impl Drop for OverlayWindow {
        fn drop(&mut self) {
            bind(0);
            if let Ok(mut guard) = DISPLAY.lock() {
                *guard = None;
            }
            unsafe {
                if !self.hwnd.is_invalid() {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
        }
    }

    fn register_capsule_class() -> Result<()> {
        let wc = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(capsule_wnd_proc),
            hInstance: HINSTANCE::default(),
            lpszClassName: w!("OpenViKeySuggestionCapsule"),
            ..Default::default()
        };
        let atom = unsafe { RegisterClassW(&raw const wc) };
        if atom == 0 {
            let error = windows::core::Error::from_thread();
            if error.code().0 & 0xFFFF != 1410 {
                return Err(error);
            }
        }
        Ok(())
    }

    unsafe extern "system" fn capsule_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_APPLY_PRESENTATION => {
                let commands = COMMANDS
                    .lock()
                    .map(|mut queue| queue.drain(..).collect::<Vec<_>>())
                    .unwrap_or_default();
                for command in commands {
                    apply_command(hwnd, command);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == HIDE_TIMER_ID => {
                expire_current(hwnd);
                LRESULT(0)
            }
            WM_PAINT => {
                paint_capsule(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_NCHITTEST => LRESULT(isize::try_from(HTTRANSPARENT).unwrap_or(-1)),
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    fn apply_command(hwnd: HWND, command: OverlayCommand) {
        let Ok(mut display) = DISPLAY.lock() else {
            return;
        };
        let Some(state) = display.as_mut() else {
            return;
        };
        match command {
            OverlayCommand::Dismiss => hide_now(hwnd, state),
            OverlayCommand::Present(OverlayPresentation::Hidden) => {
                state.pending_suggestion = None;
                if !matches!(state.current, OverlayPresentation::Learning(_)) {
                    hide_now(hwnd, state);
                }
            }
            OverlayCommand::Present(presentation @ OverlayPresentation::Suggestion { .. }) => {
                if matches!(state.current, OverlayPresentation::Learning(_)) {
                    state.pending_suggestion = Some(presentation);
                } else {
                    show_now(hwnd, state, presentation);
                }
            }
            OverlayCommand::Present(presentation @ OverlayPresentation::Learning(_)) => {
                state.pending_suggestion = None;
                show_now(hwnd, state, presentation);
            }
        }
    }

    fn expire_current(hwnd: HWND) {
        let Ok(mut display) = DISPLAY.lock() else {
            return;
        };
        let Some(state) = display.as_mut() else {
            return;
        };
        if matches!(state.current, OverlayPresentation::Learning(_))
            && let Some(pending) = state.pending_suggestion.take()
        {
            show_now(hwnd, state, pending);
            return;
        }
        hide_now(hwnd, state);
    }

    fn show_now(hwnd: HWND, state: &mut DisplayState, presentation: OverlayPresentation) {
        let dpi = active_dpi();
        let (base_width, base_height, duration) = match presentation {
            OverlayPresentation::Suggestion { .. } => (
                BASE_SUGGESTION_WIDTH,
                BASE_SUGGESTION_HEIGHT,
                SUGGESTION_DURATION_MS,
            ),
            OverlayPresentation::Learning(_) => (
                BASE_LEARNING_WIDTH,
                BASE_LEARNING_HEIGHT,
                LEARNING_DURATION_MS,
            ),
            OverlayPresentation::Hidden => {
                hide_now(hwnd, state);
                return;
            }
        };
        let width = scale_dpi(base_width, dpi);
        let height = scale_dpi(base_height, dpi);
        if is_foreground_fullscreen() {
            hide_now(hwnd, state);
            return;
        }
        let (x, y) = capsule_anchor(width, height);
        let caption = accessible_text(&presentation);
        let wide: Vec<u16> = caption.encode_utf16().chain([0]).collect();
        state.current = presentation;
        unsafe {
            let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let radius = scale_dpi(12, dpi);
            let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius, radius);
            if !region.is_invalid() && SetWindowRgn(hwnd, Some(region), true) == 0 {
                let _ = DeleteObject(region.into());
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = KillTimer(Some(hwnd), HIDE_TIMER_ID);
            let _ = SetTimer(Some(hwnd), HIDE_TIMER_ID, duration, None);
        }
    }

    fn hide_now(hwnd: HWND, state: &mut DisplayState) {
        state.current = OverlayPresentation::Hidden;
        state.pending_suggestion = None;
        unsafe {
            let _ = KillTimer(Some(hwnd), HIDE_TIMER_ID);
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }

    fn accessible_text(presentation: &OverlayPresentation) -> String {
        match presentation {
            OverlayPresentation::Hidden => String::new(),
            OverlayPresentation::Suggestion {
                candidate,
                position,
                total,
            } => format!("Gợi ý: {candidate} · Ctrl+. · {position}/{total}"),
            OverlayPresentation::Learning(notice) => notice.display_text(),
        }
    }

    fn active_dpi() -> u32 {
        let foreground = unsafe { GetForegroundWindow() };
        if foreground.is_invalid() {
            96
        } else {
            unsafe { GetDpiForWindow(foreground) }.max(96)
        }
    }

    fn capsule_anchor(width: i32, height: i32) -> (i32, i32) {
        let foreground = unsafe { GetForegroundWindow() };
        let monitor = unsafe { MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).unwrap_or(u32::MAX),
            ..Default::default()
        };
        if !monitor.is_invalid() && unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
            let margin = scale_dpi(12, active_dpi());
            return (
                info.rcWork
                    .right
                    .saturating_sub(width)
                    .saturating_sub(margin),
                info.rcWork
                    .bottom
                    .saturating_sub(height)
                    .saturating_sub(margin),
            );
        }
        (16, 16)
    }

    fn is_foreground_fullscreen() -> bool {
        let foreground = unsafe { GetForegroundWindow() };
        if foreground.is_invalid() {
            return false;
        }
        let monitor = unsafe { MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).unwrap_or(u32::MAX),
            ..Default::default()
        };
        let mut rect = RECT::default();
        if monitor.is_invalid()
            || !unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool()
            || unsafe { GetWindowRect(foreground, &raw mut rect) }.is_err()
        {
            return false;
        }
        rect.left <= info.rcMonitor.left
            && rect.top <= info.rcMonitor.top
            && rect.right >= info.rcMonitor.right
            && rect.bottom >= info.rcMonitor.bottom
    }

    fn scale_dpi(value: i32, dpi: u32) -> i32 {
        value.saturating_mul(dpi.cast_signed()).saturating_add(48) / 96
    }

    fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
        COLORREF(u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16))
    }

    fn create_font(dpi: u32) -> HFONT {
        unsafe {
            CreateFontW(
                -scale_dpi(14, dpi),
                0,
                0,
                0,
                FW_NORMAL.0.cast_signed(),
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                u32::from(DEFAULT_PITCH.0 | FF_DONTCARE.0),
                w!("Segoe UI"),
            )
        }
    }

    fn paint_capsule(hwnd: HWND) {
        let presentation = DISPLAY
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|state| state.current.clone()))
            .unwrap_or(OverlayPresentation::Hidden);
        if presentation == OverlayPresentation::Hidden {
            return;
        }
        let mut paint = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &raw mut paint) };
        let mut client = RECT::default();
        if unsafe { GetClientRect(hwnd, &raw mut client) }.is_err() {
            unsafe {
                let _ = EndPaint(hwnd, &raw const paint);
            }
            return;
        }
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        let memory = unsafe { CreateCompatibleDC(Some(hdc)) };
        let bitmap = unsafe { CreateCompatibleBitmap(hdc, width, height) };
        if memory.is_invalid() || bitmap.is_invalid() {
            unsafe {
                if !bitmap.is_invalid() {
                    let _ = DeleteObject(bitmap.into());
                }
                if !memory.is_invalid() {
                    let _ = DeleteDC(memory);
                }
                let _ = EndPaint(hwnd, &raw const paint);
            }
            return;
        }
        let previous_bitmap = unsafe { SelectObject(memory, bitmap.into()) };
        draw_capsule(memory, width, height, &presentation, active_dpi());
        unsafe {
            let _ = BitBlt(hdc, 0, 0, width, height, Some(memory), 0, 0, SRCCOPY);
            let _ = SelectObject(memory, previous_bitmap);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            let _ = EndPaint(hwnd, &raw const paint);
        }
    }

    fn draw_capsule(
        hdc: HDC,
        width: i32,
        height: i32,
        presentation: &OverlayPresentation,
        dpi: u32,
    ) {
        let learning = matches!(presentation, OverlayPresentation::Learning(_));
        let forgotten = matches!(
            presentation,
            OverlayPresentation::Learning(LearningNotice {
                kind: LearningNoticeKind::Forgotten,
                ..
            })
        );
        let background = if forgotten {
            rgb(248, 250, 252)
        } else if learning {
            rgb(236, 253, 243)
        } else {
            rgb(255, 255, 255)
        };
        let border = if forgotten {
            rgb(203, 213, 225)
        } else if learning {
            rgb(134, 239, 172)
        } else {
            rgb(191, 219, 254)
        };
        let brush = unsafe { CreateSolidBrush(background) };
        let pen = unsafe { CreatePen(PS_SOLID, 1, border) };
        let old_brush = unsafe { SelectObject(hdc, brush.into()) };
        let old_pen = unsafe { SelectObject(hdc, pen.into()) };
        let radius = scale_dpi(12, dpi);
        unsafe {
            let _ = RoundRect(hdc, 0, 0, width, height, radius, radius);
            let _ = SetBkMode(hdc, TRANSPARENT);
        }

        let font = create_font(dpi);
        let old_font = unsafe { SelectObject(hdc, font.into()) };
        match presentation {
            OverlayPresentation::Suggestion {
                candidate,
                position,
                total,
            } => draw_suggestion(hdc, width, height, candidate, *position, *total, dpi),
            OverlayPresentation::Learning(notice) => {
                draw_learning(hdc, width, height, notice, dpi);
            }
            OverlayPresentation::Hidden => {}
        }
        unsafe {
            let _ = SelectObject(hdc, old_font);
            let _ = SelectObject(hdc, old_pen);
            let _ = SelectObject(hdc, old_brush);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(pen.into());
            let _ = DeleteObject(brush.into());
        }
    }

    fn draw_suggestion(
        hdc: HDC,
        width: i32,
        height: i32,
        candidate: &str,
        position: usize,
        total: usize,
        dpi: u32,
    ) {
        let pad = scale_dpi(14, dpi);
        let mut label_rect = RECT {
            left: pad,
            top: 0,
            right: scale_dpi(66, dpi),
            bottom: height,
        };
        draw_text(hdc, "Gợi ý", &mut label_rect, rgb(37, 99, 235));

        let mut candidate_rect = RECT {
            left: scale_dpi(68, dpi),
            top: 0,
            right: width - scale_dpi(108, dpi),
            bottom: height,
        };
        draw_text(hdc, candidate, &mut candidate_rect, rgb(15, 23, 42));

        let mut shortcut_rect = RECT {
            left: width - scale_dpi(104, dpi),
            top: 0,
            right: width - scale_dpi(46, dpi),
            bottom: height,
        };
        draw_text(hdc, "Ctrl+.", &mut shortcut_rect, rgb(71, 85, 105));

        let mut count_rect = RECT {
            left: width - scale_dpi(44, dpi),
            top: 0,
            right: width - pad,
            bottom: height,
        };
        draw_text(
            hdc,
            &format!("{position}/{total}"),
            &mut count_rect,
            rgb(37, 99, 235),
        );
    }

    fn draw_learning(hdc: HDC, width: i32, height: i32, notice: &LearningNotice, dpi: u32) {
        let accent = if notice.kind == LearningNoticeKind::Forgotten {
            rgb(71, 85, 105)
        } else {
            rgb(21, 128, 61)
        };
        let action = match notice.kind {
            LearningNoticeKind::Accepted => "Đã học",
            LearningNoticeKind::Observed => "Đã ghi nhận",
            LearningNoticeKind::Promoted => "Gợi ý cá nhân",
            LearningNoticeKind::Forgotten => "Đã quên",
            LearningNoticeKind::Corrected => "Đã sửa",
        };
        let left = scale_dpi(14, dpi);
        let title_bottom = scale_dpi(31, dpi).min(height);
        let action_width = match notice.kind {
            LearningNoticeKind::Observed => 104,
            LearningNoticeKind::Promoted => 116,
            LearningNoticeKind::Corrected => 72,
            _ => 76,
        };
        let action_right = scale_dpi(action_width, dpi);
        let mut action_rect = RECT {
            left,
            top: 0,
            right: action_right,
            bottom: title_bottom,
        };
        draw_text(hdc, action, &mut action_rect, accent);

        let mut pair_rect = RECT {
            left: action_right,
            top: 0,
            right: width - left,
            bottom: title_bottom,
        };
        draw_text(
            hdc,
            &format!("{} → {}", notice.original_nfc, notice.replacement_nfc),
            &mut pair_rect,
            rgb(15, 23, 42),
        );

        let detail = if notice.kind == LearningNoticeKind::Forgotten {
            "Điểm tích lũy đã trở về 0".to_string()
        } else if notice.kind == LearningNoticeKind::Corrected {
            "Backspace để hoàn tác".to_string()
        } else {
            let delta = if notice.positive_delta > 0.0 && notice.negative_delta > 0.0 {
                format!(
                    "+{:.1}/-{:.1} điểm",
                    notice.positive_delta, notice.negative_delta
                )
            } else if notice.positive_delta > 0.0 {
                format!("+{:.1} điểm", notice.positive_delta)
            } else {
                format!("-{:.1} điểm", notice.negative_delta)
            };
            format!(
                "{delta}  ·  Tích lũy +{:.1} / −{:.1}",
                notice.positive_total, notice.negative_total
            )
        };
        let mut detail_rect = RECT {
            left,
            top: scale_dpi(25, dpi),
            right: width - left,
            bottom: height,
        };
        draw_text(hdc, &detail, &mut detail_rect, accent);
    }

    fn draw_text(hdc: HDC, text: &str, rect: &mut RECT, color: COLORREF) {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let _ = SetTextColor(hdc, color);
            let _ = DrawTextW(
                hdc,
                &mut wide,
                &raw mut *rect,
                windows::Win32::Graphics::Gdi::DT_END_ELLIPSIS
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE
                    | windows::Win32::Graphics::Gdi::DT_VCENTER,
            );
        }
    }
}

#[cfg(windows)]
pub use hwnd_overlay::OverlayWindow;
