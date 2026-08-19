//! Host-side application of validated TSF bridge messages.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use openvikey_win_context::{
    BridgeMessage, CONTEXT_PIPE_NAME, CONTEXT_PROTOCOL_VERSION, ContextCache, ContextCacheError,
    ForegroundIdentity, decode_frame,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING,
    HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{PIPE_ACCESS_INBOUND, ReadFile};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_NOWAIT, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES,
};
use windows::core::{Error, HRESULT, HSTRING, PCWSTR, Result, w};

use crate::focus::FocusCache;
use crate::host::ContextProjectionSlot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextBridgeStateError {
    UnsupportedProtocol,
    Cache(ContextCacheError),
}

/// Running host-side named-pipe listener. Dropping it stops and joins all workers.
pub struct ContextBridgeServer {
    stop: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
}

impl ContextBridgeServer {
    pub fn start(focus: Arc<FocusCache>, projection: Arc<ContextProjectionSlot>) -> Result<Self> {
        Self::start_named(CONTEXT_PIPE_NAME, focus, projection)
    }

    /// Start a server at an explicit endpoint (used by isolated integration tests).
    pub fn start_named(
        pipe_name: &str,
        focus: Arc<FocusCache>,
        projection: Arc<ContextProjectionSlot>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let pipe_name = pipe_name.to_owned();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let listener = thread::Builder::new()
            .name("openvikey-context-listener".to_owned())
            .spawn(move || {
                run_listener(&pipe_name, &focus, projection, &worker_stop, &ready_tx);
            })
            .map_err(|error| Error::new(windows::Win32::Foundation::E_FAIL, error.to_string()))?;
        ready_rx
            .recv()
            .map_err(|_| Error::from_hresult(windows::Win32::Foundation::E_FAIL))??;
        Ok(Self {
            stop,
            listener: Some(listener),
        })
    }
}

impl Drop for ContextBridgeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

fn run_listener(
    pipe_name: &str,
    focus: &FocusCache,
    projection: Arc<ContextProjectionSlot>,
    stop: &Arc<AtomicBool>,
    ready: &mpsc::SyncSender<Result<()>>,
) {
    let security = match PipeSecurity::new() {
        Ok(security) => security,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut listener = match create_pipe(pipe_name, &security.attributes) {
        Ok(pipe) => Some(pipe),
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));

    let state = Arc::new(Mutex::new(ContextBridgeState::new(projection)));
    let mut clients = Vec::new();
    while !stop.load(Ordering::Acquire) {
        sync_focus(&state, focus);
        let Some(active_listener) = listener.as_ref() else {
            break;
        };
        match try_connect(active_listener.0) {
            Ok(true) => {
                let connected = listener.take().expect("listener exists");
                let client_state = Arc::clone(&state);
                let client_stop = Arc::clone(stop);
                clients.push(thread::spawn(move || {
                    run_client(connected, &client_state, &client_stop);
                }));
                match create_pipe(pipe_name, &security.attributes) {
                    Ok(next) => listener = Some(next),
                    Err(_) => break,
                }
            }
            Ok(false) => thread::sleep(Duration::from_millis(10)),
            Err(_) => match create_pipe(pipe_name, &security.attributes) {
                Ok(next) => listener = Some(next),
                Err(_) => break,
            },
        }
    }
    drop(listener);
    for client in clients {
        let _ = client.join();
    }
}

fn sync_focus(state: &Mutex<ContextBridgeState>, focus: &FocusCache) {
    if let Some(foreground) = focus.try_get_identity() {
        state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .sync_focus(foreground);
    }
}

#[allow(clippy::needless_pass_by_value)] // Owns and closes the kernel handle on return.
fn run_client(pipe: OwnedPipe, state: &Mutex<ContextBridgeState>, stop: &AtomicBool) {
    let mut actual_pid = 0_u32;
    if unsafe { GetNamedPipeClientProcessId(pipe.0, &raw mut actual_pid) }.is_err()
        || actual_pid == 0
    {
        return;
    }
    let mut buffer = [0_u8; 4100];
    let mut connected_source = None;
    while !stop.load(Ordering::Acquire) {
        let mut read = 0_u32;
        match unsafe { ReadFile(pipe.0, Some(&mut buffer), Some(&raw mut read), None) } {
            Ok(()) if read == 0 => break,
            Ok(()) => {
                let read = usize::try_from(read)
                    .unwrap_or(buffer.len())
                    .min(buffer.len());
                let Ok(message) = decode_frame(&buffer[..read]) else {
                    break;
                };
                if !message_matches_process(&message, actual_pid) {
                    break;
                }
                match &message {
                    BridgeMessage::Connect {
                        source_tid,
                        instance_id,
                        ..
                    } => connected_source = Some((*source_tid, *instance_id)),
                    BridgeMessage::Disconnect { .. } => connected_source = None,
                    BridgeMessage::Snapshot(_) => {}
                }
                if state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .apply(message)
                    .is_err()
                {
                    break;
                }
            }
            Err(error) if error_is(error.code(), ERROR_NO_DATA.0) => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error_is(error.code(), ERROR_BROKEN_PIPE.0) => break,
            Err(_) => break,
        }
    }
    if let Some((thread_id, instance_id)) = connected_source {
        let _ =
            state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .apply(BridgeMessage::Disconnect {
                    protocol_version: CONTEXT_PROTOCOL_VERSION,
                    source_pid: actual_pid,
                    source_tid: thread_id,
                    instance_id,
                });
    }
    unsafe {
        let _ = DisconnectNamedPipe(pipe.0);
    }
}

