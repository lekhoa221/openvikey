//! WH_MOUSE_LL decision helpers and callback (no lock or blocking wait on the hook path).

use crate::host::caret_break_runtime_locked;
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

#[cfg(windows)]
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
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

    if code == i32::try_from(HC_ACTION).unwrap_or(0) {
        let message = u32::try_from(wparam.0).unwrap_or(0);
        let decision = mouse_decision(message);
        if matches!(decision, KeyDecision::CaretBreakAndPass) {
            let at_ms = now_ms();
            let (caps, alt, meta) = read_host_modifier_state();
            caret_break_runtime_locked(at_ms, caps, alt, meta);
        }
        let _ = mouse_return(&decision);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Installed `WH_MOUSE_LL` hook; unhooks on drop.
#[cfg(windows)]
pub struct MouseLlHook {
    hook: windows::Win32::UI::WindowsAndMessaging::HHOOK,
}

#[cfg(windows)]
impl MouseLlHook {
    /// Install the low-level mouse hook.
    ///
    /// # Errors
    ///
    /// Returns a Win32 error when `SetWindowsHookExW` fails.
    pub fn install() -> windows::core::Result<Self> {
        use windows::Win32::Foundation::HINSTANCE;
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_MOUSE_LL};
        let hook = unsafe {
            SetWindowsHookExW(
                WH_MOUSE_LL,
                Some(mouse_ll_proc),
                Some(HINSTANCE::default()),
                0,
            )?
        };
        Ok(Self { hook })
    }
}

#[cfg(windows)]
impl Drop for MouseLlHook {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(self.hook);
        }
    }
}
