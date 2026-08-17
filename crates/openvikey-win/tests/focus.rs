//! Focus cache and mouse caret-break decisions.

use openvikey_win::focus::FocusCache;
use openvikey_win::mouse::mouse_decision;
use openvikey_win::policy::KeyDecision;

#[test]
fn focus_cache_roundtrip() {
    let cache = FocusCache::new();
    cache.set(1, "notepad.exe");
    assert_eq!(cache.get(), (1, "notepad.exe".into()));
}

#[test]
fn mouse_lbutton_is_caret_break() {
    assert_eq!(mouse_decision(0x0201), KeyDecision::CaretBreakAndPass); // WM_LBUTTONDOWN
}
