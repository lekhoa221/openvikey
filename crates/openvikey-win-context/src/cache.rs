//! Foreground-bound projection of snapshots received from TSF instances.

use std::collections::HashMap;

use crate::{ContextProjection, ContextSnapshot, ContextState, ForegroundIdentity};

#[derive(Debug, Clone)]
struct SourceEntry {
    instance_id: u64,
    focus_generation: Option<u64>,
    focus_hwnd: Option<u64>,
    snapshot: Option<ContextSnapshot>,
    last_context_seq: u64,
    last_observed_seq: u64,
    awaiting_fresh_observation: bool,
}

/// Rejection reason at the stateful cache boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCacheError {
    InvalidSource,
    UnknownSource,
    WrongInstance,
    RecedingSequence,
    WindowMismatch,
    InvalidSnapshot,
}

/// Latest-value cache keyed by the TSF source process and thread.
#[derive(Debug, Default)]
pub struct ContextCache {
    sources: HashMap<(u32, u32), SourceEntry>,
}

impl ContextCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a TSF source connection. Repeating the same handshake is idempotent.
    pub fn connect(
        &mut self,
        process_id: u32,
        thread_id: u32,
        instance_id: u64,
    ) -> Result<(), ContextCacheError> {
        if process_id == 0 || thread_id == 0 || instance_id == 0 {
            return Err(ContextCacheError::InvalidSource);
        }
        let key = (process_id, thread_id);
        if self
            .sources
            .get(&key)
            .is_some_and(|entry| entry.instance_id == instance_id)
        {
            return Ok(());
        }
        self.sources.insert(
            key,
            SourceEntry {
                instance_id,
                focus_generation: None,
                focus_hwnd: None,
                snapshot: None,
                last_context_seq: 0,
                last_observed_seq: 0,
                awaiting_fresh_observation: false,
            },
        );
        Ok(())
    }

    /// Start a new host focus generation and invalidate any prior observation.
    pub fn focus_changed(&mut self, foreground: ForegroundIdentity) {
        for ((process_id, _), entry) in &mut self.sources {
            if *process_id == foreground.pid {
                entry.focus_generation = Some(foreground.generation);
                entry.focus_hwnd = foreground.hwnd;
                entry.snapshot = None;
                entry.awaiting_fresh_observation = true;
            }
        }
    }

    /// Validate and accept the latest observation for a connected source.
    pub fn ingest(&mut self, snapshot: ContextSnapshot) -> Result<(), ContextCacheError> {
        snapshot
            .validate()
            .map_err(|_| ContextCacheError::InvalidSnapshot)?;
        let entry = self
            .sources
            .get_mut(&(snapshot.source_pid, snapshot.source_tid))
            .ok_or(ContextCacheError::UnknownSource)?;
        if entry.instance_id != snapshot.instance_id {
            return Err(ContextCacheError::WrongInstance);
        }
        if let (Some(expected), Some(actual)) = (entry.focus_hwnd, snapshot.hwnd)
            && expected != actual
        {
            return Err(ContextCacheError::WindowMismatch);
        }
        if (entry.awaiting_fresh_observation && snapshot.observed_seq <= entry.last_observed_seq)
            || snapshot.context_seq < entry.last_context_seq
            || snapshot.observed_seq < entry.last_observed_seq
        {
            return Err(ContextCacheError::RecedingSequence);
        }
        entry.last_context_seq = snapshot.context_seq;
        entry.last_observed_seq = snapshot.observed_seq;
        entry.awaiting_fresh_observation = false;
        entry.snapshot = Some(snapshot);
        Ok(())
    }

    /// Remove a source only when the disconnect belongs to its current instance.
    pub fn disconnect(&mut self, process_id: u32, thread_id: u32, instance_id: u64) {
        let key = (process_id, thread_id);
        if self
            .sources
            .get(&key)
            .is_some_and(|entry| entry.instance_id == instance_id)
        {
            self.sources.remove(&key);
        }
    }

    /// Project only a snapshot anchored to the exact current foreground generation.
    #[must_use]
    pub fn project(&self, foreground: &ForegroundIdentity) -> ContextProjection {
        if foreground.pid == 0 || foreground.tid == 0 || foreground.generation == 0 {
            return ContextProjection::Unsupported;
        }
        if let Some(entry) = self.sources.get(&(foreground.pid, foreground.tid))
            && let Some(projection) = project_entry(entry, foreground, false)
        {
            return projection;
        }

        let mut same_process_source = false;
        let mut best = None;
        let mut candidates: Vec<_> = self
            .sources
            .iter()
            .filter(|((process_id, _), _)| *process_id == foreground.pid)
            .collect();
        candidates.sort_by_key(|((_, thread_id), _)| *thread_id);
        for (_, entry) in candidates {
            if entry.focus_generation != Some(foreground.generation)
                || windows_conflict(entry.focus_hwnd, foreground.hwnd)
            {
                continue;
            }
            same_process_source = true;
            if let Some(projection) = project_entry(entry, foreground, true) {
                best = Some(prefer_conservative(best, projection));
            }
        }
        best.unwrap_or(if same_process_source {
            ContextProjection::Pending
        } else {
            ContextProjection::Unsupported
        })
    }
}

fn project_entry(
    entry: &SourceEntry,
    foreground: &ForegroundIdentity,
    require_matching_window: bool,
) -> Option<ContextProjection> {
    if entry.focus_generation != Some(foreground.generation)
        || windows_conflict(entry.focus_hwnd, foreground.hwnd)
    {
        return None;
    }
    let snapshot = entry.snapshot.as_ref()?;
    if windows_conflict(snapshot.hwnd, foreground.hwnd)
        || (require_matching_window && !windows_match(snapshot.hwnd, foreground.hwnd))
    {
        return None;
    }
    Some(match snapshot.state {
        ContextState::Normal => ContextProjection::Normal {
            left_token_nfc: snapshot.left_token_nfc.clone(),
        },
        ContextState::Sensitive => ContextProjection::Sensitive,
        ContextState::Unavailable => ContextProjection::Unavailable,
        ContextState::Pending => ContextProjection::Pending,
        ContextState::Unsupported => ContextProjection::Unsupported,
    })
}

fn prefer_conservative(
    current: Option<ContextProjection>,
    candidate: ContextProjection,
) -> ContextProjection {
    let Some(current) = current else {
        return candidate;
    };
    if projection_priority(&candidate) > projection_priority(&current) {
        candidate
    } else {
        current
    }
}

fn projection_priority(projection: &ContextProjection) -> u8 {
    match projection {
        ContextProjection::Sensitive => 4,
        ContextProjection::Unavailable => 3,
        ContextProjection::Pending => 2,
        ContextProjection::Normal { .. } => 1,
        ContextProjection::Unsupported => 0,
    }
}

fn windows_match(left: Option<u64>, right: Option<u64>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left == right)
}

fn windows_conflict(left: Option<u64>, right: Option<u64>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left != right)
}
