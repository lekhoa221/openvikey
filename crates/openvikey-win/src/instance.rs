//! Per-session single-instance guard for the standalone host.

#[cfg(windows)]
mod win32 {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use windows::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, WAIT_OBJECT_0,
        WPARAM,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, SetEvent, WaitForSingleObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
    use windows::core::{PCWSTR, Result, w};

    const REQUEST_EVENT: windows::core::PCWSTR = w!("Local\\OpenViKey.Standalone.OpenSettings.v1");

    struct SendHandle(HANDLE);
    // Kernel handles remain valid while ownership is retained by SingleInstance.
    unsafe impl Send for SendHandle {}

    pub struct SingleInstance {
        mutex: HANDLE,
        request: HANDLE,
        stop: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    impl SingleInstance {
        /// Acquire the standalone instance. `None` means an existing host was notified.
        pub fn acquire() -> Result<Option<Self>> {
            let mutex = unsafe {
                CreateMutexW(None, false, w!("Local\\OpenViKey.Standalone.Singleton.v1"))?
            };
            let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            if already_running {
                unsafe {
                    let _ = CloseHandle(mutex);
                }
                if let Ok(event) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, REQUEST_EVENT) } {
                    unsafe {
                        let _ = SetEvent(event);
                        let _ = CloseHandle(event);
                    }
                }
                return Ok(None);
            }

            let request = unsafe { CreateEventW(None, false, false, REQUEST_EVENT)? };
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let send_request = SendHandle(request);
            let worker = thread::Builder::new()
                .name("openvikey-single-instance".to_owned())
                .spawn(move || request_worker(&send_request, &worker_stop))
                .map_err(|error| {
                    windows::core::Error::new(windows::Win32::Foundation::E_FAIL, error.to_string())
                })?;
            Ok(Some(Self {
                mutex,
                request,
                stop,
                worker: Some(worker),
            }))
        }
    }

    impl Drop for SingleInstance {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            unsafe {
                let _ = CloseHandle(self.request);
                let _ = CloseHandle(self.mutex);
            }
        }
    }

    fn request_worker(request: &SendHandle, stop: &AtomicBool) {
        while !stop.load(Ordering::Acquire) {
            if unsafe { WaitForSingleObject(request.0, 50) } == WAIT_OBJECT_0 {
                notify_ui_thread();
            }
        }
    }

    fn notify_ui_thread() {
        for _ in 0..20 {
            let sink = unsafe { FindWindowW(w!("OpenViKeyTraySink"), PCWSTR::null()) };
            if let Ok(hwnd) = sink
                && hwnd != HWND::default()
            {
                unsafe {
                    let _ = PostMessageW(
                        Some(hwnd),
                        crate::tray::WM_OPEN_SETTINGS,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

#[cfg(windows)]
pub use win32::SingleInstance;
