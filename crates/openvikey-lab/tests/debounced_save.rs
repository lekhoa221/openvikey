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

#[test]
fn lazy_snapshot_runs_on_worker_and_coalesces_notifies() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let caller_thread = thread::current().id();
    let snapshots = Arc::new(AtomicUsize::new(0));
    let (thread_tx, thread_rx) = mpsc::channel();
    let (save_tx, save_rx) = mpsc::channel();
    let saver = DebouncedSaver::spawn_lazy(
        Duration::from_millis(50),
        {
            let snapshots = Arc::clone(&snapshots);
            let thread_tx = thread_tx.clone();
            move || {
                snapshots.fetch_add(1, Ordering::SeqCst);
                let _ = thread_tx.send(thread::current().id());
                Ok(b"late-serialize".to_vec())
            }
        },
        move |payload| save_tx.send(payload).map_err(|error| error.to_string()),
    );
    saver.notify().unwrap();
    saver.notify().unwrap();
    saver.notify().unwrap();
    saver.flush().unwrap();
    assert_eq!(snapshots.load(Ordering::SeqCst), 1);
    let snapshot_thread = thread_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_ne!(snapshot_thread, caller_thread);
    assert_eq!(
        save_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        b"late-serialize"
    );
}

#[test]
fn failed_task_stays_pending_until_flush_retries_it() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let saver = DebouncedSaver::spawn_task(Duration::from_millis(10), {
        let calls = Arc::clone(&calls);
        move || {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                Err("one-shot failure".to_string())
            } else {
                Ok(())
            }
        }
    });
    saver.notify().unwrap();
    for _ in 0..100 {
        if calls.load(Ordering::SeqCst) >= 1 {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    saver.flush().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
