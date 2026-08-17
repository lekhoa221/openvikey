//! M10 persistence work stays off the typing thread and coalesces bursts.

use openvikey_lab::persistence::{DebouncedSaver, MODEL_SAVE_DEBOUNCE};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn burst_is_coalesced_and_saved_on_worker_thread() {
    assert_eq!(MODEL_SAVE_DEBOUNCE, Duration::from_secs(2));
    let caller_thread = thread::current().id();
    let (observed_tx, observed_rx) = mpsc::channel();
    let saver = DebouncedSaver::spawn(Duration::from_secs(2), move |payload| {
        observed_tx
            .send((payload, thread::current().id()))
            .map_err(|error| error.to_string())
    });

    saver.submit(b"model-v1".to_vec()).unwrap();
    saver.submit(b"model-v2".to_vec()).unwrap();
    saver.submit(b"model-v3".to_vec()).unwrap();
    saver.flush().unwrap();

    let (payload, worker_thread) = observed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(payload, b"model-v3");
    assert_ne!(worker_thread, caller_thread);
    assert!(
        observed_rx.try_recv().is_err(),
        "burst must produce one write"
    );
}
