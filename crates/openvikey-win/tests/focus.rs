//! Focus cache and mouse caret-break decisions.

use std::sync::{Arc, Mutex};

use openvikey_win::focus::{
    bind_callback_cache, peek_callback_cache, unbind_callback_cache, FocusCache,
};
use openvikey_win::mouse::mouse_decision;
use openvikey_win::policy::KeyDecision;

static CALLBACK_SLOT_TEST_LOCK: Mutex<()> = Mutex::new(());

struct CallbackSlotGuard;

impl CallbackSlotGuard {
    fn clear() -> Self {
        unbind_callback_cache();
        Self
    }
}

impl Drop for CallbackSlotGuard {
    fn drop(&mut self) {
        unbind_callback_cache();
    }
}

#[test]
fn callback_cache_stays_unbound_until_explicit_bind() {
    let _slot = CALLBACK_SLOT_TEST_LOCK.lock().unwrap();
    let _guard = CallbackSlotGuard::clear();
    assert!(peek_callback_cache().is_none());
}

#[test]
fn callback_cache_rebinds_after_unbind() {
    let _slot = CALLBACK_SLOT_TEST_LOCK.lock().unwrap();

    let first = Arc::new(FocusCache::new());
    first.set(1, "first.exe");
    bind_callback_cache(Arc::clone(&first));
    assert!(Arc::ptr_eq(
        &peek_callback_cache().expect("bound"),
        &first
    ));

    unbind_callback_cache();
    assert!(peek_callback_cache().is_none());

    let second = Arc::new(FocusCache::new());
    second.set(2, "second.exe");
    bind_callback_cache(Arc::clone(&second));
    assert!(Arc::ptr_eq(
        &peek_callback_cache().expect("rebound"),
        &second
    ));
    assert_eq!(first.get(), (1, "first.exe".into()));
    assert_eq!(second.get(), (2, "second.exe".into()));
}

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
