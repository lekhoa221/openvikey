//! Foreground HWND/exe cache filled by a WinEvent thread; hook path only reads.

use std::sync::{Arc, OnceLock, RwLock};

use crate::classify::profile_for_exe;
use crate::inject::InjectProfile;
use windows::core::Result;
use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT,
};

struct Snapshot {
    hwnd: isize,
    exe: String,
    profile: InjectProfile,
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
}

impl Default for FocusCache {
    fn default() -> Self {
        Self::new()
    }
}

static FOREGROUND_CACHE: OnceLock<Arc<FocusCache>> = OnceLock::new();

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
        let _ = FOREGROUND_CACHE.set(Arc::clone(&cache));
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
        Ok(Self {
            hook,
            _cache: cache,
        })
    }
}

impl Drop for FocusHook {
    fn drop(&mut self) {
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
    let Some(cache) = FOREGROUND_CACHE.get() else {
        return;
    };
    let exe = exe_for_hwnd(hwnd);
    cache.set(hwnd.0 as isize, &exe);
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
