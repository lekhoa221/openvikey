//! Native Win32 Settings & Control Window (Slice UI-1 & UI-2).

use std::sync::Mutex;

use openvikey_core::model::ModelInspectionRow;
use openvikey_core::types::{CandidateSource, InputMethod, TonePlacement};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject,
    DrawFocusRect, DrawTextW, FF_DONTCARE, FW_NORMAL, FillRect, HBRUSH, HDC, HFONT,
    OUT_DEFAULT_PRECIS, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
    SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, HIMAGELIST, ICC_LISTVIEW_CLASSES, ILC_COLOR32, ILC_MASK, INITCOMMONCONTROLSEX,
    ImageList_Create, ImageList_Destroy, ImageList_ReplaceIcon, InitCommonControlsEx, LVCF_SUBITEM,
    LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVIS_SELECTED, LVITEMW, LVM_DELETEALLITEMS,
    LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETITEMSTATE, LVM_SETITEMW, LVN_ITEMCHANGED, LVNI_SELECTED, LVS_EX_DOUBLEBUFFER,
    LVS_EX_FULLROWSELECT, LVS_REPORT, LVS_SINGLESEL, NMHDR, ODS_FOCUS, ODS_SELECTED, TCIF_IMAGE,
    TCIF_TEXT, TCITEMW, TCM_GETCURSEL, TCM_INSERTITEMW, TCM_SETCURSEL, TCM_SETIMAGELIST,
    TCN_SELCHANGE, TCS_HOTTRACK, WC_LISTVIEWW, WC_TABCONTROLW,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON, BS_DEFPUSHBUTTON, BS_GROUPBOX,
    BS_OWNERDRAW, BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBN_SELCHANGE,
    CBS_DROPDOWNLIST, CS_HREDRAW, CS_VREDRAW, CreateIcon, CreateWindowExW, DefWindowProcW,
    DestroyIcon, EN_CHANGE, ES_AUTOHSCROLL, GetSystemMetrics, GetWindowTextLengthW, GetWindowTextW,
    HCURSOR, HICON, HMENU, IsWindow, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LBN_SELCHANGE,
    LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, MB_ICONERROR, MB_OK, MessageBoxW, RegisterClassW,
    SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos,
    SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_DRAWITEM, WM_NOTIFY,
    WM_SETFONT, WM_SETREDRAW, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN,
    WS_GROUP, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, Result, w};

use crate::host::ControlSnapshot;
use crate::policy::Mode;
use crate::settings::{
    AppInjectProfile, AppLearningPolicy, AppPolicyV1, AppTransformPolicy, HotkeySettingsV1,
    StartupMode, default_settings_path, load_settings,
};

const BASE_WINDOW_WIDTH: i32 = 760;
const BASE_WINDOW_HEIGHT: i32 = 610;

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

const IDC_LIST_APPS: usize = 401;
const IDC_EDIT_APP_EXE: usize = 402;
const IDC_COMBO_APP_TRANSFORM: usize = 403;
const IDC_COMBO_APP_LEARNING: usize = 404;
const IDC_COMBO_APP_PROFILE: usize = 405;
const IDC_BTN_APP_SAVE: usize = 406;
const IDC_BTN_APP_REMOVE: usize = 407;
const IDC_BTN_APP_CURRENT: usize = 408;

const IDC_EDIT_HOTKEY_TOGGLE: usize = 501;
const IDC_EDIT_HOTKEY_ACCEPT: usize = 502;
const IDC_EDIT_HOTKEY_REJECT: usize = 503;
const IDC_EDIT_HOTKEY_UNDO: usize = 504;
const IDC_EDIT_HOTKEY_FORGET: usize = 505;
const IDC_BTN_HOTKEY_SAVE: usize = 506;
const IDC_BTN_HOTKEY_RESET: usize = 507;
const IDC_BTN_OPEN_DATA: usize = 601;
const IDC_BTN_REFRESH_ABOUT: usize = 701;

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

fn create_nav_icon(rows: &[u16; 16]) -> Result<HICON> {
    let mut and_mask = [0_u8; 32];
    for (index, row) in rows.iter().enumerate() {
        let [high, low] = (!row).to_be_bytes();
        and_mask[index * 2] = high;
        and_mask[index * 2 + 1] = low;
    }
    let xor_mask = [0_u8; 32];
    unsafe { CreateIcon(None, 16, 16, 1, 1, and_mask.as_ptr(), xor_mask.as_ptr()) }
}

struct ControlLayout {
    hwnd: HWND,
    base_x: i32,
    base_y: i32,
    base_w: i32,
    base_h: i32,
}

fn override_layout(
    layouts: &mut [ControlLayout],
    hwnd: HWND,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    if let Some(layout) = layouts.iter_mut().find(|layout| layout.hwnd == hwnd) {
        layout.base_x = x;
        layout.base_y = y;
        layout.base_w = width;
        layout.base_h = height;
    }
}

#[allow(dead_code)]
struct UiControls {
    sidebar: HWND,
    header_mark: HWND,
    header_title: HWND,
    header_subtitle: HWND,
    accent_line: HWND,
    lbl_local: HWND,
    btn_apply: HWND,
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
    lbl_detail_rule_id: HWND,
    btn_forget_selected: HWND,
    btn_forget_last: HWND,
    btn_refresh_rules: HWND,
    list_apps: HWND,
    edit_app_exe: HWND,
    combo_app_transform: HWND,
    combo_app_learning: HWND,
    combo_app_profile: HWND,
    edit_hotkey_toggle: HWND,
    edit_hotkey_accept: HWND,
    edit_hotkey_reject: HWND,
    edit_hotkey_undo: HWND,
    edit_hotkey_forget: HWND,
    lbl_privacy_path: HWND,
    lbl_about_runtime: HWND,
    filtered_rows: Vec<ModelInspectionRow>,
    all_rows: Vec<ModelInspectionRow>,
    status_label: HWND,
    current_font: HFONT,
    nav_images: HIMAGELIST,
    accent_brush: HBRUSH,
    accent_pressed_brush: HBRUSH,
    soft_blue_brush: HBRUSH,
    window_brush: HBRUSH,
    page_controls: [Vec<HWND>; 6],
    persistent_controls: Vec<HWND>,
    active_page: Option<usize>,
    layouts: Vec<ControlLayout>,
}

