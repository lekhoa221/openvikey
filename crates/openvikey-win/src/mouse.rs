//! WH_MOUSE_LL decision helpers (no lock or blocking wait on the hook path).

use crate::policy::KeyDecision;

/// `WM_LBUTTONDOWN`
const WM_LBUTTONDOWN: u32 = 0x0201;
/// `WM_RBUTTONDOWN`
const WM_RBUTTONDOWN: u32 = 0x0204;
/// `WM_MBUTTONDOWN`
const WM_MBUTTONDOWN: u32 = 0x0207;

/// Map a low-level mouse message to a host decision.
///
/// Button downs trigger caret-break session work; all other messages pass through.
#[must_use]
pub fn mouse_decision(message: u32) -> KeyDecision {
    match message {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => KeyDecision::CaretBreakAndPass,
        _ => KeyDecision::Pass,
    }
}

/// Map a mouse decision to the LL hook return value.
///
/// Mouse never eats clicks — always `CallNextHookEx` (0).
#[must_use]
pub fn mouse_return(_decision: &KeyDecision) -> isize {
    0
}
