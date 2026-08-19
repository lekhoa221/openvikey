//! Pure context classification and bridge contracts for the Windows host.

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

mod cache;
mod codec;

pub use cache::{ContextCache, ContextCacheError};
pub use codec::{BridgeCodecError, BridgeMessage, decode_frame, encode_frame};

/// Maximum UTF-8 size of a token transported from TSF to the host.
pub const MAX_TOKEN_BYTES: usize = 128;

/// Wire protocol major understood by both the TSF DLL and Windows host.
pub const CONTEXT_PROTOCOL_VERSION: u16 = 1;

/// User-ACL-protected local endpoint for protocol v1.
pub const CONTEXT_PIPE_NAME: &str = r"\\.\pipe\OpenViKey.Context.v1";

/// Host treatment of the active TSF edit context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextState {
    /// The foreground application has no active TSF bridge.
    Unsupported,
    /// A TSF context exists but its first read has not completed.
    Pending,
    /// The context is safe for normal transform and learning policy.
    Normal,
    /// A password or PIN context; OpenViKey must pass through without reading text.
    Sensitive,
    /// The active TSF context could not be read.
    Unavailable,
}

/// Foreground identity sampled by the host outside the keyboard-hook callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundIdentity {
    pub pid: u32,
    pub tid: u32,
    pub hwnd: Option<u64>,
    pub generation: u64,
}

/// One bounded context observation sent by a TSF service instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub protocol_version: u16,
    pub source_pid: u32,
    pub source_tid: u32,
    pub instance_id: u64,
    pub context_seq: u64,
    pub observed_seq: u64,
    pub hwnd: Option<u64>,
    pub state: ContextState,
    pub left_token_nfc: Option<String>,
}

/// Host-facing result after validating and matching the latest snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextProjection {
    Unsupported,
    Pending,
    Normal { left_token_nfc: Option<String> },
    Sensitive,
    Unavailable,
}

/// Owned value returned from the Windows TSF read adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadContextResult {
    pub state: ContextState,
    pub left_token_nfc: Option<String>,
    pub hwnd: Option<u64>,
}

/// Structural reason a context snapshot cannot cross the bridge boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotValidationError {
    UnsupportedProtocol,
    InvalidIdentity,
    InvalidSequence,
    InvalidWindow,
    InvalidStateTokenPair,
    InvalidToken,
}

impl ContextSnapshot {
    /// Validate invariants that do not require cache history or foreground state.
    pub fn validate(&self) -> Result<(), SnapshotValidationError> {
        if self.protocol_version != CONTEXT_PROTOCOL_VERSION {
            return Err(SnapshotValidationError::UnsupportedProtocol);
        }
        if self.source_pid == 0 || self.source_tid == 0 || self.instance_id == 0 {
            return Err(SnapshotValidationError::InvalidIdentity);
        }
        if self.context_seq == 0 || self.observed_seq == 0 {
            return Err(SnapshotValidationError::InvalidSequence);
        }
        if self.hwnd == Some(0) {
            return Err(SnapshotValidationError::InvalidWindow);
        }
        if self.state != ContextState::Normal && self.left_token_nfc.is_some() {
            return Err(SnapshotValidationError::InvalidStateTokenPair);
        }
        if let Some(token) = &self.left_token_nfc
            && (token.is_empty() || token.len() > MAX_TOKEN_BYTES || !token.nfc().eq(token.chars()))
        {
            return Err(SnapshotValidationError::InvalidToken);
        }
        Ok(())
    }
}

/// Map Windows `InputScope` numeric values to a host context state.
#[must_use]
pub fn classify_input_scopes(scopes: &[i32]) -> ContextState {
    const SENSITIVE_SCOPES: [i32; 4] = [31, 64, 65, 66];
    if scopes.iter().any(|scope| SENSITIVE_SCOPES.contains(scope)) {
        ContextState::Sensitive
    } else {
        ContextState::Normal
    }
}