unsafe impl Send for UiControls {}
unsafe impl Sync for UiControls {}

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
        hbrBackground: HBRUSH(
            usize::try_from(COLOR_WINDOW.0 + 1).unwrap_or(0) as *mut core::ffi::c_void
        ),
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
    let controls_init = INITCOMMONCONTROLSEX {
        dwSize: u32::try_from(std::mem::size_of::<INITCOMMONCONTROLSEX>()).unwrap_or(u32::MAX),
        dwICC: ICC_LISTVIEW_CLASSES,
    };
    unsafe {
        let _ = InitCommonControlsEx(&raw const controls_init);
    }
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

    let header_mark = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("V"),
            ws(WS_CHILD | WS_VISIBLE, 0x0201),
            scale_dpi(18, dpi),
            scale_dpi(17, dpi),
            scale_dpi(30, dpi),
            scale_dpi(30, dpi),
            Some(hwnd),
            None,
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(header_mark, 18, 17, 30, 30);
    let header_title = create_label(hwnd, "OpenViKey", 58, 14, 180, 22)?;
    add_layout(header_title, 58, 14, 180, 22);
    let header_subtitle = create_label(
        hwnd,
        "Bộ gõ tiếng Việt · Dữ liệu chỉ lưu trên máy",
        58,
        37,
        300,
        20,
    )?;
    add_layout(header_subtitle, 58, 37, 300, 20);

    let sidebar = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WC_TABCONTROLW,
            w!(""),
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, TCS_HOTTRACK),
            scale_dpi(16, dpi),
            scale_dpi(78, dpi),
            scale_dpi(728, dpi),
            scale_dpi(34, dpi),
            Some(hwnd),
            Some(HMENU(IDC_SIDEBAR as *mut core::ffi::c_void)),
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(sidebar, 16, 78, 728, 34);

    let nav_images = unsafe { ImageList_Create(16, 16, ILC_COLOR32 | ILC_MASK, 6, 1) };
    let icon_rows = [
        [
            0, 0x0180, 0x0180, 0x0DB0, 0x1FF8, 0x318C, 0x6186, 0x6386, 0x6186, 0x318C, 0x1FF8,
            0x0DB0, 0x0180, 0x0180, 0, 0,
        ],
        [
            0, 0x0180, 0x0180, 0x1998, 0x0DB0, 0x07E0, 0x3FFC, 0x07E0, 0x0DB0, 0x1998, 0x0180,
            0x0180, 0, 0x6000, 0x6000, 0,
        ],
        [
            0, 0x3FFC, 0x2004, 0x2AA4, 0x2004, 0x3FFC, 0x0000, 0x1FF8, 0x1008, 0x13C8, 0x1248,
            0x13C8, 0x1008, 0x1FF8, 0, 0,
        ],
        [
            0, 0, 0x3FFC, 0x2004, 0x2AA4, 0x2AA4, 0x2AA4, 0x2004, 0x27E4, 0x2004, 0x3FFC, 0, 0, 0,
            0, 0,
        ],
        [
            0, 0x0180, 0x07E0, 0x1FF8, 0x3FFC, 0x3FFC, 0x3FFC, 0x1FF8, 0x1FF8, 0x0FF0, 0x07E0,
            0x03C0, 0x0180, 0, 0, 0,
        ],
        [
            0, 0x07E0, 0x1818, 0x2004, 0x2184, 0x2184, 0x2004, 0x2184, 0x2184, 0x2184, 0x2004,
            0x1818, 0x07E0, 0, 0, 0,
        ],
    ];
    for rows in &icon_rows {
        if let Ok(icon) = create_nav_icon(rows) {
            unsafe {
                let _ = ImageList_ReplaceIcon(nav_images, -1, icon);
                let _ = DestroyIcon(icon);
            }
        }
    }
    unsafe {
        let _ = send_msg(sidebar, TCM_SETIMAGELIST, 0, nav_images.0);
    }

    let items = [
        "Chung",
        "Đã học",
        "Ứng dụng",
        "Phím tắt",
        "Riêng tư",
        "Giới thiệu",
    ];
    for (index, label) in items.iter().enumerate() {
        let label = HSTRING::from(*label);
        let mut item = TCITEMW {
            mask: TCIF_TEXT | TCIF_IMAGE,
            pszText: PWSTR(label.as_ptr().cast_mut()),
            iImage: i32::try_from(index).unwrap_or(-1),
            ..Default::default()
        };
        unsafe {
            let _ = send_msg(sidebar, TCM_INSERTITEMW, index, (&raw mut item) as isize);
        }
    }
    let accent_line = create_label(hwnd, "", 16, 102, 728, 2)?;
    add_layout(accent_line, 16, 102, 728, 2);

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
            w!("Hiện gợi ý gọn ở góc màn hình"),
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

    let grp_local = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!(" Riêng tư mặc định "),
            ws(WS_CHILD | WS_VISIBLE, BS_GROUPBOX as u32),
            scale_dpi(200, dpi),
            scale_dpi(370, dpi),
            scale_dpi(530, dpi),
            scale_dpi(65, dpi),
            Some(hwnd),
            None,
            Some(HINSTANCE::default()),
            None,
        )?
    };
    add_layout(grp_local, 200, 370, 530, 65);
    page_0.push(grp_local);
    let lbl_local = create_label(
        hwnd,
        "✓ Local-only · Password/PIN luôn bỏ qua · Terminal không học nội dung · Không telemetry",
        scale_dpi(214, dpi),
        scale_dpi(394, dpi),
        scale_dpi(500, dpi),
        scale_dpi(28, dpi),
    )?;
    add_layout(lbl_local, 214, 394, 500, 28);
    page_0.push(lbl_local);

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
            WC_LISTVIEWW,
            w!(""),
            ws(
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WS_VSCROLL,
                LVS_REPORT | LVS_SINGLESEL,
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
    unsafe {
        let _ = send_msg(
            list_rules,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            isize::try_from(LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER).unwrap_or(0),
        );
    }
    for (index, (title, width)) in [
        ("Đã gõ", 92),
        ("Thay bằng", 104),
        ("Trạng thái", 82),
        ("Bằng chứng", 92),
        ("Nguồn", 140),
    ]
    .into_iter()
    .enumerate()
    {
        let title = HSTRING::from(title);
        let mut column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
            cx: scale_dpi(width, dpi),
            pszText: PWSTR(title.as_ptr().cast_mut()),
            iSubItem: i32::try_from(index).unwrap_or(i32::MAX),
            ..Default::default()
        };
        unsafe {
            let _ = send_msg(
                list_rules,
                LVM_INSERTCOLUMNW,
                index,
                (&raw mut column) as isize,
            );
        }
    }
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

    let lbl_detail_rule_id = create_label(
        hwnd,
        "• Rule ID:       -",
        scale_dpi(216, dpi),
        scale_dpi(372, dpi),
        scale_dpi(500, dpi),
        scale_dpi(18, dpi),
    )?;
    add_layout(lbl_detail_rule_id, 216, 372, 500, 18);
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

    let mut make_control = |class: PCWSTR,
                            text: &str,
                            style: WINDOW_STYLE,
                            id: Option<usize>,
                            x: i32,
                            y: i32,
                            width: i32,
                            height: i32|
     -> Result<HWND> {
        let text = HSTRING::from(text);
        let control = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                PCWSTR(text.as_ptr()),
                style,
                scale_dpi(x, dpi),
                scale_dpi(y, dpi),
                scale_dpi(width, dpi),
                scale_dpi(height, dpi),
                Some(hwnd),
                id.map(|value| HMENU(value as *mut core::ffi::c_void)),
                Some(HINSTANCE::default()),
                None,
            )?
        };
        add_layout(control, x, y, width, height);
        Ok(control)
    };

    // Page 2 — per-application policy.
    let mut page_2 = Vec::new();
    page_2.push(make_control(
        w!("STATIC"),
        "Chính sách theo ứng dụng",
        WS_CHILD | WS_VISIBLE,
        None,
        204,
        16,
        526,
        24,
    )?);
    let list_apps = make_control(
        w!("LISTBOX"),
        "",
        ws(
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WS_VSCROLL,
            LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32,
        ),
        Some(IDC_LIST_APPS),
        204,
        46,
        526,
        180,
    )?;
    page_2.push(list_apps);
    page_2.push(make_control(
        w!("STATIC"),
        "Executable:",
        WS_CHILD | WS_VISIBLE,
        None,
        204,
        242,
        90,
        22,
    )?);
    let edit_app_exe = make_control(
        w!("EDIT"),
        "",
        ws(
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
            ES_AUTOHSCROLL as u32,
        ),
        Some(IDC_EDIT_APP_EXE),
        300,
        238,
        280,
        26,
    )?;
    page_2.push(edit_app_exe);
    let btn_app_current = make_control(
        w!("BUTTON"),
        "Ứng dụng hiện tại",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
        Some(IDC_BTN_APP_CURRENT),
        590,
        238,
        140,
        26,
    )?;
    page_2.push(btn_app_current);
    page_2.push(make_control(
        w!("STATIC"),
        "Chuyển đổi:",
        WS_CHILD | WS_VISIBLE,
        None,
        204,
        282,
        90,
        22,
    )?);
    let combo_app_transform = make_control(
        w!("COMBOBOX"),
        "",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, CBS_DROPDOWNLIST as u32),
        Some(IDC_COMBO_APP_TRANSFORM),
        300,
        278,
        150,
        120,
    )?;
    page_2.push(combo_app_transform);
    page_2.push(make_control(
        w!("STATIC"),
        "Learning:",
        WS_CHILD | WS_VISIBLE,
        None,
        470,
        282,
        70,
        22,
    )?);
    let combo_app_learning = make_control(
        w!("COMBOBOX"),
        "",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, CBS_DROPDOWNLIST as u32),
        Some(IDC_COMBO_APP_LEARNING),
        545,
        278,
        185,
        120,
    )?;
    page_2.push(combo_app_learning);
    page_2.push(make_control(
        w!("STATIC"),
        "Inject profile:",
        WS_CHILD | WS_VISIBLE,
        None,
        204,
        322,
        90,
        22,
    )?);
    let combo_app_profile = make_control(
        w!("COMBOBOX"),
        "",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, CBS_DROPDOWNLIST as u32),
        Some(IDC_COMBO_APP_PROFILE),
        300,
        318,
        150,
        120,
    )?;
    page_2.push(combo_app_profile);
    for (combo, values) in [
        (combo_app_transform, ["Default", "Allow", "Block"]),
        (combo_app_learning, ["Default", "Allow", "Block"]),
        (combo_app_profile, ["Auto", "Win32", "Electron"]),
    ] {
        for value in values {
            let value = HSTRING::from(value);
            unsafe {
                let _ = send_msg(combo, CB_ADDSTRING, 0, value.as_ptr() as isize);
            }
        }
        unsafe {
            let _ = send_msg(combo, CB_SETCURSEL, 0, 0);
        }
    }
    let btn_app_save = make_control(
        w!("BUTTON"),
        "Lưu policy",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_DEFPUSHBUTTON as u32),
        Some(IDC_BTN_APP_SAVE),
        300,
        370,
        120,
        30,
    )?;
    let btn_app_remove = make_control(
        w!("BUTTON"),
        "Xóa policy",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
        Some(IDC_BTN_APP_REMOVE),
        430,
        370,
        120,
        30,
    )?;
    page_2.extend([btn_app_save, btn_app_remove]);
    page_2.push(make_control(
        w!("STATIC"),
        "Credential apps luôn Block; Terminal luôn không học.",
        WS_CHILD | WS_VISIBLE,
        None,
        204,
        418,
        526,
        38,
    )?);

    // Page 3 — hotkeys.
    let mut page_3 = Vec::new();
    page_3.push(make_control(
        w!("STATIC"),
        "Phím tắt",
        WS_CHILD | WS_VISIBLE,
        None,
        204,
        16,
        526,
        24,
    )?);
    let hotkey_rows = [
        ("Đổi V/E", IDC_EDIT_HOTKEY_TOGGLE, 62),
        ("Nhận gợi ý", IDC_EDIT_HOTKEY_ACCEPT, 106),
        ("Từ chối gợi ý", IDC_EDIT_HOTKEY_REJECT, 150),
        ("Hoàn tác Auto", IDC_EDIT_HOTKEY_UNDO, 194),
        ("Quên rule cuối", IDC_EDIT_HOTKEY_FORGET, 238),
    ];
    let mut hotkey_edits = Vec::new();
    for (label, id, y) in hotkey_rows {
        page_3.push(make_control(
            w!("STATIC"),
            label,
            WS_CHILD | WS_VISIBLE,
            None,
            220,
            y + 4,
            180,
            24,
        )?);
        let edit = make_control(
            w!("EDIT"),
            "",
            ws(
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
                ES_AUTOHSCROLL as u32,
            ),
            Some(id),
            410,
            y,
            260,
            28,
        )?;
        page_3.push(edit);
        hotkey_edits.push(edit);
    }
    let edit_hotkey_toggle = hotkey_edits[0];
    let edit_hotkey_accept = hotkey_edits[1];
    let edit_hotkey_reject = hotkey_edits[2];
    let edit_hotkey_undo = hotkey_edits[3];
    let edit_hotkey_forget = hotkey_edits[4];
    let btn_hotkey_save = make_control(
        w!("BUTTON"),
        "Lưu phím tắt",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_DEFPUSHBUTTON as u32),
        Some(IDC_BTN_HOTKEY_SAVE),
        410,
        300,
        125,
        30,
    )?;
    let btn_hotkey_reset = make_control(
        w!("BUTTON"),
        "Mặc định",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
        Some(IDC_BTN_HOTKEY_RESET),
        545,
        300,
        125,
        30,
    )?;
    page_3.extend([btn_hotkey_save, btn_hotkey_reset]);
    page_3.push(make_control(
        w!("STATIC"),
        "Định dạng: Ctrl+., Ctrl+Shift+Z, Alt+Space. Không cho trùng phím.",
        WS_CHILD | WS_VISIBLE,
        None,
        220,
        352,
        500,
        42,
    )?);

    // Page 4 — privacy.
    let mut page_4 = Vec::new();
    page_4.push(make_control(
        w!("STATIC"),
        "Quyền riêng tư",
        WS_CHILD | WS_VISIBLE,
        None,
        204,
        16,
        526,
        28,
    )?);
    page_4.push(make_control(w!("STATIC"), "✓ Local-only, không backend hoặc telemetry\r\n✓ Không tự động upload\r\n✓ Password/PIN pass-through, không học\r\n✓ Terminal được biến đổi tùy chọn nhưng không learning/capture\r\n✓ Không đăng ký TSF/COM", WS_CHILD | WS_VISIBLE, None, 220, 62, 500, 150)?);
    page_4.push(make_control(
        w!("STATIC"),
        "Dữ liệu preview là JSON cục bộ; không chia sẻ nếu chứa từ viết tắt riêng.",
        WS_CHILD | WS_VISIBLE,
        None,
        220,
        230,
        500,
        50,
    )?);
    let lbl_privacy_path = make_control(
        w!("STATIC"),
        "",
        WS_CHILD | WS_VISIBLE,
        None,
        220,
        300,
        500,
        44,
    )?;
    page_4.push(lbl_privacy_path);
    let btn_open_data = make_control(
        w!("BUTTON"),
        "Mở thư mục dữ liệu",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
        Some(IDC_BTN_OPEN_DATA),
        220,
        360,
        175,
        32,
    )?;
    page_4.push(btn_open_data);

    // Page 5 — about and safe diagnostics.
    let mut page_5 = Vec::new();
    page_5.push(make_control(
        w!("STATIC"),
        "OpenViKey Standalone Preview · v0.1.0 · x64",
        WS_CHILD | WS_VISIBLE,
        None,
        204,
        16,
        526,
        30,
    )?);
    page_5.push(make_control(
        w!("STATIC"),
        "MIT License · OpenViKey Authors",
        WS_CHILD | WS_VISIBLE,
        None,
        220,
        54,
        500,
        24,
    )?);
    let lbl_about_runtime = make_control(
        w!("STATIC"),
        "",
        WS_CHILD | WS_VISIBLE,
        None,
        220,
        100,
        500,
        230,
    )?;
    page_5.push(lbl_about_runtime);
    let btn_refresh_about = make_control(
        w!("BUTTON"),
        "Làm mới chẩn đoán",
        ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_PUSHBUTTON as u32),
        Some(IDC_BTN_REFRESH_ABOUT),
        220,
        360,
        175,
        32,
    )?;
    page_5.push(btn_refresh_about);

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
            ws(WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_OWNERDRAW as u32),
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

    // A+B layout: compact horizontal navigation with the visual breathing room
    // of the modern variant. Non-general pages keep their proven vertical
    // structure but use the full window width.
    let page_handles: Vec<HWND> = page_0
        .iter()
        .chain(&page_1)
        .chain(&page_2)
        .chain(&page_3)
        .chain(&page_4)
        .chain(&page_5)
        .copied()
        .collect();
    for layout in &mut layouts {
        if page_handles.contains(&layout.hwnd) {
            let old_right = layout.base_x + layout.base_w;
            layout.base_x -= 180;
            layout.base_y += 95;
            if old_right >= 700 {
                layout.base_w += 180;
            }
        }
        if page_2.contains(&layout.hwnd) && layout.base_y >= 333 {
            layout.base_y -= 35;
        }
    }

    override_layout(&mut layouts, list_apps, 24, 141, 706, 145);
    override_layout(&mut layouts, grp_mode, 410, 8, 320, 52);
    override_layout(&mut layouts, radio_viet, 428, 28, 125, 22);
    override_layout(&mut layouts, radio_eng, 565, 28, 138, 22);
    override_layout(&mut layouts, sidebar, 16, 68, 728, 34);
    override_layout(&mut layouts, grp_method, 20, 112, 330, 60);
    override_layout(&mut layouts, radio_vni, 40, 134, 125, 22);
    override_layout(&mut layouts, radio_telex, 175, 134, 125, 22);
    override_layout(&mut layouts, lbl_tone, 380, 115, 350, 20);
    override_layout(&mut layouts, combo_tone, 380, 137, 350, 120);
    override_layout(&mut layouts, lbl_startup, 20, 184, 165, 22);
    override_layout(&mut layouts, combo_startup_mode, 200, 180, 530, 120);
    override_layout(&mut layouts, chk_suggestions, 20, 220, 710, 26);
    override_layout(&mut layouts, chk_autostart, 20, 253, 710, 26);
    override_layout(&mut layouts, grp_terminal, 20, 286, 710, 92);
    override_layout(&mut layouts, chk_terminal, 38, 309, 675, 24);
    override_layout(&mut layouts, lbl_term_hint, 38, 338, 675, 30);
    override_layout(&mut layouts, grp_local, 20, 393, 710, 62);
    override_layout(&mut layouts, lbl_local, 38, 417, 675, 26);
    override_layout(&mut layouts, lbl_search, 24, 111, 95, 20);
    override_layout(&mut layouts, txt_search_rules, 124, 109, 606, 24);
    override_layout(&mut layouts, list_rules, 24, 139, 706, 190);
    override_layout(&mut layouts, grp_details, 24, 338, 706, 124);
    override_layout(&mut layouts, lbl_detail_original, 40, 358, 325, 18);
    override_layout(&mut layouts, lbl_detail_candidate, 380, 358, 325, 18);
    override_layout(&mut layouts, lbl_detail_method, 40, 380, 325, 18);
    override_layout(&mut layouts, lbl_detail_source, 380, 380, 325, 18);
    override_layout(&mut layouts, lbl_detail_evidence, 40, 402, 325, 18);
    override_layout(&mut layouts, lbl_detail_state, 380, 402, 325, 18);
    override_layout(&mut layouts, lbl_detail_rule_id, 40, 424, 665, 18);
    override_layout(&mut layouts, btn_forget_selected, 24, 474, 140, 30);
    override_layout(&mut layouts, btn_forget_last, 172, 474, 140, 30);
    override_layout(&mut layouts, btn_refresh_rules, 320, 474, 105, 30);
    override_layout(&mut layouts, status_label, 18, 545, 480, 20);
    override_layout(&mut layouts, btn_close, 515, 536, 100, 32);
    override_layout(&mut layouts, btn_apply, 625, 536, 105, 32);

    for item in &layouts {
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
            let _ = send_msg(item.hwnd, WM_SETFONT, font.0 as usize, 1);
        }
    }

    let accent_brush = unsafe { CreateSolidBrush(COLORREF(0x00DA_6909)) };
    let accent_pressed_brush = unsafe { CreateSolidBrush(COLORREF(0x00AD_5407)) };
    let soft_blue_brush = unsafe { CreateSolidBrush(COLORREF(0x00FF_F3EA)) };
    let window_brush = unsafe { CreateSolidBrush(COLORREF(0x00FF_FFFF)) };

    let controls = UiControls {
        sidebar,
        header_mark,
        header_title,
        header_subtitle,
        accent_line,
        lbl_local,
        btn_apply,
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
        lbl_detail_rule_id,
        btn_forget_selected,
        btn_forget_last,
        btn_refresh_rules,
        list_apps,
        edit_app_exe,
        combo_app_transform,
        combo_app_learning,
        combo_app_profile,
        edit_hotkey_toggle,
        edit_hotkey_accept,
        edit_hotkey_reject,
        edit_hotkey_undo,
        edit_hotkey_forget,
        lbl_privacy_path,
        lbl_about_runtime,
        filtered_rows: Vec::new(),
        all_rows: Vec::new(),
        status_label,
        current_font: font,
        nav_images,
        accent_brush,
        accent_pressed_brush,
        soft_blue_brush,
        window_brush,
        page_controls: [page_0, page_1, page_2, page_3, page_4, page_5],
        persistent_controls: vec![
            header_mark,
            header_title,
            header_subtitle,
            sidebar,
            accent_line,
            grp_mode,
            radio_viet,
            radio_eng,
            status_label,
            btn_close,
            btn_apply,
        ],
        active_page: None,
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
        let _ = send_msg(controls.sidebar, TCM_SETCURSEL, target, 0);
        let _ = SetFocus(Some(controls.sidebar));
    }
    if controls.active_page == Some(target) {
        let redraw_controls: Vec<HWND> = controls.page_controls[target]
            .iter()
            .chain(&controls.persistent_controls)
            .copied()
            .collect();
        drop(guard);
        unsafe {
            for ctrl in redraw_controls {
                let _ = RedrawWindow(
                    Some(ctrl),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW,
                );
            }
        }
        return;
    }

    unsafe {
        let _ = send_msg(hwnd, WM_SETREDRAW, 0, 0);
    }
    if let Some(previous) = controls.active_page {
        for &ctrl in &controls.page_controls[previous] {
            unsafe {
                let _ = ShowWindow(ctrl, SW_HIDE);
            }
        }
    } else {
        for page in &controls.page_controls {
            for &ctrl in page {
                unsafe {
                    let _ = ShowWindow(ctrl, SW_HIDE);
                }
            }
        }
    }

    // Clear the previous page while every content control is hidden. Painting only
    // after showing the next page leaves stale pixels beneath transparent labels
    // because the parent uses WS_CLIPCHILDREN.
    unsafe {
        let _ = send_msg(hwnd, WM_SETREDRAW, 1, 0);
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW,
        );
        let _ = send_msg(hwnd, WM_SETREDRAW, 0, 0);
    }
    for &ctrl in &controls.page_controls[target] {
        unsafe {
            let _ = ShowWindow(ctrl, SW_SHOW);
        }
    }
    controls.active_page = Some(target);
    let redraw_controls: Vec<HWND> = controls.page_controls[target]
        .iter()
        .chain(&controls.persistent_controls)
        .copied()
        .collect();
    drop(guard);

    unsafe {
        let _ = send_msg(hwnd, WM_SETREDRAW, 1, 0);
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
        );
        for ctrl in redraw_controls {
            let _ = RedrawWindow(
                Some(ctrl),
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW,
            );
        }
    }
}