fn message_matches_process(message: &BridgeMessage, actual_pid: u32) -> bool {
    match message {
        BridgeMessage::Connect { source_pid, .. }
        | BridgeMessage::Disconnect { source_pid, .. } => *source_pid == actual_pid,
        BridgeMessage::Snapshot(snapshot) => snapshot.source_pid == actual_pid,
    }
}

fn try_connect(pipe: HANDLE) -> Result<bool> {
    match unsafe { ConnectNamedPipe(pipe, None) } {
        Ok(()) => Ok(true),
        Err(error) if error_is(error.code(), ERROR_PIPE_CONNECTED.0) => Ok(true),
        Err(error) if error_is(error.code(), ERROR_PIPE_LISTENING.0) => Ok(false),
        Err(error) if error_is(error.code(), ERROR_NO_DATA.0) => Ok(false),
        Err(error) => Err(error),
    }
}

fn error_is(actual: HRESULT, win32: u32) -> bool {
    actual == HRESULT::from_win32(win32)
}

fn create_pipe(name: &str, security: &SECURITY_ATTRIBUTES) -> Result<OwnedPipe> {
    let name = HSTRING::from(name);
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_INBOUND,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_NOWAIT,
            PIPE_UNLIMITED_INSTANCES,
            4100,
            4100,
            50,
            Some(security),
        )
    };
    if handle.is_invalid() {
        Err(Error::from_thread())
    } else {
        Ok(OwnedPipe(handle))
    }
}

struct OwnedPipe(HANDLE);

// A kernel handle remains valid when ownership moves to another process thread.
unsafe impl Send for OwnedPipe {}

impl Drop for OwnedPipe {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

impl PipeSecurity {
    fn new() -> Result<Self> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w!("D:P(A;;GA;;;SY)(A;;GA;;;OW)"),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )?;
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        Ok(Self {
            descriptor,
            attributes,
        })
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        if !self.descriptor.0.is_null() {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
            }
        }
    }
}

/// Stateful bridge reducer kept off the low-level keyboard-hook callback.
pub struct ContextBridgeState {
    cache: ContextCache,
    foreground: Option<ForegroundIdentity>,
    projection: Arc<ContextProjectionSlot>,
}

impl ContextBridgeState {
    #[must_use]
    pub fn new(projection: Arc<ContextProjectionSlot>) -> Self {
        Self {
            cache: ContextCache::new(),
            foreground: None,
            projection,
        }
    }

    /// Anchor all later observations to a newly sampled foreground generation.
    pub fn sync_focus(&mut self, foreground: ForegroundIdentity) {
        if self.foreground == Some(foreground) {
            return;
        }
        self.cache.focus_changed(foreground);
        self.foreground = Some(foreground);
        self.publish_projection();
    }

    /// Apply one already-decoded bridge message and publish the resulting projection.
    pub fn apply(
        &mut self,
        message: BridgeMessage,
    ) -> std::result::Result<(), ContextBridgeStateError> {
        match message {
            BridgeMessage::Connect {
                protocol_version,
                source_pid,
                source_tid,
                instance_id,
            } => {
                if protocol_version != CONTEXT_PROTOCOL_VERSION {
                    return Err(ContextBridgeStateError::UnsupportedProtocol);
                }
                self.cache
                    .connect(source_pid, source_tid, instance_id)
                    .map_err(ContextBridgeStateError::Cache)?;
                if let Some(foreground) = self.foreground
                    && foreground.pid == source_pid
                    && foreground.tid == source_tid
                {
                    self.cache.focus_changed(foreground);
                }
            }
            BridgeMessage::Snapshot(snapshot) => self
                .cache
                .ingest(snapshot)
                .map_err(ContextBridgeStateError::Cache)?,
            BridgeMessage::Disconnect {
                protocol_version,
                source_pid,
                source_tid,
                instance_id,
            } => {
                if protocol_version != CONTEXT_PROTOCOL_VERSION {
                    return Err(ContextBridgeStateError::UnsupportedProtocol);
                }
                self.cache.disconnect(source_pid, source_tid, instance_id);
            }
        }
        self.publish_projection();
        Ok(())
    }

    fn publish_projection(&self) {
        if let Some(foreground) = &self.foreground {
            self.projection.publish(self.cache.project(foreground));
        }
    }
}