/// Extract the final Unicode word before the caret and normalize it to NFC.
///
/// The TSF adapter bounds the source range separately. This function enforces
/// the smaller bridge contract and drops rather than truncates an oversized
/// token so a partial word can never influence learning.
#[must_use]
pub fn extract_previous_token(left_text: &str) -> Option<String> {
    let token = left_text
        .unicode_words()
        .next_back()?
        .nfc()
        .collect::<String>();
    (token.len() <= MAX_TOKEN_BYTES).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::{
        BridgeMessage, CONTEXT_PROTOCOL_VERSION, ContextCache, ContextProjection, ContextSnapshot,
        ContextState, ForegroundIdentity, ReadContextResult, classify_input_scopes, decode_frame,
        encode_frame, extract_previous_token,
    };

    #[test]
    fn password_and_pin_scopes_are_sensitive() {
        let sensitive_scopes: &[&[i32]] = &[&[31], &[64], &[65], &[66], &[0, 31], &[65, 0]];
        for scopes in sensitive_scopes {
            assert_eq!(classify_input_scopes(scopes), ContextState::Sensitive);
        }
    }

    #[test]
    fn extracts_last_unicode_word_and_normalizes_nfc() {
        assert_eq!(
            extract_previous_token("một xin cha\u{300}o,  "),
            Some("chào".to_owned())
        );
        assert_eq!(extract_previous_token("...  \t"), None);
    }

    #[test]
    fn rejects_tokens_above_the_bridge_bound() {
        let exactly_at_bound = "a".repeat(128);
        let above_bound = "a".repeat(129);
        assert_eq!(
            extract_previous_token(&exactly_at_bound),
            Some(exactly_at_bound)
        );
        assert_eq!(extract_previous_token(&above_bound), None);
    }

    fn normal_snapshot() -> ContextSnapshot {
        ContextSnapshot {
            protocol_version: CONTEXT_PROTOCOL_VERSION,
            source_pid: 10,
            source_tid: 11,
            instance_id: 12,
            context_seq: 13,
            observed_seq: 14,
            hwnd: Some(15),
            state: ContextState::Normal,
            left_token_nfc: Some("chào".to_owned()),
        }
    }

    #[test]
    fn owned_context_values_preserve_identity_and_read_result() {
        let foreground = ForegroundIdentity {
            pid: 10,
            tid: 11,
            hwnd: Some(15),
            generation: 14,
        };
        let result = ReadContextResult {
            state: ContextState::Normal,
            left_token_nfc: Some("chào".to_owned()),
            hwnd: Some(15),
        };
        assert_eq!(foreground.generation, 14);
        assert_eq!(result.left_token_nfc.as_deref(), Some("chào"));
        assert!(normal_snapshot().validate().is_ok());
    }

    #[test]
    fn snapshot_validation_rejects_malformed_identity_and_state() {
        let mut snapshot = normal_snapshot();
        snapshot.protocol_version += 1;
        assert!(snapshot.validate().is_err());

        let mut snapshot = normal_snapshot();
        snapshot.source_pid = 0;
        assert!(snapshot.validate().is_err());

        let mut snapshot = normal_snapshot();
        snapshot.context_seq = 0;
        assert!(snapshot.validate().is_err());

        let mut snapshot = normal_snapshot();
        snapshot.state = ContextState::Sensitive;
        assert!(snapshot.validate().is_err());

        let mut snapshot = normal_snapshot();
        snapshot.left_token_nfc = Some("cha\u{300}o".to_owned());
        assert!(snapshot.validate().is_err());
    }

    fn foreground(generation: u64) -> ForegroundIdentity {
        ForegroundIdentity {
            pid: 10,
            tid: 11,
            hwnd: Some(15),
            generation,
        }
    }

    #[test]
    fn cache_projects_only_a_fresh_snapshot_for_the_focus_generation() {
        let mut cache = ContextCache::new();
        cache.connect(10, 11, 12).unwrap();
        cache.focus_changed(foreground(1));
        assert_eq!(cache.project(&foreground(1)), ContextProjection::Pending);

        cache.ingest(normal_snapshot()).unwrap();
        assert_eq!(
            cache.project(&foreground(1)),
            ContextProjection::Normal {
                left_token_nfc: Some("chào".to_owned())
            }
        );

        cache.focus_changed(foreground(2));
        assert_eq!(cache.project(&foreground(2)), ContextProjection::Pending);
        assert!(cache.ingest(normal_snapshot()).is_err());
        assert_eq!(
            cache.project(&foreground(1)),
            ContextProjection::Unsupported
        );
    }

    #[test]
    fn cache_rejects_wrong_window_instance_and_receding_sequence() {
        let mut cache = ContextCache::new();
        cache.connect(10, 11, 12).unwrap();
        cache.focus_changed(foreground(14));
        cache.ingest(normal_snapshot()).unwrap();

        let mut stale = normal_snapshot();
        stale.context_seq -= 1;
        assert!(cache.ingest(stale).is_err());

        let mut wrong_instance = normal_snapshot();
        wrong_instance.instance_id += 1;
        assert!(cache.ingest(wrong_instance).is_err());

        let mut wrong_window = foreground(14);
        wrong_window.hwnd = Some(99);
        assert_eq!(cache.project(&wrong_window), ContextProjection::Unsupported);
    }

    #[test]
    fn cache_disconnect_invalidates_the_active_source() {
        let mut cache = ContextCache::new();
        cache.connect(10, 11, 12).unwrap();
        cache.focus_changed(foreground(1));
        cache.disconnect(10, 11, 12);
        assert_eq!(
            cache.project(&foreground(1)),
            ContextProjection::Unsupported
        );
    }

    #[test]
    fn bridge_frame_round_trips_a_valid_snapshot() {
        let message = BridgeMessage::Snapshot(normal_snapshot());
        let encoded = encode_frame(&message).unwrap();
        assert_eq!(decode_frame(&encoded).unwrap(), message);
    }

    #[test]
    fn bridge_frame_rejects_truncation_oversize_and_invalid_payload() {
        let encoded = encode_frame(&BridgeMessage::Snapshot(normal_snapshot())).unwrap();
        assert!(decode_frame(&encoded[..encoded.len() - 1]).is_err());

        let mut oversized = vec![0_u8; 4];
        oversized.copy_from_slice(&4097_u32.to_le_bytes());
        assert!(decode_frame(&oversized).is_err());

        let invalid = BridgeMessage::Connect {
            protocol_version: CONTEXT_PROTOCOL_VERSION + 1,
            source_pid: 10,
            source_tid: 11,
            instance_id: 12,
        };
        assert!(encode_frame(&invalid).is_err());
    }
}