fn rule_state_label(state: openvikey_core::decision::DecisionState) -> &'static str {
    match state {
        openvikey_core::decision::DecisionState::Ignore => "Observed",
        openvikey_core::decision::DecisionState::Suggest => "Suggest",
        openvikey_core::decision::DecisionState::Auto => "Auto",
    }
}

fn rule_source_label(source: CandidateSource) -> &'static str {
    match source {
        CandidateSource::Personal => "Cá nhân",
        CandidateSource::Abbreviation => "Viết tắt",
        CandidateSource::TelexFix => "Sửa Telex/VNI",
        CandidateSource::Fuzzy => "Gõ sai",
        CandidateSource::Diacritics => "Dấu thanh",
    }
}

fn selected_rule_index(list: HWND) -> Option<usize> {
    let index = unsafe {
        send_msg(
            list,
            LVM_GETNEXTITEM,
            usize::MAX,
            isize::try_from(LVNI_SELECTED).unwrap_or(0),
        )
        .0
    };
    usize::try_from(index).ok()
}

fn set_rule_cell(list: HWND, row: usize, column: usize, text: &str, insert: bool) {
    let text = HSTRING::from(text);
    let mut item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: i32::try_from(row).unwrap_or(i32::MAX),
        iSubItem: i32::try_from(column).unwrap_or(i32::MAX),
        pszText: PWSTR(text.as_ptr().cast_mut()),
        ..Default::default()
    };
    unsafe {
        let message = if insert {
            LVM_INSERTITEMW
        } else {
            LVM_SETITEMW
        };
        let _ = send_msg(list, message, 0, (&raw mut item) as isize);
    }
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
                "Observed (Đang quan sát / Chưa đủ tin cậy)"
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
        let _ = send_msg(controls.list_rules, WM_SETREDRAW, 0, 0);
        let _ = send_msg(controls.list_rules, LVM_DELETEALLITEMS, 0, 0);
    }

    for (index, row) in controls.filtered_rows.iter().enumerate() {
        let evidence = format!(
            "+{:.1} / -{:.1}",
            row.positive_evidence, row.negative_evidence
        );
        set_rule_cell(controls.list_rules, index, 0, &row.original_nfc, true);
        set_rule_cell(controls.list_rules, index, 1, &row.candidate_nfc, false);
        set_rule_cell(
            controls.list_rules,
            index,
            2,
            rule_state_label(row.state),
            false,
        );
        set_rule_cell(controls.list_rules, index, 3, &evidence, false);
        set_rule_cell(
            controls.list_rules,
            index,
            4,
            rule_source_label(row.source),
            false,
        );
    }

    if controls.filtered_rows.is_empty() {
        update_rule_details_view(controls, None);
    } else {
        let mut selected = LVITEMW {
            state: LVIS_SELECTED,
            stateMask: LVIS_SELECTED,
            ..Default::default()
        };
        unsafe {
            let _ = send_msg(
                controls.list_rules,
                LVM_SETITEMSTATE,
                0,
                (&raw mut selected) as isize,
            );
        }
        update_rule_details_view(controls, controls.filtered_rows.first());
    }

    unsafe {
        let _ = send_msg(controls.list_rules, WM_SETREDRAW, 1, 0);
        let _ = RedrawWindow(
            Some(controls.list_rules),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW,
        );
    }
}

