//! Non-blocking latest-value publisher from TSF callbacks to the host pipe.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_queue::ArrayQueue;
use openvikey_win_context::{
    BridgeMessage, CONTEXT_PIPE_NAME, CONTEXT_PROTOCOL_VERSION, ContextSnapshot, ReadContextResult,
    encode_frame,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE, OPEN_EXISTING, WriteFile,
};
use windows::Win32::System::Pipes::{PIPE_NOWAIT, PIPE_READMODE_MESSAGE, SetNamedPipeHandleState};
use windows::core::{Error, HSTRING, PCWSTR, Result};

use crate::SERVER_STATE;

struct LatestValue {
    message: ArrayQueue<BridgeMessage>,
    wait: Mutex<()>,
    wake: Condvar,
}

impl Default for LatestValue {
    fn default() -> Self {
        Self {
            message: ArrayQueue::new(1),
            wait: Mutex::new(()),
            wake: Condvar::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SnapshotEmitter {
    source_pid: u32,
    source_tid: u32,
    instance_id: u64,
    next_observed: Arc<AtomicU64>,
    latest: Arc<LatestValue>,
}

impl SnapshotEmitter {
    pub(crate) fn publish(&self, context_seq: u64, result: ReadContextResult) {
        let observed_seq = self.next_observed.fetch_add(1, Ordering::Relaxed);
        let message = BridgeMessage::Snapshot(ContextSnapshot {
            protocol_version: CONTEXT_PROTOCOL_VERSION,
            source_pid: self.source_pid,
            source_tid: self.source_tid,
            instance_id: self.instance_id,
            context_seq,
            observed_seq,
            hwnd: result.hwnd,
            state: result.state,
            left_token_nfc: result.left_token_nfc,
        });
        let _replaced = self.latest.message.force_push(message);
        self.latest.wake.notify_one();
    }
}

pub(crate) struct ContextPublisher {
    emitter: SnapshotEmitter,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ContextPublisher {
    pub(crate) fn start(process_id: u32, thread_id: u32, instance_id: u64) -> Result<Self> {
        Self::start_named(CONTEXT_PIPE_NAME, process_id, thread_id, instance_id)
    }

    pub(crate) fn start_named(
        pipe_name: &str,
        process_id: u32,
        thread_id: u32,
        instance_id: u64,
    ) -> Result<Self> {
        let latest = Arc::new(LatestValue::default());
        let stop = Arc::new(AtomicBool::new(false));
        let emitter = SnapshotEmitter {
            source_pid: process_id,
            source_tid: thread_id,
            instance_id,
            next_observed: Arc::new(AtomicU64::new(1)),
            latest: Arc::clone(&latest),
        };
        let worker_stop = Arc::clone(&stop);
        let worker = SERVER_STATE.acquire_worker();
        let pipe_name = pipe_name.to_owned();
        let join = thread::Builder::new()
            .name("openvikey-context-publisher".to_owned())
            .spawn(move || {
                let _worker = worker;
                run_publisher(
                    &pipe_name,
                    process_id,
                    thread_id,
                    instance_id,
                    &latest,
                    &worker_stop,
                );
            })
            .map_err(|error| Error::new(windows::Win32::Foundation::E_FAIL, error.to_string()))?;
        Ok(Self {
            emitter,
            stop,
            worker: Some(join),
        })
    }

    pub(crate) fn emitter(&self) -> SnapshotEmitter {
        self.emitter.clone()
    }

    pub(crate) fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.emitter.latest.wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ContextPublisher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_publisher(
    pipe_name: &str,
    process_id: u32,
    thread_id: u32,
    instance_id: u64,
    latest: &LatestValue,
    stop: &AtomicBool,
) {
    let connect = BridgeMessage::Connect {
        protocol_version: CONTEXT_PROTOCOL_VERSION,
        source_pid: process_id,
        source_tid: thread_id,
        instance_id,
    };
    let disconnect = BridgeMessage::Disconnect {
        protocol_version: CONTEXT_PROTOCOL_VERSION,
        source_pid: process_id,
        source_tid: thread_id,
        instance_id,
    };
    let mut pipe = None;
    let mut last_snapshot = None;
    while !stop.load(Ordering::Acquire) {
        if pipe.is_none()
            && let Ok(opened) = open_pipe(pipe_name)
            && write_message(opened.0, &connect).is_ok()
        {
            if let Some(snapshot) = &last_snapshot {
                let _ = write_message(opened.0, snapshot);
            }
            pipe = Some(opened);
        }

        if let Some(message) = take_latest(latest, Duration::from_millis(50)) {
            last_snapshot = Some(message.clone());
            if let Some(opened) = &pipe
                && write_message(opened.0, &message).is_err()
            {
                pipe = None;
            }
        }
    }
    if let Some(opened) = &pipe {
        let _ = write_message(opened.0, &disconnect);
    }
}

fn take_latest(latest: &LatestValue, timeout: Duration) -> Option<BridgeMessage> {
    if let Some(message) = latest.message.pop() {
        return Some(message);
    }
    let guard = latest.wait.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(message) = latest.message.pop() {
        return Some(message);
    }
    let (_guard, _) = latest
        .wake
        .wait_timeout(guard, timeout)
        .unwrap_or_else(PoisonError::into_inner);
    latest.message.pop()
}

fn open_pipe(pipe_name: &str) -> Result<OwnedPipe> {
    let name = HSTRING::from(pipe_name);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?
    };
    let mode = PIPE_READMODE_MESSAGE | PIPE_NOWAIT;
    unsafe {
        SetNamedPipeHandleState(handle, Some(&raw const mode), None, None)?;
    }
    Ok(OwnedPipe(handle))
}

fn write_message(pipe: HANDLE, message: &BridgeMessage) -> Result<()> {
    let frame = encode_frame(message)
        .map_err(|_| Error::from_hresult(windows::Win32::Foundation::E_INVALIDARG))?;
    let mut written = 0_u32;
    unsafe {
        WriteFile(pipe, Some(&frame), Some(&raw mut written), None)?;
    }
    if usize::try_from(written).ok() == Some(frame.len()) {
        Ok(())
    } else {
        Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL))
    }
}

struct OwnedPipe(HANDLE);

impl Drop for OwnedPipe {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use openvikey_win_context::{BridgeMessage, ContextState, ReadContextResult};

    use super::{LatestValue, SnapshotEmitter, take_latest};

    #[test]
    fn newest_snapshot_replaces_older_without_waiting() {
        let latest = Arc::new(LatestValue::default());
        let emitter = SnapshotEmitter {
            source_pid: 10,
            source_tid: 11,
            instance_id: 12,
            next_observed: Arc::default(),
            latest: Arc::clone(&latest),
        };
        let worker_wait = latest.wait.lock().unwrap();

        emitter.publish(
            1,
            ReadContextResult {
                state: ContextState::Normal,
                left_token_nfc: Some("normal".to_owned()),
                hwnd: Some(13),
            },
        );
        emitter.publish(
            2,
            ReadContextResult {
                state: ContextState::Sensitive,
                left_token_nfc: None,
                hwnd: Some(13),
            },
        );

        let message = latest.message.pop().expect("latest snapshot");
        let BridgeMessage::Snapshot(snapshot) = message else {
            panic!("expected a snapshot");
        };
        assert_eq!(snapshot.context_seq, 2);
        assert_eq!(snapshot.state, ContextState::Sensitive);
        assert!(latest.message.is_empty());
        drop(worker_wait);
        assert!(take_latest(&latest, std::time::Duration::ZERO).is_none());
    }
}
