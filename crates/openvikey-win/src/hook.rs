//! WH_KEYBOARD_LL return helpers (no key queue).
//!
//! Callback order (synchronous, no key queue channel):
//! 1. `decide` (lock-free policy)
//! 2. if the decision needs session work: `handle_key_locked` → `host.handle_key` (SendInput)
//! 3. `ll_return(decision)` — 0 forwards via CallNextHookEx, 1 eats
//!
//! Session acquisition for step 2 lives in `host.rs` (`handle_key_locked`), not here.

use std::sync::Mutex;

use crate::host::{handle_key_locked, TypingHost};
use crate::policy::{KeyDecision, RawKey};

/// `LLKHF_UP` — transition state is key-up when set.
const LLKHF_UP: u32 = 0x0080;

/// Map a decision to the LL hook return value.
///
/// `0` = CallNextHookEx (Pass / CommitAndPass / CaretBreakAndPass).
/// `1` = eat (Eat* / Hotkey).
#[must_use]
pub fn ll_return(decision: &KeyDecision) -> isize {
    // 0 = CallNextHookEx, 1 = eat
    match decision {
        KeyDecision::Pass | KeyDecision::CommitAndPass { .. } | KeyDecision::CaretBreakAndPass => 0,
        _ => 1,
    }
}

/// Testable LL path: `handle_key_locked` then [`ll_return`] (no live hook).
#[must_use]
pub fn dispatch_ll(host: &Mutex<TypingHost>, raw: RawKey, at_ms: i64) -> isize {
    ll_return(&handle_key_locked(host, raw, at_ms))
}

/// Build [`RawKey`] from LL keyboard fields (`vkCode`, `flags`, `dwExtraInfo`).
///
/// Modifier bits are left false here; the real callback fills them from key state.
#[must_use]
pub fn raw_from_ll(vk: u16, flags: u32, extra_info: usize) -> RawKey {
    RawKey {
        vk,
        down: flags & LLKHF_UP == 0,
        control: false,
        shift: false,
        extra_info,
        left_ctrl: false,
        left_shift: false,
    }
}