fn populate_rules_inner(controls: &mut UiControls) {
    let snapshot = crate::host::control_snapshot();
    let mut rows = snapshot.map_or_else(Vec::new, |s| s.learned_rows);
    rows.reverse();
    controls.all_rows = rows;
    apply_rules_filter_inner(controls);
}

fn set_text(hwnd: HWND, value: &str) {
    let value = HSTRING::from(value);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(value.as_ptr()));
    }
}

fn populate_app_policies(controls: &UiControls, settings: &crate::settings::SettingsV1) {
    unsafe {
        let _ = send_msg(controls.list_apps, LB_RESETCONTENT, 0, 0);
    }
    for policy in &settings.app_policies {
        let row = format!(
            "{}  ·  {:?}  ·  {:?}  ·  {:?}",
            policy.executable, policy.transform, policy.learning, policy.inject_profile
        );
        let row = HSTRING::from(row);
        unsafe {
            let _ = send_msg(controls.list_apps, LB_ADDSTRING, 0, row.as_ptr() as isize);
        }
    }
}

fn populate_non_general_pages(controls: &UiControls, settings: &crate::settings::SettingsV1) {
    populate_app_policies(controls, settings);
    set_text(controls.edit_hotkey_toggle, &settings.hotkeys.toggle_mode);
    set_text(controls.edit_hotkey_accept, &settings.hotkeys.accept_top);
    set_text(controls.edit_hotkey_reject, &settings.hotkeys.reject_top);
    set_text(controls.edit_hotkey_undo, &settings.hotkeys.undo_last);
    set_text(controls.edit_hotkey_forget, &settings.hotkeys.forget_last);
    let data =
        default_settings_path(std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from));
    set_text(
        controls.lbl_privacy_path,
        &format!(
            "Thư mục dữ liệu:\r\n{}",
            data.parent().unwrap_or(&data).display()
        ),
    );
    refresh_about(controls);
}

