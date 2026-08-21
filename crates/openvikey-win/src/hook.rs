//! WH_KEYBOARD_LL helpers and callback (no key queue).
//!
//! Callback order (synchronous, no key queue channel):
//! 1. `decide` (lock-free policy)
//! 2. if the decision needs session work: `handle_runtime_key` → one host session lock (HWND + key)
//! 3. `ll_return(decision)` — 0 forwards via CallNextHookEx, 1 eats
//!
//! Session acquisition for step 2 lives in `host.rs` (`handle_runtime_key` / `handle_key_locked`), not here.

use std::sync::Mutex;

use crate::host::{TypingHost, handle_key_locked, handle_runtime_key};
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

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

#[cfg(windows)]
fn fill_modifiers(raw: &mut RawKey) -> (bool, bool, bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VIRTUAL_KEY, VK_CAPITAL, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
        VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    };

    let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(vk.0)) } < 0;
    raw.control = down(VK_CONTROL) || down(VK_LCONTROL) || down(VK_RCONTROL);
    raw.shift = down(VK_SHIFT) || down(VK_LSHIFT) || down(VK_RSHIFT);
    raw.left_ctrl = down(VK_LCONTROL);
    raw.left_shift = down(VK_LSHIFT);

    let caps = unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1 != 0;
    let alt = down(VK_MENU) || down(VK_LMENU) || down(VK_RMENU);
    let meta = down(VK_LWIN) || down(VK_RWIN);
    (caps, alt, meta)
}

/// Keyboard LL procedure: lock-free decide, then one host session lock for HWND + key.
///
/// # Safety
///
/// Must be installed via `SetWindowsHookExW(WH_KEYBOARD_LL, ...)`. `lparam` must
/// point to a valid `KBDLLHOOKSTRUCT` when `code == HC_ACTION`.
#[cfg(windows)]
pub unsafe extern "system" fn keyboard_ll_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, HC_ACTION, KBDLLHOOKSTRUCT};

    if code != i32::try_from(HC_ACTION).unwrap_or(0) {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let mut raw = raw_from_ll(
        u16::try_from(kb.vkCode).unwrap_or(0),
        kb.flags.0,
        kb.dwExtraInfo,
    );
    let (caps, alt, meta) = fill_modifiers(&mut raw);
    let at_ms = now_ms();
    let ret = ll_return(&handle_runtime_key(raw, at_ms, caps, alt, meta));
    if ret != 0 {
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

/// Installed `WH_KEYBOARD_LL` hook; unhooks on drop.
#[cfg(windows)]
pub struct KeyboardLlHook {
    hook: windows::Win32::UI::WindowsAndMessaging::HHOOK,
}

#[cfg(windows)]
impl KeyboardLlHook {
    /// Install the low-level keyboard hook.
    ///
    /// # Errors
    ///
    /// Returns a Win32 error when `SetWindowsHookExW` fails.
    pub fn install() -> windows::core::Result<Self> {
        use windows::Win32::Foundation::HINSTANCE;
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_KEYBOARD_LL};
        let hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_ll_proc),
                Some(HINSTANCE::default()),
                0,
            )?
        };
        Ok(Self { hook })
    }
}

#[cfg(windows)]
impl Drop for KeyboardLlHook {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(self.hook);
        }
    }
}
