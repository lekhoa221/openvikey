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
    Flush(Sender<Result<(), String>>),
    Shutdown,
}

pub struct DebouncedSaver {
    sender: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

impl DebouncedSaver {
    #[must_use]
    pub fn spawn_encrypted(path: impl Into<PathBuf>, provider: PassphraseProvider) -> Self {
        let mut store = FileModelStore::new(path);
        Self::spawn(MODEL_SAVE_DEBOUNCE, move |payload| {
            store
                .save(&payload, &provider)
                .map_err(|error| error.to_string())
        })
    }

    #[must_use]
    pub fn spawn(
        debounce: Duration,
        mut save: impl FnMut(Vec<u8>) -> Result<(), String> + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut pending: Option<Vec<u8>> = None;
            let mut last_error: Option<String> = None;
            loop {
                let message = if pending.is_some() {
                    match receiver.recv_timeout(debounce) {
                        Ok(message) => message,
                        Err(RecvTimeoutError::Timeout) => {
                            if let Some(payload) = pending.take() {
                                last_error = save(payload).err();
                            }
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
                    Message::Submit(payload) => pending = Some(payload),
                    Message::Flush(reply) => {
                        if let Some(payload) = pending.take() {
                            last_error = save(payload).err();
                        }
                        let result = last_error.take().map_or(Ok(()), Err);
                        let _ = reply.send(result);
                    }
                    Message::Shutdown => {
                        if let Some(payload) = pending.take() {
                            let _ = save(payload);
                        }
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

impl Drop for DebouncedSaver {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