fn refresh_about(controls: &UiControls) {
    let snapshot = crate::host::control_snapshot();
    let text = snapshot.map_or_else(
        || "Runtime chưa sẵn sàng.".to_owned(),
        |snapshot| format!(
            "Runtime: đang chạy\r\nChế độ: {:?} · {:?}\r\nỨng dụng hiện tại: {}\r\nLearning tại đây: {}\r\nRule đã học: {}\r\nSingle instance: bật\r\nTSF / COM: không đăng ký",
            snapshot.mode,
            snapshot.engine_config.method,
            if snapshot.foreground_exe.is_empty() { "(chưa xác định)" } else { &snapshot.foreground_exe },
            if snapshot.learning_allowed { "Bật" } else { "Tắt" },
            snapshot.learned_rows.len(),
        ),
    );
    set_text(controls.lbl_about_runtime, &text);
}

fn selected_app_index(controls: &UiControls) -> Option<usize> {
    let index = unsafe { send_msg(controls.list_apps, LB_GETCURSEL, 0, 0).0 };
    usize::try_from(index).ok()
}

fn show_selected_app_policy(controls: &UiControls) {
    let Some(index) = selected_app_index(controls) else {
        return;
    };
    let path =
        default_settings_path(std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from));
    let Ok(settings) = load_settings(&path) else {
        return;
    };
    let Some(policy) = settings.app_policies.get(index) else {
        return;
    };
    set_text(controls.edit_app_exe, &policy.executable);
    let transform = match policy.transform {
        AppTransformPolicy::Default => 0,
        AppTransformPolicy::Allow => 1,
        AppTransformPolicy::Block => 2,
    };
    let learning = match policy.learning {
        AppLearningPolicy::Default => 0,
        AppLearningPolicy::Allow => 1,
        AppLearningPolicy::Block => 2,
    };
    let profile = match policy.inject_profile {
        AppInjectProfile::Auto => 0,
        AppInjectProfile::Win32 => 1,
        AppInjectProfile::Electron => 2,
    };
    unsafe {
        let _ = send_msg(controls.combo_app_transform, CB_SETCURSEL, transform, 0);
        let _ = send_msg(controls.combo_app_learning, CB_SETCURSEL, learning, 0);
        let _ = send_msg(controls.combo_app_profile, CB_SETCURSEL, profile, 0);
    }
}

