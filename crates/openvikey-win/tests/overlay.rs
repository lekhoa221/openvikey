//! Overlay candidate line cap (display-only helper).

use openvikey_win::overlay::{OverlayPresentation, overlay_lines, overlay_presentation};

#[test]
fn overlay_caps_at_three() {
    let lines = overlay_lines(&["a".into(), "b".into(), "c".into(), "d".into()], 3);
    assert_eq!(lines, ["a", "b", "c"]);
}

#[test]
fn candidates_show_and_empty_candidates_hide_the_overlay() {
    assert_eq!(
        overlay_presentation(&["không".into(), "khổng".into()], 3),
        OverlayPresentation::Visible(vec!["không".into(), "khổng".into()])
    );
    assert_eq!(overlay_presentation(&[], 3), OverlayPresentation::Hidden);
}

#[cfg(windows)]
#[test]
fn windows_overlay_is_visible_with_text_and_hides_when_empty() {
    use openvikey_win::overlay::OverlayWindow;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, GetWindowTextW, IsWindowVisible};

    let overlay = unsafe { OverlayWindow::create().expect("create overlay") };
    unsafe {
        overlay.set_lines(&["không".into(), "khổng".into()]);
    }
    assert!(unsafe { IsWindowVisible(overlay.hwnd()) }.as_bool());

    let mut text = [0u16; 128];
    let len = unsafe { GetWindowTextW(overlay.hwnd(), &mut text) };
    assert!(len > 0);
    let text = String::from_utf16_lossy(&text[..usize::try_from(len).unwrap_or(0)]);
    assert!(text.contains("không"));

    let mut rect = RECT::default();
    unsafe { GetWindowRect(overlay.hwnd(), &raw mut rect).expect("overlay rect") };
    assert!(rect.right > rect.left && rect.bottom > rect.top);

    unsafe {
        overlay.set_lines(&[]);
    }
    assert!(!unsafe { IsWindowVisible(overlay.hwnd()) }.as_bool());
}
