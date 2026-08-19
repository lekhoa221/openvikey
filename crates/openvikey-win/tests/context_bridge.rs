use std::sync::Arc;
use std::time::{Duration, Instant};

use openvikey_win::context_bridge::{ContextBridgeServer, ContextBridgeState};
use openvikey_win::focus::FocusCache;
use openvikey_win::host::ContextProjectionSlot;
use openvikey_win_context::{
    BridgeMessage, CONTEXT_PROTOCOL_VERSION, ContextProjection, ContextSnapshot, ContextState,
    ForegroundIdentity, encode_frame,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE, OPEN_EXISTING, WriteFile,
};
use windows::Win32::System::Pipes::{PIPE_NOWAIT, PIPE_READMODE_MESSAGE, SetNamedPipeHandleState};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::core::{HSTRING, PCWSTR};

fn foreground(generation: u64) -> ForegroundIdentity {
    ForegroundIdentity {
        pid: 10,
        tid: 11,
        hwnd: Some(15),
        generation,
    }
}

fn snapshot(observed_seq: u64) -> ContextSnapshot {
    ContextSnapshot {
        protocol_version: CONTEXT_PROTOCOL_VERSION,
        source_pid: 10,
        source_tid: 11,
        instance_id: 12,
        context_seq: 13,
        observed_seq,
        hwnd: Some(15),
        state: ContextState::Normal,
        left_token_nfc: Some("chào".to_owned()),
    }
}

#[test]
fn field_focus_invalidation_blocks_the_same_foreground_until_reclassified() {
    let slot = ContextProjectionSlot::new();
    let active = foreground(1);
    slot.publish(
        active,
        ContextProjection::Normal {
            left_token_nfc: None,
        },
    );

    slot.invalidate(active);

    assert_eq!(
        slot.try_projection_for(&active),
        Some(ContextProjection::Pending)
    );
}

#[test]
fn projection_never_crosses_a_foreground_generation() {
    let slot = ContextProjectionSlot::new();
    let first = foreground(1);
    let second = foreground(2);
    slot.publish(
        first,
        ContextProjection::Normal {
            left_token_nfc: Some("chào".to_owned()),
        },
    );

    assert!(matches!(
        slot.try_projection_for(&first),
        Some(ContextProjection::Normal { .. })
    ));
    assert_eq!(slot.try_projection_for(&second), None);
}

#[test]
fn bridge_reducer_publishes_pending_normal_and_invalidates_on_focus() {
    let projection = Arc::new(ContextProjectionSlot::new());
    let mut bridge = ContextBridgeState::new(Arc::clone(&projection));
    bridge.sync_focus(foreground(1));
    assert_eq!(
        projection.try_projection(),
        Some(ContextProjection::Unsupported)
    );

    bridge
        .apply(BridgeMessage::Connect {
            protocol_version: CONTEXT_PROTOCOL_VERSION,
            source_pid: 10,
            source_tid: 11,
            instance_id: 12,
        })
        .unwrap();
    assert_eq!(
        projection.try_projection(),
        Some(ContextProjection::Pending)
    );

    bridge.apply(BridgeMessage::Snapshot(snapshot(14))).unwrap();
    assert_eq!(
        projection.try_projection(),
        Some(ContextProjection::Normal {
            left_token_nfc: Some("chào".to_owned())
        })
    );

    bridge.sync_focus(foreground(2));
    assert_eq!(
        projection.try_projection(),
        Some(ContextProjection::Pending)
    );
    assert!(bridge.apply(BridgeMessage::Snapshot(snapshot(14))).is_err());
    bridge.apply(BridgeMessage::Snapshot(snapshot(15))).unwrap();
    assert!(matches!(
        projection.try_projection(),
        Some(ContextProjection::Normal { .. })
    ));
}

#[test]
fn named_pipe_server_accepts_frames_and_shuts_down_bounded() {
    let process_id = unsafe { GetCurrentProcessId() };
    let thread_id = unsafe { GetCurrentThreadId() };
    let pipe_name = format!(r"\\.\pipe\OpenViKey.Context.test.{process_id}.{thread_id}");
    let focus = Arc::new(FocusCache::new());
    focus.set_with_identity(15, "test.exe", process_id, thread_id);
    let projection = Arc::new(ContextProjectionSlot::new());
    let server =
        ContextBridgeServer::start_named(&pipe_name, Arc::clone(&focus), Arc::clone(&projection))
            .unwrap();
    let client = open_pipe_client(&pipe_name);
    let instance_id = 12;
    write_message(
        client,
        &BridgeMessage::Connect {
            protocol_version: CONTEXT_PROTOCOL_VERSION,
            source_pid: process_id,
            source_tid: thread_id,
            instance_id,
        },
    );
    wait_for(&projection, |value| value == ContextProjection::Pending);

    let mut observation = snapshot(14);
    observation.source_pid = process_id;
    observation.source_tid = thread_id;
    observation.instance_id = instance_id;
    write_message(client, &BridgeMessage::Snapshot(observation));
    wait_for(&projection, |value| {
        matches!(value, ContextProjection::Normal { .. })
    });

    unsafe {
        let _ = CloseHandle(client);
    }
    wait_for(&projection, |value| value == ContextProjection::Unsupported);
    let started = Instant::now();
    drop(server);
    assert!(started.elapsed() < Duration::from_secs(1));
}

fn open_pipe_client(pipe_name: &str) -> HANDLE {
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
        )
        .unwrap()
    };
    let mode = PIPE_READMODE_MESSAGE | PIPE_NOWAIT;
    unsafe {
        SetNamedPipeHandleState(handle, Some(&raw const mode), None, None).unwrap();
    }
    handle
}

fn write_message(client: HANDLE, message: &BridgeMessage) {
    let frame = encode_frame(message).unwrap();
    let mut written = 0_u32;
    unsafe {
        WriteFile(client, Some(&frame), Some(&raw mut written), None).unwrap();
    }
    assert_eq!(usize::try_from(written).unwrap(), frame.len());
}

fn wait_for(slot: &ContextProjectionSlot, predicate: impl Fn(ContextProjection) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(value) = slot.try_projection()
            && predicate(value)
        {
            return;
        }
        assert!(Instant::now() < deadline, "projection did not arrive");
        std::thread::sleep(Duration::from_millis(10));
    }
}
