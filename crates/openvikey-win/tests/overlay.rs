//! Overlay candidate line cap (display-only helper).

use openvikey_win::overlay::overlay_lines;

#[test]
fn overlay_caps_at_three() {
    let lines = overlay_lines(&["a".into(), "b".into(), "c".into(), "d".into()], 3);
    assert_eq!(lines, ["a", "b", "c"]);
}
