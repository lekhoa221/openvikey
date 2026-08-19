//! Lightweight standalone control surface backed by the one live host session.

use openvikey_core::types::InputMethod;

use crate::host::ControlSnapshot;
use crate::policy::Mode;

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
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        use windows::core::{HSTRING, PCWSTR, w};
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

#[cfg(windows)]
pub fn show_control_window(owner: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
    use windows::core::{HSTRING, PCWSTR, w};

    let Some(snapshot) = crate::host::control_snapshot() else {
        return;
    };
    let text = HSTRING::from(format_control_snapshot(&snapshot));
    unsafe {
        let _ = MessageBoxW(
            Some(owner),
            PCWSTR(text.as_ptr()),
            w!("OpenViKey — Cài đặt và rule đã học"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
