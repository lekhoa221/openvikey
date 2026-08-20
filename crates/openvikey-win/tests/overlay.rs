//! Compact suggestion/learning capsule presentation helpers.

use openvikey_session::session::{LearningNotice, LearningNoticeKind};
use openvikey_win::overlay::{
    OverlayPresentation, overlay_display_lines, overlay_lines, overlay_presentation,
    overlay_presentation_if,
};

#[test]
fn overlay_caps_at_three() {
    let lines = overlay_lines(&["a".into(), "b".into(), "c".into(), "d".into()], 3);
    assert_eq!(lines, ["a", "b", "c"]);
}

#[test]
fn candidates_show_and_empty_candidates_hide_the_overlay() {
    assert_eq!(
        overlay_presentation(&["không".into(), "khổng".into()], 3),
        OverlayPresentation::Suggestion {
            candidate: "không".into(),
            position: 1,
            total: 2,
        }
    );
    assert_eq!(overlay_presentation(&[], 3), OverlayPresentation::Hidden);
}

#[test]
fn disabled_suggestions_hide_overlay_even_with_candidates() {
    let candidates = ["không".into(), "khổng".into()];
    assert_eq!(
        overlay_presentation_if(&candidates, 3, false),
        OverlayPresentation::Hidden
    );
    assert!(overlay_display_lines(&candidates, 3, false).is_empty());
    assert_eq!(
        overlay_display_lines(&candidates, 3, true),
        vec!["không".to_string(), "khổng".to_string()]
    );
}

#[test]
fn corrected_notice_tells_the_user_to_backspace() {
    let notice = LearningNotice {
        kind: LearningNoticeKind::Corrected,
        original_nfc: "ko".into(),
        replacement_nfc: "không".into(),
        positive_delta: 0.0,
        negative_delta: 0.0,
        positive_total: 0.0,
        negative_total: 0.0,
    };
    let text = notice.display_text();
    assert!(text.starts_with("Đã sửa: ko → không"));
    assert!(text.contains("Backspace"));
}

#[test]
fn learning_notice_exposes_delta_and_accumulated_points() {
    let notice = LearningNotice {
        kind: LearningNoticeKind::Accepted,
        original_nfc: "paht".into(),
        replacement_nfc: "phát".into(),
        positive_delta: 1.0,
        negative_delta: 0.0,
        positive_total: 4.0,
        negative_total: 0.2,
    };
    let text = notice.display_text();
    assert!(text.contains("+1.0 điểm"));
    assert!(text.contains("Tích lũy +4.0/-0.2"));
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

    let mut suggestion_rect = RECT::default();
    unsafe { GetWindowRect(overlay.hwnd(), &raw mut suggestion_rect).expect("overlay rect") };
    assert!(
        suggestion_rect.right > suggestion_rect.left
            && suggestion_rect.bottom > suggestion_rect.top
    );

    let notice = LearningNotice {
        kind: LearningNoticeKind::Accepted,
        original_nfc: "paht".into(),
        replacement_nfc: "phát".into(),
        positive_delta: 1.0,
        negative_delta: 0.0,
        positive_total: 4.0,
        negative_total: 0.2,
    };
    unsafe {
        overlay.set_presentation(OverlayPresentation::Learning(notice));
    }
    let mut learning_buffer = [0u16; 192];
    let len = unsafe { GetWindowTextW(overlay.hwnd(), &mut learning_buffer) };
    let learning_text =
        String::from_utf16_lossy(&learning_buffer[..usize::try_from(len).unwrap_or(0)]);
    assert!(learning_text.contains("Đã học"));
    assert!(learning_text.contains("+1.0 điểm"));
    let mut learning_rect = RECT::default();
    unsafe { GetWindowRect(overlay.hwnd(), &raw mut learning_rect).expect("learning rect") };
    assert!(
        learning_rect.bottom - learning_rect.top > suggestion_rect.bottom - suggestion_rect.top
    );

    unsafe {
        overlay.hide();
    }
    assert!(!unsafe { IsWindowVisible(overlay.hwnd()) }.as_bool());
}