fn combo_index(hwnd: HWND) -> usize {
    unsafe { send_msg(hwnd, CB_GETCURSEL, 0, 0).0.cast_unsigned() }
}

fn save_app_policy(controls: &UiControls) -> std::result::Result<(), String> {
    let raw = get_edit_text(controls.edit_app_exe);
    let executable = std::path::Path::new(raw.trim())
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_owned();
    if executable.is_empty() || !executable.to_ascii_lowercase().ends_with(".exe") {
        return Err("Executable phải là tên file .exe hợp lệ".into());
    }
    let mut transform = match combo_index(controls.combo_app_transform) {
        1 => AppTransformPolicy::Allow,
        2 => AppTransformPolicy::Block,
        _ => AppTransformPolicy::Default,
    };
    let mut learning = match combo_index(controls.combo_app_learning) {
        1 => AppLearningPolicy::Allow,
        2 => AppLearningPolicy::Block,
        _ => AppLearningPolicy::Default,
    };
    let inject_profile = match combo_index(controls.combo_app_profile) {
        1 => AppInjectProfile::Win32,
        2 => AppInjectProfile::Electron,
        _ => AppInjectProfile::Auto,
    };
    if crate::policy::is_denylisted(&executable) {
        transform = AppTransformPolicy::Block;
        learning = AppLearningPolicy::Block;
    } else if crate::policy::is_terminal_exe(&executable) {
        learning = AppLearningPolicy::Block;
    }
    let path =
        default_settings_path(std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from));
    let mut settings = load_settings(&path).map_err(|error| error.to_string())?;
    let policy = AppPolicyV1 {
        executable: executable.clone(),
        transform,
        learning,
        inject_profile,
    };
    if let Some(existing) = settings
        .app_policies
        .iter_mut()
        .find(|item| item.executable.eq_ignore_ascii_case(&executable))
    {
        *existing = policy;
    } else {
        settings.app_policies.push(policy);
    }
    crate::ui_coordinator::execute(crate::ui_coordinator::UiCommand::SetAppPolicies(
        settings.app_policies.clone(),
    ))?;
    populate_app_policies(controls, &settings);
    Ok(())
}

