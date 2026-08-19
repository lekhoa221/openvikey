//! Native Win32 Settings & Control Window (Slice UI-1 & UI-2).

use std::sync::Mutex;
use std::time::SystemTime;

use openvikey_core::model::ModelInspectionRow;
use openvikey_core::types::{CandidateSource, InputMethod, TonePlacement};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_BTNFACE, CreateFontW, DEFAULT_CHARSET,
    DEFAULT_PITCH, DeleteObject, FF_DONTCARE, FW_NORMAL, HBRUSH, HFONT, OUT_DEFAULT_PRECIS,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON, BS_DEFPUSHBUTTON, BS_GROUPBOX,
    BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBN_SELCHANGE, CBS_DROPDOWNLIST,
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, EN_CHANGE, ES_AUTOHSCROLL,
    GetSystemMetrics, GetWindowTextLengthW, GetWindowTextW, HCURSOR, HICON, HMENU, IDYES, IsWindow,
    LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, LBN_SELCHANGE, LBS_HASSTRINGS,
    LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO, MessageBoxW,
    RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetForegroundWindow,
    SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
    WM_DESTROY, WM_DPICHANGED, WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_CLIPCHILDREN, WS_GROUP, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::host::ControlSnapshot;
use crate::policy::Mode;
use crate::settings::{
    SETTINGS_VERSION, StartupMode, default_settings_path, load_settings, save_settings,
};

const BASE_WINDOW_WIDTH: i32 = 760;
const BASE_WINDOW_HEIGHT: i32 = 560;

const BST_UNCHECKED: usize = 0;
const BST_CHECKED: usize = 1;

const IDC_SIDEBAR: usize = 100;

const IDC_RADIO_VIET: usize = 201;
const IDC_RADIO_ENG: usize = 202;
const IDC_RADIO_VNI: usize = 203;
const IDC_RADIO_TELEX: usize = 204;
const IDC_COMBO_TONE: usize = 205;
const IDC_COMBO_STARTUP_MODE: usize = 206;
const IDC_CHK_SUGGESTIONS: usize = 207;
const IDC_CHK_AUTOSTART: usize = 208;
const IDC_CHK_TERMINAL: usize = 209;

const IDC_TXT_SEARCH_RULES: usize = 301;
const IDC_LIST_RULES: usize = 302;
const IDC_BTN_FORGET_RULE: usize = 303;
const IDC_BTN_FORGET_LAST: usize = 304;
const IDC_BTN_REFRESH_RULES: usize = 305;
const IDC_BTN_CLEAR_ALL: usize = 306;

const IDC_BTN_CLOSE: usize = 801;
const IDC_BTN_APPLY: usize = 802;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct SendHwnd(HWND);
unsafe impl Send for SendHwnd {}
unsafe impl Sync for SendHwnd {}

static SETTINGS_HWND: Mutex<Option<SendHwnd>> = Mutex::new(None);
static CONTROLS: Mutex<Option<UiControls>> = Mutex::new(None);

#[inline]
unsafe fn send_msg(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }
}

#[inline]
const fn ws(base: WINDOW_STYLE, extra: u32) -> WINDOW_STYLE {
    WINDOW_STYLE(base.0 | extra)
}

#[inline]
fn scale_dpi(val: i32, dpi: u32) -> i32 {
    (val * dpi.cast_signed() + 48) / 96
}

