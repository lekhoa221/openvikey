//! Opt-in typing diagnostics. Never records keystrokes or composed text.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::inject::InjectProfile;
use crate::policy::{
    HostState, KeyDecision, Mode, OVK_EXTRA, RawKey, is_denylisted, requires_terminal_opt_in,
};
use crate::settings::AppTransformPolicy;
use openvikey_win_context::ContextState;
use serde::Serialize;

/// One diagnostic record. Fields never include composed text or virtual-key codes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiagEvent {
    Key {
        at_ms: i64,
        exe: String,
        inject_profile: String,
        context: String,
        decision: String,
        pass_reason: Option<PassReason>,
        key_class: KeyClass,
        down: bool,
        sending: bool,
        hook_us: u64,
        anomalies: Vec<Anomaly>,
    },
    Inject {
        at_ms: i64,
        exe: String,
        inject_profile: String,
        backspaces: usize,
        utf16_units: usize,
        batches: usize,
        duration_us: u64,
        partial: bool,
        anomalies: Vec<Anomaly>,
    },
    Context {
        at_ms: i64,
        exe: String,
        from: String,
        to: String,
    },
}

/// Bounded in-memory log. Hot path uses `try_lock` and never writes disk.
pub struct DiagLog {
    enabled: AtomicBool,
    capacity: usize,
    events: Mutex<VecDeque<DiagEvent>>,
}

impl DiagLog {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            enabled: AtomicBool::new(false),
            capacity: capacity.max(1),
            events: Mutex::new(VecDeque::new()),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn try_push(&self, event: DiagEvent) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let Ok(mut guard) = self.events.try_lock() else {
            return;
        };
        if guard.len() >= self.capacity {
            guard.pop_front();
        }
        guard.push_back(event);
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<DiagEvent> {
        self.events
            .lock()
            .map(|guard| guard.iter().cloned().collect())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn to_jsonl(&self) -> String {
        self.snapshot()
            .iter()
            .filter_map(|event| serde_json::to_string(event).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Write the current snapshot as JSONL. Off the hot path.
    ///
    /// # Errors
    ///
    /// Returns I/O errors from creating the parent directory or writing the file.
    pub fn flush_to(&self, path: &Path) -> std::io::Result<usize> {
        let events = self.snapshot();
        if events.is_empty() {
            return Ok(0);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        for event in &events {
            let line = serde_json::to_string(event)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            writeln!(file, "{line}")?;
        }
        file.flush()?;
        Ok(events.len())
    }

    fn clear(&self) {
        if let Ok(mut events) = self.events.lock() {
            events.clear();
        }
    }
}

const CAPACITY: usize = 1024;

/// Process-wide log used by the hook host. Disabled until `--diag` or `OPENVIKEY_DIAG=1`.
#[must_use]
pub fn global() -> &'static DiagLog {
    static LOG: OnceLock<DiagLog> = OnceLock::new();
    LOG.get_or_init(|| DiagLog::new(CAPACITY))
}

/// Disable and empty the process log. Tests that touch the global must serialize.
pub fn reset_for_tests() {
    let log = global();
    log.set_enabled(false);
    log.clear();
}

/// `%LOCALAPPDATA%\OpenViKey\diag.jsonl`
#[must_use]
pub fn default_diag_path(local_app_data: Option<&Path>) -> PathBuf {
    let root =
        local_app_data.map_or_else(|| PathBuf::from("OpenViKey"), |base| base.join("OpenViKey"));
    root.join("diag.jsonl")
}

/// Parse `OPENVIKEY_DIAG` without touching the process environment in tests.
#[must_use]
pub fn env_value_enables(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "TRUE" | "yes" | "on"))
}

#[must_use]
pub fn env_requests_diag() -> bool {
    env_value_enables(std::env::var("OPENVIKEY_DIAG").ok().as_deref())
}

#[must_use]
pub fn elapsed_us(started: Option<std::time::Instant>) -> u64 {
    started.map_or(0, |start| {
        u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
    })
}

/// Why a physical key was passed through or ignored. Contains no key identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PassReason {
    OwnInject,
    Sensitive,
    Pending,
    Unavailable,
    Sending,
    English,
    EmptyForeground,
    Denylist,
    AppBlock,
    Terminal,
    TryLockFail,
}