fn remove_selected_app_policy(controls: &UiControls) -> std::result::Result<bool, String> {
    let Some(index) = selected_app_index(controls) else {
        return Ok(false);
    };
    let path =
        default_settings_path(std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from));
    let mut settings = load_settings(&path).map_err(|error| error.to_string())?;
    if index >= settings.app_policies.len() {
        return Ok(false);
    }
    settings.app_policies.remove(index);
    crate::ui_coordinator::execute(crate::ui_coordinator::UiCommand::SetAppPolicies(
        settings.app_policies.clone(),
    ))?;
    populate_app_policies(controls, &settings);
    Ok(true)
}

fn save_hotkeys(controls: &UiControls) -> std::result::Result<(), String> {
    let hotkeys = HotkeySettingsV1 {
        toggle_mode: get_edit_text(controls.edit_hotkey_toggle),
        accept_top: get_edit_text(controls.edit_hotkey_accept),
        reject_top: get_edit_text(controls.edit_hotkey_reject),
        undo_last: get_edit_text(controls.edit_hotkey_undo),
        forget_last: get_edit_text(controls.edit_hotkey_forget),
    };
    crate::ui_coordinator::execute(crate::ui_coordinator::UiCommand::SetHotkeys(hotkeys))
        .map(|_| ())
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
    populate_non_general_pages(controls, &settings);
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

    let command = crate::ui_coordinator::UiCommand::ApplyGeneral(
        crate::ui_coordinator::GeneralSettingsDraft {
            mode,
            input_method: method,
            tone_placement,
            mode_on_start,
            show_suggestions,
            allow_terminal,
            start_with_windows,
        },
    );
    let status_text = match crate::ui_coordinator::execute(command) {
        Ok(_) => HSTRING::from("✓ Đã lưu và áp dụng cài đặt thành công."),
        Err(error) => HSTRING::from(format!("⚠ Không thể áp dụng cài đặt: {error}")),
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

    let Some(selected) = selected_rule_index(controls.list_rules) else {
        return;
    };
    let Some(row) = controls.filtered_rows.get(selected).cloned() else {
        return;
    };

    let changed = matches!(
        crate::ui_coordinator::execute(crate::ui_coordinator::UiCommand::ForgetRule(row.clone())),
        Ok(crate::ui_coordinator::UiCommandOutcome::Changed(true))
    );
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
    let changed = matches!(
        crate::ui_coordinator::execute(crate::ui_coordinator::UiCommand::ForgetLastRule),
        Ok(crate::ui_coordinator::UiCommandOutcome::Changed(true))
    );

    let Ok(mut guard) = CONTROLS.lock() else {
        return;
    };
    let Some(controls) = guard.as_mut() else {
        return;
    };

    populate_rules_inner(controls);
    let message = if changed {
        "✓ Đã quên rule vừa học gần nhất."
    } else {
        "⚠ Không có rule vừa học để quên."
    };
    unsafe {
        let text = HSTRING::from(message);
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

#[allow(clippy::too_many_lines)]
unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CTLCOLORSTATIC => {
            let Ok(guard) = CONTROLS.try_lock() else {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            };
            let Some(controls) = guard.as_ref() else {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            };
            let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
            let control = HWND(lparam.0 as *mut core::ffi::c_void);
            let (brush, background, foreground) =
                if control == controls.header_mark || control == controls.accent_line {
                    (
                        controls.accent_brush,
                        COLORREF(0x00DA_6909),
                        COLORREF(0x00FF_FFFF),
                    )
                } else if control == controls.lbl_local {
                    (
                        controls.soft_blue_brush,
                        COLORREF(0x00FF_F3EA),
                        COLORREF(0x0092_5B20),
                    )
                } else if control == controls.header_title {
                    (
                        controls.window_brush,
                        COLORREF(0x00FF_FFFF),
                        COLORREF(0x00A8_5708),
                    )
                } else if control == controls.header_subtitle {
                    (
                        controls.window_brush,
                        COLORREF(0x00FF_FFFF),
                        COLORREF(0x0072_6353),
                    )
                } else if control == controls.status_label {
                    (
                        controls.window_brush,
                        COLORREF(0x00FF_FFFF),
                        COLORREF(0x005B_8416),
                    )
                } else {
                    (
                        controls.window_brush,
                        COLORREF(0x00FF_FFFF),
                        COLORREF(0x002A_2017),
                    )
                };
            unsafe {
                let _ = SetBkColor(hdc, background);
                let _ = SetTextColor(hdc, foreground);
            }
            LRESULT(brush.0 as isize)
        }
        WM_CTLCOLORBTN => {
            let Ok(guard) = CONTROLS.try_lock() else {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            };
            let Some(controls) = guard.as_ref() else {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            };
            let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
            unsafe {
                let _ = SetBkColor(hdc, COLORREF(0x00FF_FFFF));
                let _ = SetTextColor(hdc, COLORREF(0x002A_2017));
            }
            LRESULT(controls.window_brush.0 as isize)
        }
        WM_DRAWITEM => {
            if lparam.0 == 0 {
                return LRESULT(0);
            }
            let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            if usize::try_from(item.CtlID).unwrap_or(0) != IDC_BTN_APPLY {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }
            let Ok(guard) = CONTROLS.try_lock() else {
                return LRESULT(0);
            };
            let Some(controls) = guard.as_ref() else {
                return LRESULT(0);
            };
            let selected = item.itemState.0 & ODS_SELECTED.0 != 0;
            let brush = if selected {
                controls.accent_pressed_brush
            } else {
                controls.accent_brush
            };
            let mut rect = item.rcItem;
            let mut text: Vec<u16> = "Áp dụng".encode_utf16().collect();
            unsafe {
                let _ = FillRect(item.hDC, &raw const rect, brush);
                let _ = SetBkMode(item.hDC, TRANSPARENT);
                let _ = SetTextColor(item.hDC, COLORREF(0x00FF_FFFF));
                let _ = DrawTextW(
                    item.hDC,
                    &mut text,
                    &raw mut rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                if item.itemState.0 & ODS_FOCUS.0 != 0 {
                    let _ = DrawFocusRect(item.hDC, &raw const rect);
                }
            }
            LRESULT(1)
        }
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
        WM_NOTIFY => {
            if lparam.0 == 0 {
                return LRESULT(0);
            }
            let header = unsafe { &*(lparam.0 as *const NMHDR) };
            if header.idFrom == IDC_SIDEBAR && header.code == TCN_SELCHANGE {
                let Ok(guard) = CONTROLS.lock() else {
                    return LRESULT(0);
                };
                let Some(controls) = guard.as_ref() else {
                    return LRESULT(0);
                };
                let page = unsafe {
                    send_msg(controls.sidebar, TCM_GETCURSEL, 0, 0)
                        .0
                        .cast_unsigned()
                };
                drop(guard);
                select_page(hwnd, page);
                return LRESULT(0);
            }
            if header.idFrom == IDC_LIST_RULES && header.code == LVN_ITEMCHANGED {
                let Ok(guard) = CONTROLS.try_lock() else {
                    return LRESULT(0);
                };
                if let Some(controls) = guard.as_ref() {
                    let row = selected_rule_index(controls.list_rules)
                        .and_then(|index| controls.filtered_rows.get(index));
                    update_rule_details_view(controls, row);
                }
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
            if id == IDC_LIST_APPS && code == LBN_SELCHANGE as usize {
                if let Ok(guard) = CONTROLS.lock()
                    && let Some(controls) = guard.as_ref()
                {
                    show_selected_app_policy(controls);
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_APP_CURRENT {
                if let Ok(guard) = CONTROLS.lock()
                    && let Some(controls) = guard.as_ref()
                    && let Some(snapshot) = crate::host::control_snapshot()
                {
                    set_text(controls.edit_app_exe, &snapshot.last_external_exe);
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_APP_SAVE {
                if let Ok(guard) = CONTROLS.lock()
                    && let Some(controls) = guard.as_ref()
                {
                    let message = match save_app_policy(controls) {
                        Ok(()) => "✓ Đã lưu và áp dụng policy ứng dụng.".to_owned(),
                        Err(error) => format!("⚠ Không thể lưu policy: {error}"),
                    };
                    set_text(controls.status_label, &message);
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_APP_REMOVE {
                if let Ok(guard) = CONTROLS.lock()
                    && let Some(controls) = guard.as_ref()
                {
                    let message = match remove_selected_app_policy(controls) {
                        Ok(true) => "✓ Đã xóa policy ứng dụng.".to_owned(),
                        Ok(false) => "⚠ Chưa chọn policy để xóa.".to_owned(),
                        Err(error) => format!("⚠ Không thể xóa policy: {error}"),
                    };
                    set_text(controls.status_label, &message);
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_HOTKEY_SAVE {
                if let Ok(guard) = CONTROLS.lock()
                    && let Some(controls) = guard.as_ref()
                {
                    let message = match save_hotkeys(controls) {
                        Ok(()) => "✓ Đã lưu và áp dụng phím tắt.".to_owned(),
                        Err(error) => format!("⚠ Phím tắt không hợp lệ: {error}"),
                    };
                    set_text(controls.status_label, &message);
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_HOTKEY_RESET {
                if let Ok(guard) = CONTROLS.lock()
                    && let Some(controls) = guard.as_ref()
                {
                    let defaults = HotkeySettingsV1::default();
                    set_text(controls.edit_hotkey_toggle, &defaults.toggle_mode);
                    set_text(controls.edit_hotkey_accept, &defaults.accept_top);
                    set_text(controls.edit_hotkey_reject, &defaults.reject_top);
                    set_text(controls.edit_hotkey_undo, &defaults.undo_last);
                    set_text(controls.edit_hotkey_forget, &defaults.forget_last);
                    let _ = crate::ui_coordinator::execute(
                        crate::ui_coordinator::UiCommand::SetHotkeys(defaults),
                    );
                    set_text(controls.status_label, "✓ Đã khôi phục phím tắt mặc định.");
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_OPEN_DATA {
                let path = default_settings_path(
                    std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from),
                );
                if let Some(folder) = path.parent() {
                    let _ = std::process::Command::new("explorer.exe")
                        .arg(folder)
                        .spawn();
                }
                return LRESULT(0);
            }
            if id == IDC_BTN_REFRESH_ABOUT {
                if let Ok(guard) = CONTROLS.lock()
                    && let Some(controls) = guard.as_ref()
                {
                    refresh_about(controls);
                }
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
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Ok(mut guard) = SETTINGS_HWND.lock() {
                *guard = None;
            }
            if let Ok(mut guard) = CONTROLS.lock()
                && let Some(controls) = guard.take()
            {
                unsafe {
                    if !controls.current_font.is_invalid() {
                        let _ = DeleteObject(controls.current_font.into());
                    }
                    let _ = ImageList_Destroy(Some(controls.nav_images));
                    let _ = DeleteObject(controls.accent_brush.into());
                    let _ = DeleteObject(controls.accent_pressed_brush.into());
                    let _ = DeleteObject(controls.soft_blue_brush.into());
                    let _ = DeleteObject(controls.window_brush.into());
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