fn create_dpi_font(dpi: u32) -> HFONT {
    let height = -scale_dpi(14, dpi);
    unsafe {
        CreateFontW(
            height,
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

struct ControlLayout {
    hwnd: HWND,
    base_x: i32,
    base_y: i32,
    base_w: i32,
    base_h: i32,
}

#[allow(dead_code)]
struct UiControls {
    sidebar: HWND,
    radio_viet: HWND,
    radio_eng: HWND,
    radio_vni: HWND,
    radio_telex: HWND,
    combo_tone: HWND,
    combo_startup_mode: HWND,
    chk_suggestions: HWND,
    chk_autostart: HWND,
    chk_terminal: HWND,
    txt_search_rules: HWND,
    list_rules: HWND,
    lbl_detail_original: HWND,
    lbl_detail_candidate: HWND,
    lbl_detail_method: HWND,
    lbl_detail_source: HWND,
    lbl_detail_evidence: HWND,
    lbl_detail_state: HWND,
    lbl_detail_context: HWND,
    lbl_detail_rule_id: HWND,
    btn_forget_selected: HWND,
    btn_forget_last: HWND,
    btn_refresh_rules: HWND,
    btn_clear_all: HWND,
    filtered_rows: Vec<ModelInspectionRow>,
    all_rows: Vec<ModelInspectionRow>,
    status_label: HWND,
    current_font: HFONT,
    page_controls: [Vec<HWND>; 6],
    layouts: Vec<ControlLayout>,
}

unsafe impl Send for UiControls {}
unsafe impl Sync for UiControls {}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

pub fn init_dpi_awareness() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[must_use]
pub fn active_settings_window_handle() -> HWND {
    SETTINGS_HWND
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .map_or_else(HWND::default, |h| h.0)
}

#[must_use]
pub fn format_control_snapshot(snapshot: &ControlSnapshot) -> String {
    let mode = match snapshot.mode {
        Mode::Viet => "Tiếng Việt (V)",
        Mode::English => "Tiếng Anh (E)",
    };
    let method = match snapshot.engine_config.method {
        InputMethod::Telex => "Telex",
        InputMethod::Vni => "VNI",
    };
    let suggestions = if snapshot.show_suggestions {
        "Bật"
    } else {
        "Tắt"
    };
    let learning = if snapshot.learning_allowed {
        "Bật"
    } else {
        "Tắt"
    };
    let foreground = if snapshot.foreground_exe.is_empty() {
        "(chưa xác định)"
    } else {
        &snapshot.foreground_exe
    };
    let mut text = format!(
        "OpenViKey standalone preview\r\n\r\nChế độ: {mode}\r\nKiểu gõ: {method}\r\nGợi ý: {suggestions}\r\nỨng dụng hiện tại: {foreground}\r\nLearning tại đây: {learning}\r\nRule đã ghi nhận: {}\r\n\r\n",
        snapshot.learned_rows.len()
    );
    if snapshot.learned_rows.is_empty() {
        text.push_str("Chưa có rule cá nhân. Gõ một correction tự nhiên hoặc nhấn Ctrl+. để học.");
    } else {
        text.push_str("Các rule gần nhất:\r\n");
        for row in snapshot.learned_rows.iter().rev().take(20) {
            let line = format!(
                "{} → {} · {:?} · {:?} · +{:.1}/-{:.1}\r\n",
                row.original_nfc,
                row.candidate_nfc,
                row.source,
                row.state,
                row.positive_evidence,
                row.negative_evidence
            );
            text.push_str(&line);
        }
    }
    text.push_str(
        "\r\n\r\nHotkey: Ctrl+. nhận · Ctrl+, từ chối · Ctrl+Shift+Z hoàn tác · Ctrl+Shift+. quên rule cuối.",
    );
    text
}

/// Present startup failure without requiring a console window.
pub fn show_startup_error(message: &str) {
    #[cfg(windows)]
    {
        let text = HSTRING::from(format!("OpenViKey không thể khởi động.\r\n\r\n{message}"));
        unsafe {
            let _ = MessageBoxW(
                None,
                PCWSTR(text.as_ptr()),
                w!("OpenViKey — Lỗi khởi động"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    eprintln!("OpenViKey startup failed: {message}");
}

pub fn show_control_window(_owner: HWND) {
    show_settings_window(Some(0));
}

pub fn show_settings_window(page_index: Option<usize>) {
    #[cfg(windows)]
    {
        let existing = SETTINGS_HWND
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .map(|h| h.0);
        let hwnd = if let Some(h) = existing
            && unsafe { IsWindow(Some(h)) }.as_bool()
        {
            h
        } else {
            let Ok(h) = create_settings_window() else {
                return;
            };
            if let Ok(mut guard) = SETTINGS_HWND.lock() {
                *guard = Some(SendHwnd(h));
            }
            h
        };

        if let Some(index) = page_index {
            select_page(hwnd, index);
        }

        populate_controls(hwnd);

        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = page_index;
    }
}

fn register_window_class() -> Result<()> {
    let class_name = w!("OpenViKeySettingsWindowClass");
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(settings_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: HINSTANCE::default(),
        hIcon: HICON::default(),
        hCursor: HCURSOR::default(),
        hbrBackground: HBRUSH(COLOR_BTNFACE.0 as *mut core::ffi::c_void),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: class_name,
    };
    let atom = unsafe { RegisterClassW(&raw const wc) };
    if atom == 0 {
        let err = windows::core::Error::from_thread();
        if (err.code().0 & 0xFFFF) == 1410 {
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

fn create_settings_window() -> Result<HWND> {
    register_window_class()?;

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let x = (screen_w - BASE_WINDOW_WIDTH) / 2;
    let y = (screen_h - BASE_WINDOW_HEIGHT) / 2;

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("OpenViKeySettingsWindowClass"),
            w!("OpenViKey — Cài đặt"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
            x,
            y,
            BASE_WINDOW_WIDTH,
            BASE_WINDOW_HEIGHT,
            None,
            None,
            Some(HINSTANCE::default()),
            None,
        )?
    };

    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let dpi = if dpi == 0 { 96 } else { dpi };
    if dpi != 96 {
        let scaled_w = scale_dpi(BASE_WINDOW_WIDTH, dpi);
        let scaled_h = scale_dpi(BASE_WINDOW_HEIGHT, dpi);
        let scaled_x = (screen_w - scaled_w) / 2;
        let scaled_y = (screen_h - scaled_h) / 2;
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                scaled_x,
                scaled_y,
                scaled_w,
                scaled_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    build_child_controls(hwnd, dpi)?;
    Ok(hwnd)
}

fn create_label(hwnd: HWND, text: &str, x: i32, y: i32, w: i32, h: i32) -> Result<HWND> {
    let wide = HSTRING::from(text);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            PCWSTR(wide.as_ptr()),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            Some(hwnd),
            None,
            Some(HINSTANCE::default()),
            None,
        )
    }
}

fn get_edit_text(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let len_usize = usize::try_from(len).unwrap_or(0);
    let mut buf = vec![0u16; len_usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if copied > 0 {
        let copied_usize = usize::try_from(copied).unwrap_or(0);
        String::from_utf16_lossy(&buf[..copied_usize])
    } else {
        String::new()
    }
}

#[allow(clippy::too_many_lines)]
fn build_child_controls(hwnd: HWND, dpi: u32) -> Result<()> {
    let font = create_dpi_font(dpi);
    let mut layouts = Vec::new();

    let mut add_layout = |ctrl: HWND, x: i32, y: i32, w: i32, h: i32| {
        layouts.push(ControlLayout {
            hwnd: ctrl,
            base_x: x,
            base_y: y,
            base_w: w,
            base_h: h,
        });
    };

    let sidebar = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("LISTBOX"),
            w!(""),
            ws(
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
                LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32,
            ),
            scale_dpi(16, dpi),
            scale_dpi(16, dpi),
            scale_dpi(170, dpi),
            scale_dpi(460, dpi),
            Some(hwnd),
            Some(HMENU(IDC_SIDEBAR as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(sidebar, 16, 16, 170, 460);

    let items = [
        "  ⚙  Chung",
        "  🧠  Đã học",
        "  📱  Ứng dụng",
        "  ⌨  Phím tắt",
        "  🛡  Riêng tư",
        "  ℹ  Giới thiệu",
    ];
    for item in items {
        let wide = HSTRING::from(item);
        unsafe {
            let _ = send_msg(sidebar, LB_ADDSTRING, 0, wide.as_ptr() as isize);
        }
    }

    let mut page_0 = Vec::new();

    let grp_mode = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!(" Chế độ gõ "),
            ws(WS_CHILD | WS_VISIBLE, BS_GROUPBOX as u32),
            scale_dpi(200, dpi),
            scale_dpi(16, dpi),
            scale_dpi(530, dpi),
            scale_dpi(56, dpi),
            Some(hwnd),
            None,
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(grp_mode, 200, 16, 530, 56);
    page_0.push(grp_mode);

    let radio_viet = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Tiếng Việt (V)"),
            ws(
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_GROUP,
                BS_AUTORADIOBUTTON as u32,
            ),
            scale_dpi(216, dpi),
            scale_dpi(38, dpi),
            scale_dpi(150, dpi),
            scale_dpi(22, dpi),
            Some(hwnd),
            Some(HMENU(IDC_RADIO_VIET as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(radio_viet, 216, 38, 150, 22);
    page_0.push(radio_viet);

    let radio_eng = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Tiếng Anh (E)"),
            ws(WS_CHILD | WS_VISIBLE, BS_AUTORADIOBUTTON as u32),
            scale_dpi(380, dpi),
            scale_dpi(38, dpi),
            scale_dpi(150, dpi),
            scale_dpi(22, dpi),
            Some(hwnd),
            Some(HMENU(IDC_RADIO_ENG as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(radio_eng, 380, 38, 150, 22);
    page_0.push(radio_eng);

    let grp_method = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!(" Kiểu gõ "),
            ws(WS_CHILD | WS_VISIBLE, BS_GROUPBOX as u32),
            scale_dpi(200, dpi),
            scale_dpi(80, dpi),
            scale_dpi(530, dpi),
            scale_dpi(56, dpi),
            Some(hwnd),
            None,
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(grp_method, 200, 80, 530, 56);
    page_0.push(grp_method);

    let radio_vni = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("VNI"),
            ws(
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_GROUP,
                BS_AUTORADIOBUTTON as u32,
            ),
            scale_dpi(216, dpi),
            scale_dpi(102, dpi),
            scale_dpi(150, dpi),
            scale_dpi(22, dpi),
            Some(hwnd),
            Some(HMENU(IDC_RADIO_VNI as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(radio_vni, 216, 102, 150, 22);
    page_0.push(radio_vni);

    let radio_telex = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Telex"),
            ws(WS_CHILD | WS_VISIBLE, BS_AUTORADIOBUTTON as u32),
            scale_dpi(380, dpi),
            scale_dpi(102, dpi),
            scale_dpi(150, dpi),
            scale_dpi(22, dpi),
            Some(hwnd),
            Some(HMENU(IDC_RADIO_TELEX as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(radio_telex, 380, 102, 150, 22);
    page_0.push(radio_telex);

    let lbl_tone = create_label(
        hwnd,
        "Cách đặt dấu:",
        scale_dpi(204, dpi),
        scale_dpi(148, dpi),
        scale_dpi(170, dpi),
        scale_dpi(20, dpi),
    )?;
    add_layout(lbl_tone, 204, 148, 170, 20);
    page_0.push(lbl_tone);

    let combo_tone = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("COMBOBOX"),
            w!(""),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, CBS_DROPDOWNLIST as u32),
            scale_dpi(380, dpi),
            scale_dpi(144, dpi),
            scale_dpi(240, dpi),
            scale_dpi(120, dpi),
            Some(hwnd),
            Some(HMENU(IDC_COMBO_TONE as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(combo_tone, 380, 144, 240, 120);
    for opt in ["Hiện đại (òa, úy)", "Cổ điển (oà, uý)"] {
        let wide = HSTRING::from(opt);
        unsafe {
            let _ = send_msg(combo_tone, CB_ADDSTRING, 0, wide.as_ptr() as isize);
        }
    }
    page_0.push(combo_tone);

    let lbl_startup = create_label(
        hwnd,
        "Khi mở OpenViKey:",
        scale_dpi(204, dpi),
        scale_dpi(182, dpi),
        scale_dpi(170, dpi),
        scale_dpi(20, dpi),
    )?;
    add_layout(lbl_startup, 204, 182, 170, 20);
    page_0.push(lbl_startup);

    let combo_startup_mode = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("COMBOBOX"),
            w!(""),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, CBS_DROPDOWNLIST as u32),
            scale_dpi(380, dpi),
            scale_dpi(178, dpi),
            scale_dpi(240, dpi),
            scale_dpi(120, dpi),
            Some(hwnd),
            Some(HMENU(IDC_COMBO_STARTUP_MODE as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(combo_startup_mode, 380, 178, 240, 120);
    for opt in [
        "Khôi phục trạng thái trước",
        "Luôn bật Tiếng Việt",
        "Luôn bật Tiếng Anh",
    ] {
        let wide = HSTRING::from(opt);
        unsafe {
            let _ = send_msg(combo_startup_mode, CB_ADDSTRING, 0, wide.as_ptr() as isize);
        }
    }
    page_0.push(combo_startup_mode);

    let chk_suggestions = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Hiện khung gợi ý từ (Overlay)"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_AUTOCHECKBOX as u32),
            scale_dpi(204, dpi),
            scale_dpi(216, dpi),
            scale_dpi(400, dpi),
            scale_dpi(22, dpi),
            Some(hwnd),
            Some(HMENU(IDC_CHK_SUGGESTIONS as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(chk_suggestions, 204, 216, 400, 22);
    page_0.push(chk_suggestions);

    let chk_autostart = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Khởi động cùng Windows"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_AUTOCHECKBOX as u32),
            scale_dpi(204, dpi),
            scale_dpi(244, dpi),
            scale_dpi(400, dpi),
            scale_dpi(22, dpi),
            Some(hwnd),
            Some(HMENU(IDC_CHK_AUTOSTART as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(chk_autostart, 204, 244, 400, 22);
    page_0.push(chk_autostart);

    let grp_terminal = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!(" Hỗ trợ Terminal "),
            ws(WS_CHILD | WS_VISIBLE, BS_GROUPBOX as u32),
            scale_dpi(200, dpi),
            scale_dpi(276, dpi),
            scale_dpi(530, dpi),
            scale_dpi(88, dpi),
            Some(hwnd),
            None,
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(grp_terminal, 200, 276, 530, 88);
    page_0.push(grp_terminal);

    let chk_terminal = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Cho phép gõ tiếng Việt trong Terminal (PowerShell, CMD, Windows Terminal)"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_AUTOCHECKBOX as u32),
            scale_dpi(214, dpi),
            scale_dpi(298, dpi),
            scale_dpi(500, dpi),
            scale_dpi(22, dpi),
            Some(hwnd),
            Some(HMENU(IDC_CHK_TERMINAL as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(chk_terminal, 214, 298, 500, 22);
    page_0.push(chk_terminal);

    let lbl_term_hint = create_label(
        hwnd,
        "* OpenViKey không học hoặc lưu bất kỳ nội dung nào gõ trong Terminal.",
        scale_dpi(214, dpi),
        scale_dpi(326, dpi),
        scale_dpi(500, dpi),
        scale_dpi(22, dpi),
    )?;
    add_layout(lbl_term_hint, 214, 326, 500, 22);
    page_0.push(lbl_term_hint);

    let mut page_1 = Vec::new();

    let lbl_search = create_label(
        hwnd,
        "Tìm kiếm rule:",
        scale_dpi(204, dpi),
        scale_dpi(16, dpi),
        scale_dpi(95, dpi),
        scale_dpi(20, dpi),
    )?;
    add_layout(lbl_search, 204, 16, 95, 20);
    page_1.push(lbl_search);

    let txt_search_rules = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            ws(
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
                ES_AUTOHSCROLL as u32,
            ),
            scale_dpi(304, dpi),
            scale_dpi(14, dpi),
            scale_dpi(426, dpi),
            scale_dpi(24, dpi),
            Some(hwnd),
            Some(HMENU(IDC_TXT_SEARCH_RULES as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(txt_search_rules, 304, 14, 426, 24);
    page_1.push(txt_search_rules);

    let list_rules = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("LISTBOX"),
            w!(""),
            ws(
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WS_VSCROLL,
                LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32 | LBS_HASSTRINGS as u32,
            ),
            scale_dpi(204, dpi),
            scale_dpi(44, dpi),
            scale_dpi(526, dpi),
            scale_dpi(236, dpi),
            Some(hwnd),
            Some(HMENU(IDC_LIST_RULES as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(list_rules, 204, 44, 526, 236);
    page_1.push(list_rules);

    let grp_details = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!(" Chi tiết rule "),
            ws(WS_CHILD | WS_VISIBLE, BS_GROUPBOX as u32),
            scale_dpi(204, dpi),
            scale_dpi(286, dpi),
            scale_dpi(526, dpi),
            scale_dpi(148, dpi),
            Some(hwnd),
            None,
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(grp_details, 204, 286, 526, 148);
    page_1.push(grp_details);

    let lbl_detail_original = create_label(
        hwnd,
        "• Từ gốc:        -",
        scale_dpi(216, dpi),
        scale_dpi(306, dpi),
        scale_dpi(240, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_original, 216, 306, 240, 18);
    page_1.push(lbl_detail_original);

    let lbl_detail_candidate = create_label(
        hwnd,
        "• Từ thay thế:   -",
        scale_dpi(470, dpi),
        scale_dpi(306, dpi),
        scale_dpi(240, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_candidate, 470, 306, 240, 18);
    page_1.push(lbl_detail_candidate);

    let lbl_detail_method = create_label(
        hwnd,
        "• Kiểu gõ:       -",
        scale_dpi(216, dpi),
        scale_dpi(328, dpi),
        scale_dpi(240, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_method, 216, 328, 240, 18);
    page_1.push(lbl_detail_method);

    let lbl_detail_source = create_label(
        hwnd,
        "• Nguồn gốc:     -",
        scale_dpi(470, dpi),
        scale_dpi(328, dpi),
        scale_dpi(240, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_source, 470, 328, 240, 18);
    page_1.push(lbl_detail_source);

    let lbl_detail_evidence = create_label(
        hwnd,
        "• Bằng chứng:    -",
        scale_dpi(216, dpi),
        scale_dpi(350, dpi),
        scale_dpi(240, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_evidence, 216, 350, 240, 18);
    page_1.push(lbl_detail_evidence);

    let lbl_detail_state = create_label(
        hwnd,
        "• Trạng thái:    -",
        scale_dpi(470, dpi),
        scale_dpi(350, dpi),
        scale_dpi(240, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_state, 470, 350, 240, 18);
    page_1.push(lbl_detail_state);

    let lbl_detail_context = create_label(
        hwnd,
        "• Ngữ cảnh trước: -",
        scale_dpi(216, dpi),
        scale_dpi(372, dpi),
        scale_dpi(500, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_context, 216, 372, 500, 18);
    page_1.push(lbl_detail_context);

    let lbl_detail_rule_id = create_label(
        hwnd,
        "• Rule ID:       -",
        scale_dpi(216, dpi),
        scale_dpi(394, dpi),
        scale_dpi(500, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_rule_id, 216, 394, 500, 18);
    page_1.push(lbl_detail_rule_id);

    let btn_forget_selected = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Quên rule đã chọn"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
            scale_dpi(204, dpi),
            scale_dpi(442, dpi),
            scale_dpi(140, dpi),
            scale_dpi(30, dpi),
            Some(hwnd),
            Some(HMENU(IDC_BTN_FORGET_RULE as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(btn_forget_selected, 204, 442, 140, 30);
    page_1.push(btn_forget_selected);

    let btn_forget_last = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Quên rule vừa học"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
            scale_dpi(352, dpi),
            scale_dpi(442, dpi),
            scale_dpi(140, dpi),
            scale_dpi(30, dpi),
            Some(hwnd),
            Some(HMENU(IDC_BTN_FORGET_LAST as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(btn_forget_last, 352, 442, 140, 30);
    page_1.push(btn_forget_last);

    let btn_refresh_rules = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Làm mới"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
            scale_dpi(500, dpi),
            scale_dpi(442, dpi),
            scale_dpi(105, dpi),
            scale_dpi(30, dpi),
            Some(hwnd),
            Some(HMENU(IDC_BTN_REFRESH_RULES as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(btn_refresh_rules, 500, 442, 105, 30);
    page_1.push(btn_refresh_rules);

    let btn_clear_all = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Xóa tất cả"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
            scale_dpi(613, dpi),
            scale_dpi(442, dpi),
            scale_dpi(117, dpi),
            scale_dpi(30, dpi),
            Some(hwnd),
            Some(HMENU(IDC_BTN_CLEAR_ALL as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(btn_clear_all, 613, 442, 117, 30);
    page_1.push(btn_clear_all);

    let mut make_placeholder = |title: &str, subtitle: &str| -> Result<Vec<HWND>> {
        let mut list = Vec::new();
        let h1 = create_label(
            hwnd,
            title,
            scale_dpi(204, dpi),
            scale_dpi(24, dpi),
            scale_dpi(500, dpi),
            scale_dpi(28, dpi),
        )?;
        add_layout(h1, 204, 24, 500, 28);
        let h2 = create_label(
            hwnd,
            subtitle,
            scale_dpi(204, dpi),
            scale_dpi(56, dpi),
            scale_dpi(500, dpi),
            scale_dpi(40, dpi),
        )?;
        add_layout(h2, 204, 56, 500, 40);
        list.push(h1);
        list.push(h2);
        Ok(list)
    };

    let page_2 = make_placeholder(
        "📱  Ứng dụng (Applications Policy)",
        "Cấu hình chính sách chuyển đổi và học máy cho từng ứng dụng (Sẽ hoàn thiện ở Slice UI-3).",
    )?;
    let page_3 = make_placeholder(
        "⌨  Phím tắt (Hotkeys)",
        "Tùy chỉnh các tổ hợp phím điều khiển bộ gõ (Sẽ hoàn thiện ở Slice UI-3).",
    )?;
    let page_4 = make_placeholder(
        "🛡  Riêng tư (Privacy)",
        "Cam kết Zero-Backend và quản lý thư mục dữ liệu cục bộ (Sẽ hoàn thiện ở Slice UI-4).",
    )?;
    let page_5 = make_placeholder(
        "ℹ  Giới thiệu & Chẩn đoán (About)",
        "Thông tin phiên bản, bản quyền và trạng thái runtime (Sẽ hoàn thiện ở Slice UI-4).",
    )?;

    let status_label = create_label(
        hwnd,
        "● OpenViKey đang chạy · Local-only · v0.1.0",
        scale_dpi(16, dpi),
        scale_dpi(492, dpi),
        scale_dpi(470, dpi),
        scale_dpi(24, dpi),
    )?;
    add_layout(status_label, 16, 492, 470, 24);

    let btn_close = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Đóng"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
            scale_dpi(510, dpi),
            scale_dpi(486, dpi),
            scale_dpi(100, dpi),
            scale_dpi(30, dpi),
            Some(hwnd),
            Some(HMENU(IDC_BTN_CLOSE as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(btn_close, 510, 486, 100, 30);

    let btn_apply = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Áp dụng"),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_DEFPUSHBUTTON as u32),
            scale_dpi(625, dpi),
            scale_dpi(486, dpi),
            scale_dpi(105, dpi),
            scale_dpi(30, dpi),
            Some(hwnd),
            Some(HMENU(IDC_BTN_APPLY as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(btn_apply, 625, 486, 105, 30);

    for item in &layouts {
        unsafe {
            let _ = send_msg(item.hwnd, WM_SETFONT, font.0 as usize, 1);
        }
    }

    let controls = UiControls {
        sidebar,
        radio_viet,
        radio_eng,
        radio_vni,
        radio_telex,
        combo_tone,
        combo_startup_mode,
        chk_suggestions,
        chk_autostart,
        chk_terminal,
        txt_search_rules,
        list_rules,
        lbl_detail_original,
        lbl_detail_candidate,
        lbl_detail_method,
        lbl_detail_source,
        lbl_detail_evidence,
        lbl_detail_state,
        lbl_detail_context,
        lbl_detail_rule_id,
        btn_forget_selected,
        btn_forget_last,
        btn_refresh_rules,
        btn_clear_all,
        filtered_rows: Vec::new(),
        all_rows: Vec::new(),
        status_label,
        current_font: font,
        page_controls: [page_0, page_1, page_2, page_3, page_4, page_5],
        layouts,
    };

    if let Ok(mut guard) = CONTROLS.lock() {
        *guard = Some(controls);
    }

    Ok(())
}

fn apply_dpi_layout(hwnd: HWND, dpi: u32) {
    let Ok(mut guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    let new_font = create_dpi_font(dpi);
    let old_font = controls.current_font;
    controls.current_font = new_font;

    for item in &controls.layouts {
        unsafe {
            let _ = SetWindowPos(
                item.hwnd,
                None,
                scale_dpi(item.base_x, dpi),
                scale_dpi(item.base_y, dpi),
                scale_dpi(item.base_w, dpi),
                scale_dpi(item.base_h, dpi),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = send_msg(item.hwnd, WM_SETFONT, new_font.0 as usize, 1);
        }
    }

    if !old_font.is_invalid() {
        unsafe {
            let _ = DeleteObject(old_font.into());
        }
    }

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

fn select_page(hwnd: HWND, page_index: usize) {
    let Ok(mut guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    let target = page_index.min(5);
    unsafe {
        let _ = send_msg(controls.sidebar, LB_SETCURSEL, target, 0);
    }

    for (index, page_list) in controls.page_controls.iter().enumerate() {
        let cmd = if index == target { SW_SHOW } else { SW_HIDE };
        for &ctrl in page_list {
            unsafe {
                let _ = ShowWindow(ctrl, cmd);
            }
        }
    }

    if target == 1 {
        populate_rules_inner(controls);
    }

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

fn format_rule_row(row: &ModelInspectionRow) -> String {
    let method = match row.input_method {
        InputMethod::Telex => "Telex",
        InputMethod::Vni => "VNI",
    };
    let source = match row.source {
        CandidateSource::Personal => "Cá nhân (Rewind)",
        CandidateSource::Abbreviation => "Viết tắt (Abbrev)",
        CandidateSource::TelexFix => "Sửa Telex (TelexFix)",
        CandidateSource::Fuzzy => "Gõ sai âm (Fuzzy)",
        CandidateSource::Diacritics => "Dấu thanh (Diacritics)",
    };
    format!(
        "  {:<12} → {:<12} │ {:<8} │ +{:<4.1} / -{:<3.1} │ [{method}] {source}",
        row.original_nfc,
        row.candidate_nfc,
        format!("{:?}", row.state),
        row.positive_evidence,
        row.negative_evidence
    )
}

#[allow(clippy::too_many_lines)]
fn update_rule_details_view(controls: &UiControls, row: Option<&ModelInspectionRow>) {
    if let Some(r) = row {
        let method = match r.input_method {
            InputMethod::Telex => "Telex",
            InputMethod::Vni => "VNI",
        };
        let source = match r.source {
            CandidateSource::Personal => "Sửa từ sau đó (Rewind/Personal)",
            CandidateSource::Abbreviation => "Viết tắt / Ghép tự nhiên (Abbreviation)",
            CandidateSource::TelexFix => "Sửa lỗi gõ Telex (TelexFix)",
            CandidateSource::Fuzzy => "Sửa sai âm tương đồng (Fuzzy)",
            CandidateSource::Diacritics => "Bổ sung dấu thanh (Diacritics)",
        };
        let state_desc = match r.state {
            openvikey_core::decision::DecisionState::Auto => {
                "Auto (Tự động chuyển đổi tại ranh giới từ)"
            }
            openvikey_core::decision::DecisionState::Suggest => {
                "Suggest (Được đề xuất trên thanh gợi ý)"
            }
            openvikey_core::decision::DecisionState::Ignore => {
                "Ignore (Đang quan sát / Chưa đủ tin cậy)"
            }
        };
        let orig = format!("• Từ gốc:        {}", r.original_nfc);
        let cand = format!("• Từ thay thế:   {}", r.candidate_nfc);
        let meth = format!("• Kiểu gõ:       {method}");
        let src = format!("• Nguồn gốc:     {source}");
        let evid = format!(
            "• Bằng chứng:    +{:.1} / -{:.1} ({} sự kiện)",
            r.positive_evidence, r.negative_evidence, r.evidence_count
        );
        let st = format!("• Trạng thái:    {state_desc}");
        let ctx = format!(
            "• Ngữ cảnh trước: {}",
            r.left_token_nfc.as_deref().unwrap_or("(không có)")
        );
        let rid = format!("• Rule ID:       {}", r.source_rule_id);

        unsafe {
            let _ = SetWindowTextW(
                controls.lbl_detail_original,
                PCWSTR(HSTRING::from(orig).as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_candidate,
                PCWSTR(HSTRING::from(cand).as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_method,
                PCWSTR(HSTRING::from(meth).as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_source,
                PCWSTR(HSTRING::from(src).as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_evidence,
                PCWSTR(HSTRING::from(evid).as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_state,
                PCWSTR(HSTRING::from(st).as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_context,
                PCWSTR(HSTRING::from(ctx).as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_rule_id,
                PCWSTR(HSTRING::from(rid).as_ptr()),
            );
        }
    } else {
        unsafe {
            let _ = SetWindowTextW(
                controls.lbl_detail_original,
                PCWSTR(HSTRING::from("• Từ gốc:        -").as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_candidate,
                PCWSTR(HSTRING::from("• Từ thay thế:   -").as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_method,
                PCWSTR(HSTRING::from("• Kiểu gõ:       -").as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_source,
                PCWSTR(HSTRING::from("• Nguồn gốc:     -").as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_evidence,
                PCWSTR(HSTRING::from("• Bằng chứng:    -").as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_state,
                PCWSTR(HSTRING::from("• Trạng thái:    -").as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_context,
                PCWSTR(HSTRING::from("• Ngữ cảnh trước: -").as_ptr()),
            );
            let _ = SetWindowTextW(
                controls.lbl_detail_rule_id,
                PCWSTR(HSTRING::from("• Rule ID:       -").as_ptr()),
            );
        }
    }
}

fn apply_rules_filter_inner(controls: &mut UiControls) {
    let query = get_edit_text(controls.txt_search_rules).to_lowercase();
    let query = query.trim();

    controls.filtered_rows = if query.is_empty() {
        controls.all_rows.clone()
    } else {
        controls
            .all_rows
            .iter()
            .filter(|r| {
                r.original_nfc.to_lowercase().contains(query)
                    || r.candidate_nfc.to_lowercase().contains(query)
                    || r.source_rule_id.to_lowercase().contains(query)
            })
            .cloned()
            .collect()
    };

    unsafe {
        let _ = send_msg(controls.list_rules, LB_RESETCONTENT, 0, 0);
    }

    for row in &controls.filtered_rows {
        let text = format_rule_row(row);
        let wide = HSTRING::from(text);
        unsafe {
            let _ = send_msg(controls.list_rules, LB_ADDSTRING, 0, wide.as_ptr() as isize);
        }
    }

    if controls.filtered_rows.is_empty() {
        update_rule_details_view(controls, None);
    } else {
        unsafe {
            let _ = send_msg(controls.list_rules, LB_SETCURSEL, 0, 0);
        }
        update_rule_details_view(controls, controls.filtered_rows.first());
    }
}

fn populate_rules_inner(controls: &mut UiControls) {
    let snapshot = crate::host::control_snapshot();
    let mut rows = snapshot.map_or_else(Vec::new, |s| s.learned_rows);
    rows.reverse();
    controls.all_rows = rows;
    apply_rules_filter_inner(controls);
}

#[allow(clippy::too_many_lines)]
fn populate_controls(_hwnd: HWND) {
    let Ok(mut guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let settings_path = default_settings_path(local_app_data.as_ref());
    let settings = load_settings(&settings_path).unwrap_or_default();
    let snapshot = crate::host::control_snapshot();

    let current_mode = snapshot.as_ref().map_or(Mode::Viet, |s| s.mode);
    let current_method = snapshot
        .as_ref()
        .map_or(settings.input_method, |s| s.engine_config.method);
    let current_tone = snapshot
        .as_ref()
        .map_or(settings.tone_placement, |s| s.engine_config.tone_placement);
    let current_suggestions = snapshot
        .as_ref()
        .map_or(settings.show_suggestions, |s| s.show_suggestions);
    let current_terminal = snapshot
        .as_ref()
        .map_or(settings.allow_terminal, |s| s.allow_terminal);
    let current_autostart = crate::startup::enabled();

    unsafe {
        let _ = send_msg(
            controls.radio_viet,
            BM_SETCHECK,
            if current_mode == Mode::Viet {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            },
            0,
        );
        let _ = send_msg(
            controls.radio_eng,
            BM_SETCHECK,
            if current_mode == Mode::English {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            },
            0,
        );

        let _ = send_msg(
            controls.radio_vni,
            BM_SETCHECK,
            if current_method == InputMethod::Vni {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            },
            0,
        );
        let _ = send_msg(
            controls.radio_telex,
            BM_SETCHECK,
            if current_method == InputMethod::Telex {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            },
            0,
        );

        let tone_index = match current_tone {
            TonePlacement::Modern => 0,
            TonePlacement::Classic => 1,
        };
        let _ = send_msg(controls.combo_tone, CB_SETCURSEL, tone_index, 0);

        let startup_index = match settings.mode_on_start {
            StartupMode::RestoreLast => 0,
            StartupMode::Viet => 1,
            StartupMode::English => 2,
        };
        let _ = send_msg(controls.combo_startup_mode, CB_SETCURSEL, startup_index, 0);

        let _ = send_msg(
            controls.chk_suggestions,
            BM_SETCHECK,
            if current_suggestions {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            },
            0,
        );

        let _ = send_msg(
            controls.chk_autostart,
            BM_SETCHECK,
            if current_autostart {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            },
            0,
        );

        let _ = send_msg(
            controls.chk_terminal,
            BM_SETCHECK,
            if current_terminal {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            },
            0,
        );

        let status_text = HSTRING::from("● OpenViKey đang chạy · Local-only · v0.1.0");
        let _ = SetWindowTextW(controls.status_label, PCWSTR(status_text.as_ptr()));
    }

    populate_rules_inner(controls);
}

#[allow(clippy::too_many_lines)]
fn apply_settings_from_ui(_hwnd: HWND) {
    let Ok(guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_ref() else {
        return;
    };

    let is_viet = unsafe {
        send_msg(controls.radio_viet, BM_GETCHECK, 0, 0)
            .0
            .cast_unsigned()
    } == BST_CHECKED;
    let mode = if is_viet { Mode::Viet } else { Mode::English };

    let is_vni = unsafe {
        send_msg(controls.radio_vni, BM_GETCHECK, 0, 0)
            .0
            .cast_unsigned()
    } == BST_CHECKED;
    let method = if is_vni {
        InputMethod::Vni
    } else {
        InputMethod::Telex
    };

    let tone_idx = unsafe {
        send_msg(controls.combo_tone, CB_GETCURSEL, 0, 0)
            .0
            .cast_unsigned()
    };
    let tone_placement = if tone_idx == 1 {
        TonePlacement::Classic
    } else {
        TonePlacement::Modern
    };

    let startup_idx = unsafe {
        send_msg(controls.combo_startup_mode, CB_GETCURSEL, 0, 0)
            .0
            .cast_unsigned()
    };
    let mode_on_start = match startup_idx {
        1 => StartupMode::Viet,
        2 => StartupMode::English,
        _ => StartupMode::RestoreLast,
    };

    let show_suggestions = unsafe {
        send_msg(controls.chk_suggestions, BM_GETCHECK, 0, 0)
            .0
            .cast_unsigned()
    } == BST_CHECKED;
    let start_with_windows = unsafe {
        send_msg(controls.chk_autostart, BM_GETCHECK, 0, 0)
            .0
            .cast_unsigned()
    } == BST_CHECKED;
    let allow_terminal = unsafe {
        send_msg(controls.chk_terminal, BM_GETCHECK, 0, 0)
            .0
            .cast_unsigned()
    } == BST_CHECKED;

    let at_ms = now_ms();
    crate::host::set_mode_runtime(mode, at_ms);
    crate::host::set_input_method_runtime(method, at_ms);
    crate::host::set_tone_placement_runtime(tone_placement, at_ms);
    crate::host::set_show_suggestions_runtime(show_suggestions);
    crate::host::set_allow_terminal_runtime(allow_terminal);
    let auto_res = crate::startup::set_enabled(start_with_windows);

    let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let settings_path = default_settings_path(local_app_data.as_ref());
    let mut settings = load_settings(&settings_path).unwrap_or_default();
    settings.version = SETTINGS_VERSION;
    settings.input_method = method;
    settings.tone_placement = tone_placement;
    settings.mode_on_start = mode_on_start;
    settings.last_mode_viet = mode == Mode::Viet;
    settings.show_suggestions = show_suggestions;
    settings.allow_terminal = allow_terminal;
    settings.start_with_windows = start_with_windows;

    let save_res = save_settings(&settings_path, &settings);

    let status_text = match (save_res, auto_res) {
        (Ok(()), Ok(())) => HSTRING::from("✓ Đã lưu và áp dụng cài đặt thành công."),
        (Err(e), _) => HSTRING::from(format!("⚠ Không thể lưu cài đặt: {e}")),
        (_, Err(e)) => HSTRING::from(format!(
            "⚠ Đã lưu cài đặt, nhưng lỗi khởi động cùng Windows: {e}"
        )),
    };
    unsafe {
        let _ = SetWindowTextW(controls.status_label, PCWSTR(status_text.as_ptr()));
    }
}

fn on_forget_selected_rule(hwnd: HWND) {
    let Ok(mut guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    let sel = unsafe {
        send_msg(controls.list_rules, LB_GETCURSEL, 0, 0)
            .0
            .cast_unsigned()
    };
    let Some(row) = controls.filtered_rows.get(sel).cloned() else {
        return;
    };

    let changed =
        crate::host::forget_rule_runtime(row.input_method, &row.original_nfc, &row.candidate_nfc);
    populate_rules_inner(controls);

    let status = if changed {
        format!(
            "✓ Đã quên rule: {} → {}",
            row.original_nfc, row.candidate_nfc
        )
    } else {
        "⚠ Không tìm thấy rule cần quên.".to_string()
    };
    unsafe {
        let _ = SetWindowTextW(
            controls.status_label,
            PCWSTR(HSTRING::from(status).as_ptr()),
        );
    }
    let _ = hwnd;
}

fn on_forget_last_rule(hwnd: HWND) {
    let at_ms = now_ms();
    crate::host::forget_last_rule_runtime(at_ms);

    let Ok(mut guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    populate_rules_inner(controls);
    unsafe {
        let text = HSTRING::from("✓ Đã thực hiện quên rule vừa học gần nhất.");
        let _ = SetWindowTextW(controls.status_label, PCWSTR(text.as_ptr()));
    }
    let _ = hwnd;
}

fn on_refresh_rules(hwnd: HWND) {
    let Ok(mut guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    populate_rules_inner(controls);
    unsafe {
        let text = HSTRING::from("✓ Đã làm mới danh sách các rule đã học.");
        let _ = SetWindowTextW(controls.status_label, PCWSTR(text.as_ptr()));
    }
    let _ = hwnd;
}

fn on_clear_all_rules(hwnd: HWND) {
    let confirm = unsafe {
        MessageBoxW(
            Some(hwnd),
            w!("Bạn có chắc chắn muốn xóa toàn bộ các rule và từ viết tắt đã học không?"),
            w!("OpenViKey — Xác nhận xóa dữ liệu học"),
            MB_YESNO | MB_ICONWARNING,
        )
    };
    if confirm != IDYES {
        return;
    }

    crate::host::clear_all_rules_runtime();

    let Ok(mut guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    populate_rules_inner(controls);
    unsafe {
        let text = HSTRING::from("✓ Đã xóa toàn bộ dữ liệu rule đã học.");
        let _ = SetWindowTextW(controls.status_label, PCWSTR(text.as_ptr()));
    }
}

#[allow(clippy::too_many_lines)]
unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DPICHANGED => {
            let new_dpi = u32::try_from(wparam.0 & 0xFFFF).unwrap_or(96);
            let suggested = unsafe { &*(lparam.0 as *const windows::Win32::Foundation::RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            apply_dpi_layout(hwnd, new_dpi);
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            let code = (wparam.0 >> 16) & 0xFFFF;

            if id == IDC_BTN_CLOSE {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_APPLY {
                apply_settings_from_ui(hwnd);
                return LRESULT(0);
            }
            if id == IDC_SIDEBAR && code == LBN_SELCHANGE as usize {
                let Ok(guard) = CONTROLS.lock() else {
                    return LRESULT(0);
                };
                if let Some(controls) = guard.as_ref() {
                    let sel = unsafe {
                        send_msg(controls.sidebar, LB_GETCURSEL, 0, 0)
                            .0
                            .cast_unsigned()
                    };
                    drop(guard);
                    select_page(hwnd, sel);
                }
                return LRESULT(0);
            }
            if id == IDC_RADIO_VIET || id == IDC_RADIO_ENG {
                apply_settings_from_ui(hwnd);
                return LRESULT(0);
            }
            if id == IDC_RADIO_VNI || id == IDC_RADIO_TELEX {
                apply_settings_from_ui(hwnd);
                return LRESULT(0);
            }
            if (id == IDC_COMBO_TONE || id == IDC_COMBO_STARTUP_MODE)
                && code == CBN_SELCHANGE as usize
            {
                apply_settings_from_ui(hwnd);
                return LRESULT(0);
            }
            if id == IDC_CHK_SUGGESTIONS || id == IDC_CHK_AUTOSTART || id == IDC_CHK_TERMINAL {
                apply_settings_from_ui(hwnd);
                return LRESULT(0);
            }
            if id == IDC_TXT_SEARCH_RULES && code == EN_CHANGE as usize {
                let Ok(mut guard) = CONTROLS.lock() else {
                    return LRESULT(0);
                };
                if let Some(controls) = guard.as_mut() {
                    apply_rules_filter_inner(controls);
                }
                return LRESULT(0);
            }
            if id == IDC_LIST_RULES && code == LBN_SELCHANGE as usize {
                let Ok(guard) = CONTROLS.lock() else {
                    return LRESULT(0);
                };
                if let Some(controls) = guard.as_ref() {
                    let sel = unsafe {
                        send_msg(controls.list_rules, LB_GETCURSEL, 0, 0)
                            .0
                            .cast_unsigned()
                    };
                    let row = controls.filtered_rows.get(sel);
                    update_rule_details_view(controls, row);
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_FORGET_RULE {
                on_forget_selected_rule(hwnd);
                return LRESULT(0);
            }
            if id == IDC_BTN_FORGET_LAST {
                on_forget_last_rule(hwnd);
                return LRESULT(0);
            }
            if id == IDC_BTN_REFRESH_RULES {
                on_refresh_rules(hwnd);
                return LRESULT(0);
            }
            if id == IDC_BTN_CLEAR_ALL {
                on_clear_all_rules(hwnd);
                return LRESULT(0);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Ok(mut guard) = SETTINGS_HWND.lock() {
                *guard = None;
            }
            if let Ok(mut guard) = CONTROLS.lock()
                && let Some(controls) = guard.take()
                && !controls.current_font.is_invalid()
            {
                unsafe {
                    let _ = DeleteObject(controls.current_font.into());
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