/// Name the policy reason for a Pass/EatAndIgnore without copying key payload.
#[must_use]
pub fn pass_reason(raw: &RawKey, state: &HostState, decision: &KeyDecision) -> Option<PassReason> {
    if raw.extra_info == OVK_EXTRA {
        return Some(PassReason::OwnInject);
    }
    match state.context_state {
        ContextState::Sensitive => return Some(PassReason::Sensitive),
        ContextState::Pending => return Some(PassReason::Pending),
        ContextState::Unavailable => return Some(PassReason::Unavailable),
        ContextState::Normal | ContextState::Unsupported => {}
    }
    if state.is_sending {
        return Some(PassReason::Sending);
    }
    if !matches!(decision, KeyDecision::Pass | KeyDecision::EatAndIgnore) {
        return None;
    }
    if state.mode == Mode::English {
        return Some(PassReason::English);
    }
    if state.foreground_exe.is_empty() {
        return Some(PassReason::EmptyForeground);
    }
    if is_denylisted(&state.foreground_exe) {
        return Some(PassReason::Denylist);
    }
    if state.app_transform == AppTransformPolicy::Block {
        return Some(PassReason::AppBlock);
    }
    if requires_terminal_opt_in(&state.foreground_exe) && !state.allow_terminal {
        return Some(PassReason::Terminal);
    }
    None
}

/// Detected timing/policy anomaly. Used to filter a dump quickly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Anomaly {
    HookSlow,
    InjectSlow,
    InjectVerySlow,
    InjectPartial,
    EatAndIgnoreWhileSending,
    UnavailableWhileViet,
    TryLockFail,
}

/// Coarse key class for diagnostics. The virtual-key code is not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyClass {
    Letter,
    Digit,
    Backspace,
    Space,
    Enter,
    Other,
}

const HOOK_SLOW_US: u64 = 15_000;
const INJECT_SLOW_US: u64 = 5_000;
const INJECT_VERY_SLOW_US: u64 = 1_000_000;

/// Timing and policy flags for a key observation.
#[must_use]
pub fn key_anomalies(
    hook_us: u64,
    decision: &str,
    sending: bool,
    context: &str,
    viet: bool,
    try_lock_fail: bool,
) -> Vec<Anomaly> {
    let mut flags = Vec::new();
    if hook_us > HOOK_SLOW_US {
        flags.push(Anomaly::HookSlow);
    }
    if decision == "eat_and_ignore" && sending {
        flags.push(Anomaly::EatAndIgnoreWhileSending);
    }
    if context == "unavailable" && viet {
        flags.push(Anomaly::UnavailableWhileViet);
    }
    if try_lock_fail {
        flags.push(Anomaly::TryLockFail);
    }
    flags
}

/// Timing flags for one inject batch.
#[must_use]
pub fn inject_anomalies(duration_us: u64, partial: bool) -> Vec<Anomaly> {
    let mut flags = Vec::new();
    if duration_us > INJECT_VERY_SLOW_US {
        flags.push(Anomaly::InjectVerySlow);
    }
    if duration_us > INJECT_SLOW_US {
        flags.push(Anomaly::InjectSlow);
    }
    if partial {
        flags.push(Anomaly::InjectPartial);
    }
    flags
}

/// Classify a virtual-key code without retaining the code.
#[must_use]
pub fn classify_key(vk: u16) -> KeyClass {
    match vk {
        0x08 => KeyClass::Backspace,
        0x0D => KeyClass::Enter,
        0x20 => KeyClass::Space,
        0x30..=0x39 => KeyClass::Digit,
        0x41..=0x5A => KeyClass::Letter,
        _ => KeyClass::Other,
    }
}

