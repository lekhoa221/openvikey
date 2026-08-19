//! Bounded, length-prefixed bridge frames shared by the DLL and host.

use serde::{Deserialize, Serialize};

use crate::{CONTEXT_PROTOCOL_VERSION, ContextSnapshot};

/// Maximum serialized payload size, excluding the four-byte length prefix.
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 4096;

/// One connection-lifecycle or observation message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeMessage {
    Connect {
        protocol_version: u16,
        source_pid: u32,
        source_tid: u32,
        instance_id: u64,
    },
    Snapshot(ContextSnapshot),
    Disconnect {
        protocol_version: u16,
        source_pid: u32,
        source_tid: u32,
        instance_id: u64,
    },
}

/// Structural failure before a message is allowed to reach the context cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeCodecError {
    InvalidMessage,
    PayloadTooLarge,
    Truncated,
    LengthMismatch,
    MalformedJson,
}

impl BridgeMessage {
    fn validate(&self) -> Result<(), BridgeCodecError> {
        match self {
            Self::Connect {
                protocol_version,
                source_pid,
                source_tid,
                instance_id,
            }
            | Self::Disconnect {
                protocol_version,
                source_pid,
                source_tid,
                instance_id,
            } => {
                if *protocol_version != CONTEXT_PROTOCOL_VERSION
                    || *source_pid == 0
                    || *source_tid == 0
                    || *instance_id == 0
                {
                    return Err(BridgeCodecError::InvalidMessage);
                }
                Ok(())
            }
            Self::Snapshot(snapshot) => snapshot
                .validate()
                .map_err(|_| BridgeCodecError::InvalidMessage),
        }
    }
}

/// Serialize one validated message with a little-endian `u32` length prefix.
pub fn encode_frame(message: &BridgeMessage) -> Result<Vec<u8>, BridgeCodecError> {
    message.validate()?;
    let payload = serde_json::to_vec(message).map_err(|_| BridgeCodecError::MalformedJson)?;
    if payload.len() > MAX_FRAME_PAYLOAD_BYTES {
        return Err(BridgeCodecError::PayloadTooLarge);
    }
    let payload_len =
        u32::try_from(payload.len()).map_err(|_| BridgeCodecError::PayloadTooLarge)?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decode exactly one complete bounded frame and revalidate its message.
pub fn decode_frame(frame: &[u8]) -> Result<BridgeMessage, BridgeCodecError> {
    let prefix: [u8; 4] = frame
        .get(..4)
        .ok_or(BridgeCodecError::Truncated)?
        .try_into()
        .map_err(|_| BridgeCodecError::Truncated)?;
    let payload_len = usize::try_from(u32::from_le_bytes(prefix))
        .map_err(|_| BridgeCodecError::PayloadTooLarge)?;
    if payload_len > MAX_FRAME_PAYLOAD_BYTES {
        return Err(BridgeCodecError::PayloadTooLarge);
    }
    if frame.len() != 4 + payload_len {
        return Err(BridgeCodecError::LengthMismatch);
    }
    let message: BridgeMessage =
        serde_json::from_slice(&frame[4..]).map_err(|_| BridgeCodecError::MalformedJson)?;
    message.validate()?;
    Ok(message)
}
