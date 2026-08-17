//! Background debounce worker for encrypted model saves.

use openvikey_core::store::ModelStore;
use openvikey_core::store::file::FileModelStore;
use openvikey_core::store::passphrase::PassphraseProvider;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use thiserror::Error;

pub const MODEL_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum SaverError {
    #[error("save worker is unavailable")]
    WorkerUnavailable,
    #[error("save failed: {0}")]
    Save(String),
}

enum Message {
    Submit(Vec<u8>),
    Notify,
    Flush(Sender<Result<(), String>>),
    Shutdown,
}

enum Pending {
    None,
    Bytes(Vec<u8>),
    Dirty,
}

type SnapshotFn = Box<dyn FnMut() -> Result<Vec<u8>, String> + Send>;

pub struct DebouncedSaver {
    sender: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

impl DebouncedSaver {
    #[must_use]
    pub fn spawn_encrypted(
        path: impl Into<PathBuf>,
        provider: PassphraseProvider,
        snapshot: impl FnMut() -> Result<Vec<u8>, String> + Send + 'static,
    ) -> Self {
        let mut store = FileModelStore::new(path);
        Self::spawn_lazy(MODEL_SAVE_DEBOUNCE, snapshot, move |payload| {
            store
                .save(&payload, &provider)
                .map_err(|error| error.to_string())
        })
    }

    #[must_use]
    pub fn spawn(
        debounce: Duration,
        save: impl FnMut(Vec<u8>) -> Result<(), String> + Send + 'static,
    ) -> Self {
        Self::spawn_with(debounce, None, save)
    }

    #[must_use]
    pub fn spawn_lazy(
        debounce: Duration,
        snapshot: impl FnMut() -> Result<Vec<u8>, String> + Send + 'static,
        save: impl FnMut(Vec<u8>) -> Result<(), String> + Send + 'static,
    ) -> Self {
        Self::spawn_with(debounce, Some(Box::new(snapshot)), save)
    }

    fn spawn_with(
        debounce: Duration,
        mut snapshot: Option<SnapshotFn>,
        mut save: impl FnMut(Vec<u8>) -> Result<(), String> + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut pending = Pending::None;
            let mut last_error: Option<String> = None;
            loop {
                let waiting = !matches!(pending, Pending::None);
                let message = if waiting {
                    match receiver.recv_timeout(debounce) {
                        Ok(message) => message,
                        Err(RecvTimeoutError::Timeout) => {
                            last_error =
                                persist_pending(&mut pending, snapshot.as_mut(), &mut save);
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    match receiver.recv() {
                        Ok(message) => message,
                        Err(_) => break,
                    }
                };
                match message {
                    Message::Submit(payload) => pending = Pending::Bytes(payload),
                    Message::Notify => pending = Pending::Dirty,
                    Message::Flush(reply) => {
                        if !matches!(pending, Pending::None) {
                            last_error =
                                persist_pending(&mut pending, snapshot.as_mut(), &mut save);
                        }
                        let result = last_error.take().map_or(Ok(()), Err);
                        let _ = reply.send(result);
                    }
                    Message::Shutdown => {
                        let _ = persist_pending(&mut pending, snapshot.as_mut(), &mut save);
                        break;
                    }
                }
            }
        });
        Self {
            sender,
            worker: Some(worker),
        }
    }

    pub fn submit(&self, payload: Vec<u8>) -> Result<(), SaverError> {
        self.sender
            .send(Message::Submit(payload))
            .map_err(|_| SaverError::WorkerUnavailable)
    }

    pub fn notify(&self) -> Result<(), SaverError> {
        self.sender
            .send(Message::Notify)
            .map_err(|_| SaverError::WorkerUnavailable)
    }

    pub fn flush(&self) -> Result<(), SaverError> {
        let (reply, result) = mpsc::channel();
        self.sender
            .send(Message::Flush(reply))
            .map_err(|_| SaverError::WorkerUnavailable)?;
        result
            .recv()
            .map_err(|_| SaverError::WorkerUnavailable)?
            .map_err(SaverError::Save)
    }
}

fn persist_pending(
    pending: &mut Pending,
    snapshot: Option<&mut SnapshotFn>,
    save: &mut impl FnMut(Vec<u8>) -> Result<(), String>,
) -> Option<String> {
    let payload = match std::mem::replace(pending, Pending::None) {
        Pending::None => return None,
        Pending::Bytes(bytes) => bytes,
        Pending::Dirty => match snapshot {
            Some(snapshot) => match snapshot() {
                Ok(bytes) => bytes,
                Err(error) => return Some(error),
            },
            None => return Some("lazy snapshot is missing".to_string()),
        },
    };
    save(payload).err()
}

impl Drop for DebouncedSaver {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