/// Policy decision tag without `InputKind` / character payload.
#[must_use]
pub fn decision_name(decision: &KeyDecision) -> &'static str {
    match decision {
        KeyDecision::Pass => "pass",
        KeyDecision::EatAndIgnore => "eat_and_ignore",
        KeyDecision::EatAndInject(_) => "eat_and_inject",
        KeyDecision::CommitAndPass { .. } => "commit_and_pass",
        KeyDecision::Hotkey(_) => "hotkey",
        KeyDecision::CaretBreakAndPass => "caret_break_and_pass",
    }
}

#[must_use]
pub fn context_name(state: ContextState) -> &'static str {
    match state {
        ContextState::Unsupported => "unsupported",
        ContextState::Pending => "pending",
        ContextState::Normal => "normal",
        ContextState::Sensitive => "sensitive",
        ContextState::Unavailable => "unavailable",
    }
}

#[must_use]
pub fn profile_name(profile: InjectProfile) -> &'static str {
    match profile {
        InjectProfile::Win32 => "win32",
        InjectProfile::Electron => "electron",
    }
}

#[must_use]
pub fn exe_basename(exe: &str) -> String {
    exe.rsplit(['/', '\\']).next().unwrap_or(exe).to_string()
}

#[must_use]
pub fn expected_batches(profile: InjectProfile, backspaces: usize, utf16_units: usize) -> usize {
    if backspaces == 0 && utf16_units == 0 {
        0
    } else if profile == InjectProfile::Electron && backspaces > 0 && utf16_units > 0 {
        2
    } else {
        1
    }
}

/// Record a key observation on the process log when diagnostics are enabled.
#[allow(clippy::too_many_arguments)]
pub fn record_key(
    at_ms: i64,
    raw: &RawKey,
    state: &HostState,
    decision: &KeyDecision,
    inject_profile: InjectProfile,
    hook_us: u64,
    try_lock_fail: bool,
) {
    if !global().is_enabled() {
        return;
    }
    let decision_s = decision_name(decision);
    let context = context_name(state.context_state);
    let pass_reason = if try_lock_fail {
        Some(PassReason::TryLockFail)
    } else {
        pass_reason(raw, state, decision)
    };
    global().try_push(DiagEvent::Key {
        at_ms,
        exe: exe_basename(&state.foreground_exe),
        inject_profile: profile_name(inject_profile).to_owned(),
        context: context.to_owned(),
        decision: decision_s.to_owned(),
        pass_reason,
        key_class: classify_key(raw.vk),
        down: raw.down,
        sending: state.is_sending,
        hook_us,
        anomalies: key_anomalies(
            hook_us,
            decision_s,
            state.is_sending,
            context,
            state.mode == Mode::Viet,
            try_lock_fail,
        ),
    });
}

/// Record inject batch counts and duration. `text` is never stored.
pub fn record_inject(
    at_ms: i64,
    exe: &str,
    inject_profile: InjectProfile,
    backspaces: usize,
    utf16_units: usize,
    duration_us: u64,
    partial: bool,
) {
    if !global().is_enabled() || (backspaces == 0 && utf16_units == 0) {
        return;
    }
    global().try_push(DiagEvent::Inject {
        at_ms,
        exe: exe_basename(exe),
        inject_profile: profile_name(inject_profile).to_owned(),
        backspaces,
        utf16_units,
        batches: expected_batches(inject_profile, backspaces, utf16_units),
        duration_us,
        partial,
        anomalies: inject_anomalies(duration_us, partial),
    });
}

/// Record a context-verdict change.
pub fn record_context(at_ms: i64, exe: &str, from: ContextState, to: ContextState) {
    if !global().is_enabled() || from == to {
        return;
    }
    global().try_push(DiagEvent::Context {
        at_ms,
        exe: exe_basename(exe),
        from: context_name(from).to_owned(),
        to: context_name(to).to_owned(),
    });
}
