//! Foreground HWND/exe cache filled by a WinEvent thread; hook path only reads.

use std::sync::{Arc, Mutex, RwLock};

use crate::classify::profile_for_exe;
use crate::inject::InjectProfile;
use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetWindowThreadProcessId, WINEVENT_OUTOFCONTEXT,
};
use windows::core::Result;

struct Snapshot {
    hwnd: isize,
    exe: String,
    profile: InjectProfile,
    generation: u64,
}

/// Lock-free-ish foreground snapshot: winevent thread writes; hook thread `try_read`s.
pub struct FocusCache {
    inner: RwLock<Snapshot>,
}

impl FocusCache {
    /// Empty cache (HWND 0, exe `""`, Win32 profile).
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Snapshot {
                hwnd: 0,
                exe: String::new(),
                profile: InjectProfile::Win32,
                generation: 0,
            }),
        }
    }

    /// Update foreground HWND and executable name (winevent thread).
    pub fn set(&self, hwnd: isize, exe: &str) {
        let profile = profile_for_exe(exe);
        if let Ok(mut guard) = self.inner.write() {
            guard.hwnd = hwnd;
            guard.exe.clear();
            guard.exe.push_str(exe);
            guard.profile = profile;
            guard.generation = guard.generation.wrapping_add(1);
        }
    }

    /// Read the cached HWND and executable file name (blocking read; tests and diagnostics).
    #[must_use]
    pub fn get(&self) -> (isize, String) {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (guard.hwnd, guard.exe.clone())
    }

    /// Non-blocking read for the hook path.
    #[must_use]
    pub fn try_get(&self) -> Option<(isize, String)> {
        let guard = self.inner.try_read().ok()?;
        Some((guard.hwnd, guard.exe.clone()))
    }

    /// Non-blocking read including inject profile for the hook path.
    #[must_use]
    pub fn try_get_profile(&self) -> Option<(isize, String, InjectProfile)> {
        let guard = self.inner.try_read().ok()?;
        Some((guard.hwnd, guard.exe.clone(), guard.profile))
    }

    /// Non-blocking read with the foreground generation for stale-composition invalidation.
    #[must_use]
    pub fn try_get_generation(&self) -> Option<(isize, String, u64)> {
        let guard = self.inner.try_read().ok()?;
        Some((guard.hwnd, guard.exe.clone(), guard.generation))
    }
}

impl Default for FocusCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Seed the cache from the current foreground window (install path; not the key hook).
pub fn seed_current_foreground(cache: &FocusCache) {
    let hwnd = unsafe { GetForegroundWindow() };
    let exe = exe_for_hwnd(hwnd);
    cache.set(hwnd.0 as isize, &exe);
}

static CALLBACK_CACHE: Mutex<Option<Arc<FocusCache>>> = Mutex::new(None);

/// Bind the winevent callback target after a successful hook install.
pub fn bind_callback_cache(cache: Arc<FocusCache>) {
    if let Ok(mut guard) = CALLBACK_CACHE.lock() {
        *guard = Some(cache);
    }
}

/// Clear the winevent callback target on hook uninstall.
pub fn unbind_callback_cache() {
    if let Ok(mut guard) = CALLBACK_CACHE.lock() {
        *guard = None;
    }
}

/// Current winevent callback target (diagnostics and tests).
#[must_use]
pub fn peek_callback_cache() -> Option<Arc<FocusCache>> {
    CALLBACK_CACHE
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(Arc::clone))
}

/// Installed WinEvent hook; unhooks on drop.
pub struct FocusHook {
    hook: HWINEVENTHOOK,
    _cache: Arc<FocusCache>,
}

impl FocusHook {
    /// Install `EVENT_SYSTEM_FOREGROUND` hook that resolves HWND → exe and updates `cache`.
    ///
    /// # Safety
    ///
    /// Calls Win32 hook APIs. Caller must keep `cache` alive for the hook lifetime.
    pub unsafe fn install(cache: Arc<FocusCache>) -> Result<Self> {
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_callback),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            return Err(windows::core::Error::new(E_FAIL, "SetWinEventHook failed"));
        }
        bind_callback_cache(Arc::clone(&cache));
        seed_current_foreground(&cache);
        Ok(Self {
            hook,
            _cache: cache,
        })
    }
}

impl Drop for FocusHook {
    fn drop(&mut self) {
        unbind_callback_cache();
        unsafe {
            let _ = UnhookWinEvent(self.hook);
        }
    }
}

unsafe extern "system" fn foreground_callback(
    _hwineventhook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    if event != EVENT_SYSTEM_FOREGROUND {
        return;
    }
    let Some(cache) = peek_callback_cache() else {
        return;
    };
    let exe = exe_for_hwnd(hwnd);
    cache.set(hwnd.0 as isize, &exe);
    crate::overlay::push_overlay_lines(&[]);
}

fn exe_for_hwnd(hwnd: HWND) -> String {
    if hwnd.is_invalid() {
        return String::new();
    }
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
    }
    if pid == 0 {
        return String::new();
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
    let Ok(process) = process else {
        return String::new();
    };
    let exe = query_process_image_name(process);
    unsafe {
        let _ = CloseHandle(process);
    }
    exe
}

fn query_process_image_name(process: HANDLE) -> String {
    let mut buffer = [0u16; 260];
    let mut size = u32::try_from(buffer.len()).unwrap_or(0);
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &raw mut size,
        )
    };
    if ok.is_err() || size == 0 {
        return String::new();
    }
    let path = String::from_utf16_lossy(&buffer[..size as usize]);
    std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_owned()
}

/// Keyboard + mouse + winevent hooks for the host process.
#[cfg(windows)]
pub struct HostHooks {
    _keyboard: crate::hook::KeyboardLlHook,
    _mouse: crate::mouse::MouseLlHook,
    _focus: FocusHook,
}

#[cfg(windows)]
impl HostHooks {
    /// Install WH_KEYBOARD_LL, WH_MOUSE_LL, and foreground winevent hooks.
    ///
    /// # Errors
    ///
    /// Returns a Win32 error when any install fails.
    pub fn install(cache: Arc<FocusCache>) -> Result<Self> {
        let keyboard = crate::hook::KeyboardLlHook::install()?;
        let mouse = crate::mouse::MouseLlHook::install()?;
        let focus = unsafe { FocusHook::install(cache)? };
        Ok(Self {
            _keyboard: keyboard,
            _mouse: mouse,
            _focus: focus,
        })
    }
}

/// Peek/dispatch host message loop until [`crate::persist::HostShutdown`] is requested.
pub fn run_host_message_loop(shutdown: &crate::persist::HostShutdown) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
        };
        while !shutdown.is_requested() {
            let mut msg = MSG::default();
            let has_msg = unsafe { PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE) };
            if has_msg.as_bool() {
                if msg.message == WM_QUIT {
                    shutdown.run();
                    break;
                }
                unsafe {
                    let _ = TranslateMessage(&raw const msg);
                    DispatchMessageW(&raw const msg);
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = shutdown;
    }
}
