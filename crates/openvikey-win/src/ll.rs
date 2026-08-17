//! WH_KEYBOARD_LL / WH_MOUSE_LL callbacks bound to the typing host.

use std::sync::{Arc, Mutex, OnceLock};

use crate::focus::FocusCache;
use crate::hook::ll_return;
use crate::hook::raw_from_ll;
use crate::host::{on_try_lock_fail, TypingHost};
use crate::mouse::{mouse_decision, mouse_return};
use crate::policy::{KeyDecision, RawKey};

struct Runtime {
    host: Arc<Mutex<TypingHost>>,
    focus: Arc<FocusCache>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Bind host + focus for LL callbacks (call once before installing hooks).
pub fn bind_runtime(host: Arc<Mutex<TypingHost>>, focus: Arc<FocusCache>) {
    let _ = RUNTIME.set(Runtime { host, focus });
}

#[cfg(windows)]
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

#[cfg(windows)]
fn fill_modifiers(raw: &mut RawKey) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LSHIFT, VK_RCONTROL, VK_RSHIFT,
        VK_SHIFT,
    };

    let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(vk.0)) } < 0;
    raw.control = down(VK_CONTROL) || down(VK_LCONTROL) || down(VK_RCONTROL);
    raw.shift = down(VK_SHIFT) || down(VK_LSHIFT) || down(VK_RSHIFT);
    raw.left_ctrl = down(VK_LCONTROL);
    raw.left_shift = down(VK_LSHIFT);
}

#[cfg(windows)]
fn read_host_modifier_state() -> (bool, bool, bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VIRTUAL_KEY, VK_CAPITAL, VK_LMENU, VK_LWIN, VK_MENU, VK_RMENU, VK_RWIN,
    };
    let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(vk.0)) } < 0;
    let caps = unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1 != 0;
    let alt = down(VK_MENU) || down(VK_LMENU) || down(VK_RMENU);
    let meta = down(VK_LWIN) || down(VK_RWIN);
    (caps, alt, meta)
}

#[cfg(windows)]
fn sync_focus_and_modifiers(host: &mut TypingHost, focus: &FocusCache, at_ms: i64) {
    if let Some((hwnd, exe)) = focus.try_get() {
        host.set_hwnd(u64::try_from(hwnd).unwrap_or(0), exe, at_ms);
    }
    let (caps, alt, meta) = read_host_modifier_state();
    host.caps_lock = caps;
    host.alt = alt;
    host.meta = meta;
}

/// Keyboard LL procedure: try_lock host, inject, eat or forward per [`ll_return`].
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
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, KBDLLHOOKSTRUCT, HC_ACTION};

    if code != i32::try_from(HC_ACTION).unwrap_or(0) {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let Some(rt) = RUNTIME.get() else {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let mut raw = raw_from_ll(
        u16::try_from(kb.vkCode).unwrap_or(0),
        kb.flags.0,
        kb.dwExtraInfo,
    );
    fill_modifiers(&mut raw);
    let at_ms = now_ms();

    let ret = match rt.host.try_lock() {
        Ok(mut guard) => {
            sync_focus_and_modifiers(&mut guard, &rt.focus, at_ms);
            ll_return(&guard.handle_key(raw, at_ms))
        }
        Err(_) => ll_return(&on_try_lock_fail(&raw)),
    };

    if ret != 0 {
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

/// Mouse LL procedure: caret-break on button down; never eats clicks.
///
/// # Safety
///
/// Must be installed via `SetWindowsHookExW(WH_MOUSE_LL, ...)`.
#[cfg(windows)]
pub unsafe extern "system" fn mouse_ll_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, HC_ACTION};

    if code == i32::try_from(HC_ACTION).unwrap_or(0)
        && let Some(rt) = RUNTIME.get()
    {
        let message = u32::try_from(wparam.0).unwrap_or(0);
        let decision = mouse_decision(message);
        if matches!(decision, KeyDecision::CaretBreakAndPass) {
            let at_ms = now_ms();
            if let Ok(mut guard) = rt.host.try_lock() {
                sync_focus_and_modifiers(&mut guard, &rt.focus, at_ms);
                guard.notify_caret_break(at_ms);
            }
        }
        let _ = mouse_return(&decision);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
