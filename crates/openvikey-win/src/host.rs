//! Synchronous typing host: session + inject (record and/or SendInput).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use arc_swap::ArcSwap;
use openvikey_core::chart::{ChartSnapshot, guarded_state_band};
use openvikey_core::correction::InterventionConfig;
use openvikey_core::engine::EngineConfig;
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{AdaptiveModel, ModelInspectionRow, RuleContextKey};
use openvikey_core::types::{
    EngineAction, FeedbackEvent, FeedbackKind, InputContext, InputKind, InputMethod, TonePlacement,
};
use openvikey_session::session::{LabSession, SessionCursors};
use openvikey_win_context::{ContextProjection, ContextState, ForegroundIdentity};

use crate::classify::profile_for_exe;
use crate::focus::{FocusCache, ForegroundSnapshot};
use crate::inject::{CommandInjector, InjectError, InjectProfile};
use crate::policy::{HostHotkey, HostState, KeyDecision, Mode, RawKey, decide};
use crate::settings::{
    AppInjectProfile, AppLearningPolicy, AppPolicyV1, AppTransformPolicy, HotkeySettingsV1,
};
use crate::sync::{
    InjectCommand, commands_from_accept, commands_from_caret_break, commands_from_typed,
    commands_from_undo, grapheme_len,
};

struct HostRuntime {
    host: Arc<Mutex<TypingHost>>,
    focus: Arc<FocusCache>,
    sending: Arc<AtomicBool>,
    mode: Arc<AtomicU8>,
    show_suggestions: Arc<AtomicBool>,
    allow_terminal: Arc<AtomicBool>,
    app_policies: Arc<ArcSwap<Vec<AppPolicyV1>>>,
    context: Arc<ContextProjectionSlot>,
    settings_path: Option<PathBuf>,
    persist: OnceLock<Arc<dyn Fn() + Send + Sync>>,
}

static RUNTIME: OnceLock<HostRuntime> = OnceLock::new();

#[derive(Debug, Clone)]
struct PublishedContextProjection {
    foreground: Option<ForegroundIdentity>,
    projection: ContextProjection,
}

/// Latest validated context shared with the hook through lock-free reads.
pub struct ContextProjectionSlot {
    inner: ArcSwap<PublishedContextProjection>,
}

impl ContextProjectionSlot {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: ArcSwap::from_pointee(PublishedContextProjection {
                foreground: None,
                projection: ContextProjection::Unsupported,
            }),
        }
    }

    pub fn publish(&self, foreground: ForegroundIdentity, projection: ContextProjection) {
        self.inner.store(Arc::new(PublishedContextProjection {
            foreground: Some(foreground),
            projection,
        }));
    }

    /// Fail closed while a field-focus transition is being classified.
    pub fn invalidate(&self, foreground: ForegroundIdentity) {
        self.publish(foreground, ContextProjection::Pending);
    }

    /// Read context state only when it belongs to this exact focus generation.
    #[must_use]
    pub fn try_state_for(&self, foreground: &ForegroundIdentity) -> Option<ContextState> {
        self.try_projection_for(foreground)
            .as_ref()
            .map(projection_state)
    }

    /// Read an owned projection only when PID/TID/generation and HWND still match.
    #[must_use]
    pub fn try_projection_for(&self, foreground: &ForegroundIdentity) -> Option<ContextProjection> {
        let published = self.inner.load();
        published
            .foreground
            .as_ref()
            .filter(|published| foregrounds_match(published, foreground))
            .map(|_| published.projection.clone())
    }

    /// Read the current projection for diagnostics outside the hook policy.
    #[must_use]
    pub fn try_projection(&self) -> Option<ContextProjection> {
        Some(self.inner.load().projection.clone())
    }
}

fn foregrounds_match(left: &ForegroundIdentity, right: &ForegroundIdentity) -> bool {
    left.pid == right.pid
        && left.tid == right.tid
        && left.generation == right.generation
        && !matches!((left.hwnd, right.hwnd), (Some(left), Some(right)) if left != right)
}

impl Default for ContextProjectionSlot {
    fn default() -> Self {
        Self::new()
    }
}

fn mode_to_u8(mode: Mode) -> u8 {
    match mode {
        Mode::Viet => 0,
        Mode::English => 1,
    }
}

fn mode_from_u8(value: u8) -> Mode {
    if value == 1 {
        Mode::English
    } else {
        Mode::Viet
    }
}

/// Bind host + focus for LL callbacks (call once before installing hooks).
pub fn bind_runtime(host: Arc<Mutex<TypingHost>>, focus: Arc<FocusCache>) {
    bind_runtime_with_context(host, focus, Arc::new(ContextProjectionSlot::new()));
}

/// Bind host, focus, and the validated TSF projection cache for LL callbacks.
pub fn bind_runtime_with_context(
    host: Arc<Mutex<TypingHost>>,
    focus: Arc<FocusCache>,
    context: Arc<ContextProjectionSlot>,
) {
    bind_runtime_with_context_and_settings(host, focus, context, None);
}

/// Bind product runtime with an explicit settings file. Tests use `None` and never touch profiles.
pub fn bind_runtime_with_context_and_settings(
    host: Arc<Mutex<TypingHost>>,
    focus: Arc<FocusCache>,
    context: Arc<ContextProjectionSlot>,
    settings_path: Option<PathBuf>,
) {
    let (sending, mode, show_suggestions, allow_terminal, app_policies) = match host.lock() {
        Ok(guard) => (
            Arc::clone(&guard.sending),
            Arc::clone(&guard.mode_flag),
            Arc::clone(&guard.suggestions_flag),
            Arc::clone(&guard.allow_terminal_flag),
            Arc::new(ArcSwap::from_pointee(guard.app_policies.clone())),
        ),
        Err(poisoned) => {
            let guard = poisoned.into_inner();
            (
                Arc::clone(&guard.sending),
                Arc::clone(&guard.mode_flag),
                Arc::clone(&guard.suggestions_flag),
                Arc::clone(&guard.allow_terminal_flag),
                Arc::new(ArcSwap::from_pointee(guard.app_policies.clone())),
            )
        }
    };
    let _ = RUNTIME.set(HostRuntime {
        host,
        focus,
        sending,
        mode,
        show_suggestions,
        allow_terminal,
        app_policies,
        context,
        settings_path,
        persist: OnceLock::new(),
    });
}

/// Notify both savers after the typing mutex is released.
pub fn bind_persist_notify(notify: Arc<dyn Fn() + Send + Sync>) {
    if let Some(rt) = RUNTIME.get() {
        let _ = rt.persist.set(notify);
    }
}

struct FocusSync {
    caps_lock: bool,
    alt: bool,
    meta: bool,
}

fn needs_session(decision: &KeyDecision) -> bool {
    matches!(
        decision,
        KeyDecision::EatAndInject(_)
            | KeyDecision::CommitAndPass { .. }
            | KeyDecision::Hotkey(_)
            | KeyDecision::CaretBreakAndPass
    )
}

fn app_policy_for<'a>(exe: &str, policies: &'a [AppPolicyV1]) -> Option<&'a AppPolicyV1> {
    let name = exe.rsplit(['/', '\\']).next().unwrap_or(exe);
    policies
        .iter()
        .find(|policy| policy.executable.eq_ignore_ascii_case(name))
}

fn sample_host_state(
    caps_lock: bool,
    alt: bool,
    meta: bool,
) -> (HostState, Option<ForegroundSnapshot>) {
    if let Some(rt) = RUNTIME.get() {
        let foreground = rt.focus.try_get_foreground();
        let foreground_exe = foreground
            .as_ref()
            .map_or_else(String::new, |snapshot| snapshot.exe.clone());
        let context_state = foreground
            .as_ref()
            .and_then(|snapshot| rt.context.try_state_for(&snapshot.identity))
            .unwrap_or(ContextState::Unavailable);
        let policies = rt.app_policies.load();
        let app_transform = app_policy_for(&foreground_exe, &policies)
            .map_or(AppTransformPolicy::Default, |policy| policy.transform);
        return (
            HostState {
                mode: mode_from_u8(rt.mode.load(Ordering::SeqCst)),
                foreground_exe,
                is_sending: rt.sending.load(Ordering::SeqCst),
                allow_terminal: rt.allow_terminal.load(Ordering::SeqCst),
                app_transform,
                caps_lock,
                alt,
                meta,
                context_state,
            },
            foreground,
        );
    }
    (
        HostState {
            mode: Mode::Viet,
            foreground_exe: "notepad.exe".into(),
            is_sending: false,
            allow_terminal: false,
            app_transform: AppTransformPolicy::Default,
            caps_lock,
            alt,
            meta,
            context_state: ContextState::Unsupported,
        },
        None,
    )
}

fn lock_free_host_state(caps_lock: bool, alt: bool, meta: bool) -> HostState {
    sample_host_state(caps_lock, alt, meta).0
}

fn after_unlock(lines: &[String], mode: Option<Mode>, notify_persist: bool) {
    after_unlock_with_notice(lines, None, mode, notify_persist);
}

fn after_unlock_with_notice(
    lines: &[String],
    notice: Option<openvikey_session::session::LearningNotice>,
    mode: Option<Mode>,
    notify_persist: bool,
) {
    if notify_persist
        && let Some(rt) = RUNTIME.get()
        && let Some(notify) = rt.persist.get()
    {
        notify();
    }
    let english_mode = mode == Some(Mode::English)
        || (mode.is_none()
            && RUNTIME
                .get()
                .is_some_and(|rt| mode_from_u8(rt.mode.load(Ordering::SeqCst)) == Mode::English));
    if !suggestions_visible() || english_mode {
        crate::overlay::dismiss_overlay();
    } else if let Some(notice) = notice {
        crate::overlay::push_learning_notice(notice);
    } else {
        crate::overlay::push_overlay_lines(lines);
    }
    if let Some(mode) = mode {
        crate::tray::set_tray_mode(mode);
    }
}

fn dispatch_locked_key(
    host: &Mutex<TypingHost>,
    raw: RawKey,
    at_ms: i64,
    sync: Option<&FocusSync>,
) -> KeyDecision {
    let (caps_lock, alt, meta) = sync.map_or((false, false, false), |sync| {
        (sync.caps_lock, sync.alt, sync.meta)
    });
    let (state, foreground) = sample_host_state(caps_lock, alt, meta);
    let decision = decide(&raw, &state);
    if !needs_session(&decision) {
        if state.context_state == ContextState::Sensitive {
            crate::overlay::dismiss_overlay();
        }
        let cancels_reopen = state.context_state == ContextState::Sensitive
            || (raw.down && (raw.control || alt || meta || matches!(raw.vk, 0x09 | 0x1B)));
        if cancels_reopen && let Ok(mut guard) = host.try_lock() {
            guard.cancel_reopen_anchor();
        }
        return decision;
    }
    let Ok(mut guard) = host.try_lock() else {
        return on_try_lock_fail(&decision);
    };
    let context_projection = if let Some(rt) = RUNTIME.get() {
        let Some(projection) = foreground
            .as_ref()
            .and_then(|snapshot| rt.context.try_projection_for(&snapshot.identity))
        else {
            return KeyDecision::Pass;
        };
        Some(projection)
    } else {
        None
    };
    if let Some(sync) = sync {
        if let Some(snapshot) = foreground {
            guard.sync_focus(
                u64::try_from(snapshot.hwnd).unwrap_or(0),
                snapshot.exe,
                snapshot.generation,
                at_ms,
            );
        }
        guard.caps_lock = sync.caps_lock;
        guard.alt = sync.alt;
        guard.meta = sync.meta;
    }
    if let Some(projection) = context_projection {
        guard.apply_context_projection(projection, at_ms);
    }
    let out = guard.handle_key(raw, at_ms);
    let lines = guard.overlay_display_lines();
    let notice = guard.session.take_learning_notice();
    let mode = guard.mode;
    let notify_persist = guard.allow_learning_for_foreground();
    drop(guard);
    after_unlock_with_notice(&lines, notice, Some(mode), notify_persist);
    out
}

/// Live keyboard path: lock-free [`decide`], then one `try_lock` for HWND + key.
pub fn handle_runtime_key(
    raw: RawKey,
    at_ms: i64,
    caps_lock: bool,
    alt: bool,
    meta: bool,
) -> KeyDecision {
    if raw.down && matches!(raw.vk, 0x09 | 0x25..=0x28 | 0x21..=0x24) {
        invalidate_context_runtime();
    }
    let Some(rt) = RUNTIME.get() else {
        return decide(&raw, &lock_free_host_state(caps_lock, alt, meta));
    };
    dispatch_locked_key(
        &rt.host,
        raw,
        at_ms,
        Some(&FocusSync {
            caps_lock,
            alt,
            meta,
        }),
    )
}

/// Mark the active field pending without blocking the hook callback.
pub fn invalidate_context_runtime() {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    if let Some(foreground) = rt
        .focus
        .try_mark_field_transition()
        .or_else(|| rt.focus.try_get_identity())
    {
        rt.context.invalidate(foreground);
    }
}

/// Mouse caret-break under a single `try_lock` (focus + modifiers + notify).
pub fn caret_break_runtime_locked(at_ms: i64, caps_lock: bool, alt: bool, meta: bool) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let Ok(mut guard) = rt.host.try_lock() else {
        return;
    };
    if let Some((hwnd, exe, generation)) = rt.focus.try_get_generation() {
        guard.sync_focus(u64::try_from(hwnd).unwrap_or(0), exe, generation, at_ms);
    }
    guard.caps_lock = caps_lock;
    guard.alt = alt;
    guard.meta = meta;
    guard.notify_caret_break(at_ms);
    let lines = guard.overlay_display_lines();
    let notify_persist = guard.allow_learning_for_foreground();
    drop(guard);
    after_unlock(&lines, None, notify_persist);
}

/// Tray left-click (message thread): blocking lock is allowed off the LL hook path.
pub fn handle_tray_left_click(at_ms: i64) {
    handle_tray_hotkey(HostHotkey::ToggleMode, at_ms);
}

/// Tray menu Gợi ý (message thread): hide/show overlay without changing V/E.
pub fn handle_tray_toggle_suggestions(at_ms: i64) {
    handle_tray_hotkey(HostHotkey::ToggleSuggestions, at_ms);
}

/// Overlay visibility for the tray check-mark (lock-free; message thread).
#[must_use]
pub fn suggestions_visible() -> bool {
    RUNTIME
        .get()
        .is_none_or(|rt| rt.show_suggestions.load(Ordering::SeqCst))
}

/// Lightweight snapshot for tray menus to prevent blocking typing mutex with large learned models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostTraySnapshot {
    pub mode: Mode,
    pub method: InputMethod,
    pub show_suggestions: bool,
    pub allow_terminal: bool,
}

#[must_use]
pub fn tray_snapshot() -> Option<HostTraySnapshot> {
    let rt = RUNTIME.get()?;
    let mode = mode_from_u8(rt.mode.load(Ordering::SeqCst));
    let show_suggestions = rt.show_suggestions.load(Ordering::SeqCst);
    let allow_terminal = rt.allow_terminal.load(Ordering::SeqCst);
    let method = if let Ok(guard) = rt.host.try_lock() {
        guard.session.engine_config().method
    } else {
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
        let path = crate::settings::default_settings_path(local_app_data.as_ref());
        crate::settings::load_settings(&path).map_or(InputMethod::Vni, |s| s.input_method)
    };
    Some(HostTraySnapshot {
        mode,
        method,
        show_suggestions,
        allow_terminal,
    })
}

/// Read-only product-control snapshot from the one live session.
///
/// Learning-chart fields are filled here on the Settings/idle path only; the
/// hook and inject paths never call `control_snapshot`.
#[derive(Debug, Clone)]
pub struct ControlSnapshot {
    pub mode: Mode,
    pub engine_config: EngineConfig,
    pub show_suggestions: bool,
    pub allow_terminal: bool,
    pub foreground_exe: String,
    pub last_external_exe: String,
    pub learning_allowed: bool,
    pub learned_rows: Vec<ModelInspectionRow>,
    pub learning_overview: crate::chart_view::LearningOverview,
    /// Chart of the most recently active learned rule (default selection).
    pub chart: Option<ChartSnapshot>,
}

/// Builds the read-only chart for one learned rule off the hook path.
#[must_use]
pub fn rule_chart_runtime(row: &ModelInspectionRow) -> Option<ChartSnapshot> {
    let rt = RUNTIME.get()?;
    let (memory, assessment) = {
        let guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
        (
            guard.session.model().correction_memory().clone(),
            guard.session.chart_assessment(row),
        )
    };
    let identity = openvikey_core::intervention::CorrectionIdentity {
        input_method: row.input_method,
        source: row.source,
        original_nfc: row.original_nfc.clone(),
        candidate_nfc: row.candidate_nfc.clone(),
        source_rule_id: row.source_rule_id.clone(),
    };
    let config = LearningConfigV2::compatibility_v1();
    let at_ms = crate::hook::now_ms();
    ChartSnapshot::from_memory_with_context(
        &memory,
        &identity,
        &config,
        at_ms,
        row.left_token_nfc.as_deref(),
        assessment.as_ref(),
    )
}

fn learning_overview_from_rows(
    memory: &openvikey_core::correction_memory::CorrectionMemory,
    rows: &[ModelInspectionRow],
    evaluate_at_ms: i64,
    pruned_rows: u64,
) -> crate::chart_view::LearningOverview {
    let config = LearningConfigV2::compatibility_v1();
    let mut bands = Vec::with_capacity(rows.len());
    let mut confidences = Vec::with_capacity(rows.len());
    for row in rows {
        let identity = openvikey_core::intervention::CorrectionIdentity {
            input_method: row.input_method,
            source: row.source,
            original_nfc: row.original_nfc.clone(),
            candidate_nfc: row.candidate_nfc.clone(),
            source_rule_id: row.source_rule_id.clone(),
        };
        bands.push(guarded_state_band(
            memory,
            &identity,
            evaluate_at_ms,
            row.left_token_nfc.as_deref(),
        ));
        confidences.push(memory.blended_confidence(
            &identity,
            row.left_token_nfc.as_deref(),
            evaluate_at_ms,
            config.context_shrinkage_k,
        ));
    }
    crate::chart_view::LearningOverview::from_bands_and_rows(
        &bands,
        rows,
        &confidences,
        pruned_rows,
    )
}

fn most_recent_row(rows: &[ModelInspectionRow]) -> Option<ModelInspectionRow> {
    rows.iter()
        .enumerate()
        .max_by(|(left_index, left), (right_index, right)| {
            left.last_evidence_at_ms
                .cmp(&right.last_evidence_at_ms)
                .then_with(|| right_index.cmp(left_index))
        })
        .map(|(_, row)| row.clone())
}

#[must_use]
pub fn control_snapshot() -> Option<ControlSnapshot> {
    let rt = RUNTIME.get()?;
    let (
        mode,
        engine_config,
        show_suggestions,
        allow_terminal,
        foreground_exe,
        last_external_exe,
        learning_allowed,
        memory,
        pruned_rows,
        learned_rows,
        chart_assessment,
    ) = {
        let guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
        let learned_rows = guard.session.model().inspection_rows();
        let chart_assessment =
            most_recent_row(&learned_rows).and_then(|row| guard.session.chart_assessment(&row));
        let pruned_rows = guard.session.model().pruned_row_count();
        (
            guard.mode,
            guard.session.engine_config(),
            guard.show_suggestions,
            guard.allow_terminal,
            guard.foreground_exe.clone(),
            guard.last_external_exe.clone(),
            guard.allow_learning_for_foreground(),
            guard.session.model().correction_memory().clone(),
            pruned_rows,
            learned_rows,
            chart_assessment,
        )
    };
    let at_ms = crate::hook::now_ms();
    let learning_overview = learning_overview_from_rows(&memory, &learned_rows, at_ms, pruned_rows);
    let chart = most_recent_row(&learned_rows).and_then(|row| {
        let identity = openvikey_core::intervention::CorrectionIdentity {
            input_method: row.input_method,
            source: row.source,
            original_nfc: row.original_nfc.clone(),
            candidate_nfc: row.candidate_nfc.clone(),
            source_rule_id: row.source_rule_id.clone(),
        };
        let config = LearningConfigV2::compatibility_v1();
        ChartSnapshot::from_memory_with_context(
            &memory,
            &identity,
            &config,
            at_ms,
            row.left_token_nfc.as_deref(),
            chart_assessment.as_ref(),
        )
    });
    Some(ControlSnapshot {
        mode,
        engine_config,
        show_suggestions,
        allow_terminal,
        foreground_exe,
        last_external_exe,
        learning_allowed,
        learned_rows,
        learning_overview,
        chart,
    })
}

fn persist_runtime_setting(
    mutate: impl FnOnce(&mut crate::settings::SettingsV1),
) -> Result<(), crate::settings::SettingsLoadError> {
    let Some(path) = RUNTIME
        .get()
        .and_then(|runtime| runtime.settings_path.as_deref())
    else {
        return Ok(());
    };
    crate::settings::mutate_settings(path, mutate).map(|_| ())
}

/// Change Mode (Viet/English) from the settings/tray thread.
pub fn set_mode_runtime(mode: Mode, at_ms: i64) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.mode == mode {
        return;
    }
    guard.apply_caret_break(at_ms);
    guard.last_injected_token.clear();
    guard.last_injected_hwnd = 0;
    guard.mode = mode;
    guard.mode_flag.store(mode_to_u8(mode), Ordering::SeqCst);
    let lines = guard.overlay_display_lines();
    drop(guard);
    after_unlock(&lines, Some(mode), false);
    let _ = persist_runtime_setting(|settings| settings.last_mode_viet = mode == Mode::Viet);
}

/// Change Telex/VNI from the tray/settings thread.
pub fn set_input_method_runtime(method: InputMethod, at_ms: i64) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    let mut config = guard.session.engine_config();
    if config.method == method {
        return;
    }
    config.method = method;
    guard.set_engine_config(config, at_ms);
    let lines = guard.overlay_display_lines();
    drop(guard);
    after_unlock(&lines, None, true);
    let _ = persist_runtime_setting(|settings| settings.input_method = method);
}

/// Change tone placement (Modern/Traditional) from the settings thread.
pub fn set_tone_placement_runtime(tone: TonePlacement, at_ms: i64) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    let mut config = guard.session.engine_config();
    if config.tone_placement == tone {
        return;
    }
    config.tone_placement = tone;
    guard.set_engine_config(config, at_ms);
    let lines = guard.overlay_display_lines();
    drop(guard);
    after_unlock(&lines, None, true);
    let _ = persist_runtime_setting(|settings| settings.tone_placement = tone);
}

/// Set suggestion overlay visibility from settings.
pub fn set_show_suggestions_runtime(show: bool) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    guard.set_show_suggestions(show);
    let lines = guard.overlay_display_lines();
    let ui_path = guard.ui_path.clone();
    drop(guard);
    if show {
        after_unlock(&lines, None, false);
    } else {
        crate::overlay::dismiss_overlay();
    }
    if let Some(path) = ui_path {
        let _ = crate::persist::save_ui_prefs(
            &path,
            crate::persist::UiPrefs {
                show_suggestions: show,
            },
        );
    }
    let _ = persist_runtime_setting(|settings| settings.show_suggestions = show);
}

/// Set terminal transformation permission from settings.
pub fn set_allow_terminal_runtime(allow: bool) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    guard.allow_terminal = allow;
    guard.allow_terminal_flag.store(allow, Ordering::SeqCst);
    drop(guard);
    let _ = persist_runtime_setting(|settings| settings.allow_terminal = allow);
}

/// Replace per-application policies in both lock-free hook policy and the live typing host.
pub fn set_app_policies_runtime(policies: Vec<AppPolicyV1>) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    rt.app_policies.store(Arc::new(policies.clone()));
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    guard.app_policies.clone_from(&policies);
    let foreground = guard.foreground_exe.clone();
    guard.apply_foreground_surface(foreground);
    drop(guard);
    let _ = persist_runtime_setting(move |settings| settings.app_policies = policies);
}

/// Validate and replace live hotkeys, then persist them for restart.
pub fn set_hotkeys_runtime(hotkeys: HotkeySettingsV1) -> Result<(), String> {
    crate::policy::set_runtime_hotkeys(&hotkeys)?;
    persist_runtime_setting(move |settings| settings.hotkeys = hotkeys)
        .map_err(|error| error.to_string())
}

/// Forget the latest learned rule from product UI, independent of V/E or foreground policy.
pub fn forget_last_rule_runtime() -> bool {
    let Some(rt) = RUNTIME.get() else {
        return false;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    let changed = guard.session.forget_last_rule();
    if changed {
        let lines = guard.overlay_display_lines();
        let notice = guard.session.take_learning_notice();
        drop(guard);
        after_unlock_with_notice(&lines, notice, None, true);
    }
    changed
}

/// Forget exactly one row selected from the learned-rules projection.
pub fn forget_rule_runtime(row: &ModelInspectionRow) -> bool {
    let Some(rt) = RUNTIME.get() else {
        return false;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    let changed = guard.session.forget_inspection_row(row);
    if changed {
        let lines = guard.overlay_display_lines();
        let notice = guard.session.take_learning_notice();
        drop(guard);
        after_unlock_with_notice(&lines, notice, None, true);
    }
    changed
}

fn handle_tray_hotkey(hotkey: HostHotkey, at_ms: i64) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    guard.handle_hotkey(hotkey, at_ms);
    let mode = matches!(hotkey, HostHotkey::ToggleMode).then_some(guard.mode);
    let lines = guard.overlay_display_lines();
    let notice = guard.session.take_learning_notice();
    let ui_path = guard.ui_path.clone();
    let show_suggestions = guard.show_suggestions;
    drop(guard);
    if show_suggestions {
        after_unlock_with_notice(&lines, notice, mode, false);
    } else {
        crate::overlay::dismiss_overlay();
        if let Some(mode) = mode {
            crate::tray::set_tray_mode(mode);
        }
    }
    if hotkey == HostHotkey::ToggleSuggestions
        && let Some(path) = ui_path
    {
        let _ = crate::persist::save_ui_prefs(&path, crate::persist::UiPrefs { show_suggestions });
    }
}

/// In-process host used by unit tests and the LL hook callback.
#[allow(clippy::struct_excessive_bools)]
pub struct TypingHost {
    pub session: LabSession,
    pub sent: String,
    pub last_injected_token: String,
    pub last_injected_hwnd: u64,
    pub hwnd: u64,
    pub focus_generation: u64,
    pub foreground_exe: String,
    pub last_external_exe: String,
    /// Latest host-validated TSF context projection for the active surface.
    pub context_projection: ContextProjection,
    pub mode: Mode,
    pub mode_flag: Arc<AtomicU8>,
    pub allow_terminal: bool,
    pub allow_terminal_flag: Arc<AtomicBool>,
    pub app_policies: Vec<AppPolicyV1>,
    pub caps_lock: bool,
    pub alt: bool,
    pub meta: bool,
    /// Shared with [`crate::inject::SendingGuard`] so policy sees in-flight SendInput.
    pub sending: Arc<AtomicBool>,
    /// When false, candidates still generate (Ctrl+.) but the overlay HWND stays hidden.
    pub show_suggestions: bool,
    pub suggestions_flag: Arc<AtomicBool>,
    /// Optional `%LOCALAPPDATA%\OpenViKey\ui.ovkdev.json` written from the tray thread.
    pub ui_path: Option<PathBuf>,
    pub profile: InjectProfile,
    /// Commands applied during this host's lifetime (tests assert on these).
    pub recorded: Vec<InjectCommand>,
    /// Call-stack ordering markers (`inject` before `enter` on CommitAndPass).
    pub stack_trace: Vec<String>,
    injector: Option<Box<dyn CommandInjector>>,
}

impl TypingHost {
    #[must_use]
    pub fn new_telex_fixture() -> Self {
        Self::new_with_session(LabSession::new(
            EngineConfig::default(),
            Lexicon::from_entries([], [], Some("win-telex-fixture")),
        ))
    }

    #[must_use]
    pub fn new_vni_auto_fixture() -> Self {
        let config = EngineConfig {
            method: InputMethod::Vni,
            tone_placement: TonePlacement::Modern,
        };
        let lexicon = Lexicon::from_entries(
            [LexiconEntry {
                token_nfc: "phát".to_string(),
                frequency: 10,
            }],
            [],
            Some("win-vni-auto-fixture"),
        );
        let (rule, _) = paht1_rule(config, &lexicon);
        let session = LabSession::new_with_model(
            config,
            lexicon,
            seed_accepts(&rule, 19),
            SessionCursors {
                next_seq: 19,
                next_edit_id: 1,
            },
        );
        Self::new_with_session(session)
    }

    #[must_use]
    pub fn new_with_session(session: LabSession) -> Self {
        Self {
            session,
            sent: String::new(),
            last_injected_token: String::new(),
            last_injected_hwnd: 0,
            hwnd: 1,
            focus_generation: 0,
            foreground_exe: "notepad.exe".into(),
            last_external_exe: "notepad.exe".into(),
            context_projection: ContextProjection::Unsupported,
            mode: Mode::Viet,
            mode_flag: Arc::new(AtomicU8::new(0)),
            allow_terminal: false,
            allow_terminal_flag: Arc::new(AtomicBool::new(false)),
            app_policies: Vec::new(),
            caps_lock: false,
            alt: false,
            meta: false,
            sending: Arc::new(AtomicBool::new(false)),
            show_suggestions: true,
            suggestions_flag: Arc::new(AtomicBool::new(true)),
            ui_path: None,
            profile: InjectProfile::Win32,
            recorded: Vec::new(),
            stack_trace: Vec::new(),
            injector: None,
        }
    }

    /// Attach a live / test injector; shares its `sending` flag with policy.
    pub fn set_injector(&mut self, injector: Box<dyn CommandInjector>) {
        self.sending = Arc::clone(injector.sending_flag());
        self.injector = Some(injector);
    }

    /// Focus change: caret-break forgets `sent` without backspacing into the new app.
    pub fn set_hwnd(&mut self, hwnd: u64, exe: String, at_ms: i64) {
        if hwnd != self.hwnd {
            self.reset_for_focus_change(at_ms);
            self.hwnd = hwnd;
        }
        self.apply_foreground_surface(exe);
    }

    /// Synchronize a Winevent generation, including A → blocked target → same A transitions.
    pub fn sync_focus(&mut self, hwnd: u64, exe: String, generation: u64, at_ms: i64) {
        if generation != self.focus_generation || hwnd != self.hwnd {
            self.reset_for_focus_change(at_ms);
            self.hwnd = hwnd;
            self.focus_generation = generation;
        }
        self.apply_foreground_surface(exe);
    }

    fn reset_for_focus_change(&mut self, at_ms: i64) {
        let (cmds, sent) = commands_from_caret_break(&self.sent);
        let _ = self.apply_commands(&cmds);
        self.sent = sent;
        self.last_injected_token.clear();
        self.last_injected_hwnd = 0;
        self.inject_caret_break_without_persistence(at_ms);
        self.session.clear_document_context();
    }

    /// Apply a host-validated context transition before the next physical key.
    pub fn apply_context_projection(&mut self, projection: ContextProjection, at_ms: i64) {
        if self.context_projection == projection {
            return;
        }
        let entering_blocked = !projection_blocks_input(&self.context_projection)
            && projection_blocks_input(&projection);
        if entering_blocked {
            self.sent.clear();
            self.last_injected_token.clear();
            self.last_injected_hwnd = 0;
            self.inject_caret_break_without_persistence(at_ms);
            self.session.clear_document_context();
        }
        match &projection {
            ContextProjection::Normal { left_token_nfc } => {
                self.session.rebase_left_context(left_token_nfc.clone());
            }
            ContextProjection::Unsupported
                if !matches!(self.context_projection, ContextProjection::Unsupported) =>
            {
                self.session.clear_document_context();
            }
            ContextProjection::Unsupported
            | ContextProjection::Pending
            | ContextProjection::Sensitive
            | ContextProjection::Unavailable => {}
        }
        self.context_projection = projection;
    }

    pub fn handle_hotkey(&mut self, hotkey: HostHotkey, at_ms: i64) {
        match hotkey {
            HostHotkey::AcceptTop => self.accept_top(at_ms),
            HostHotkey::RejectTop => {
                let checkpoint = self.session.checkpoint_for_inject(&InputKind::Reset);
                self.session
                    .reject_top_with_learning(at_ms, self.allow_learning_for_foreground());
                if !self.allow_learning_for_foreground() {
                    self.session.restore_persistent_state(&checkpoint);
                }
            }
            HostHotkey::UndoLast => self.undo_last(at_ms),
            HostHotkey::ForgetLastRule => {
                if self.allow_learning_for_foreground() {
                    let _ = self.session.forget_last_rule();
                }
            }
            HostHotkey::ToggleMode => {
                self.apply_caret_break(at_ms);
                self.last_injected_token.clear();
                self.last_injected_hwnd = 0;
                self.mode = match self.mode {
                    Mode::Viet => Mode::English,
                    Mode::English => Mode::Viet,
                };
                self.mode_flag
                    .store(mode_to_u8(self.mode), Ordering::SeqCst);
            }
            HostHotkey::ToggleSuggestions => {
                self.set_show_suggestions(!self.show_suggestions);
            }
        }
    }

    /// Candidate strings the overlay HWND should show (empty when the user hid suggestions).
    #[must_use]
    pub fn overlay_display_lines(&self) -> Vec<String> {
        crate::overlay::overlay_display_lines(
            &self.session.candidate_texts(),
            3,
            self.show_suggestions,
        )
    }

    pub fn set_show_suggestions(&mut self, show: bool) {
        self.show_suggestions = show;
        self.suggestions_flag.store(show, Ordering::SeqCst);
    }

    /// Set startup mode before hooks are installed.
    pub fn set_initial_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.mode_flag.store(mode_to_u8(mode), Ordering::SeqCst);
    }

    /// Apply Telex/VNI/tone settings at a safe caret boundary.
    pub fn set_engine_config(&mut self, config: EngineConfig, at_ms: i64) {
        self.apply_caret_break(at_ms);
        self.sent.clear();
        self.last_injected_token.clear();
        self.last_injected_hwnd = 0;
        self.session.set_engine_config(config);
        self.apply_foreground_surface(self.foreground_exe.clone());
    }

    /// Mouse / focus caret-break without going through keyboard policy.
    pub fn notify_caret_break(&mut self, at_ms: i64) {
        self.apply_caret_break(at_ms);
    }

    /// Synchronous key handling: inject runs on this stack before the decision is returned.
    ///
    /// On [`InjectError`], letters/`EatAndInject` become [`KeyDecision::Pass`]; Enter becomes
    /// [`KeyDecision::EatAndIgnore`] (same as try_lock fail) so we never eat a key that did not
    /// appear on screen.
    pub fn handle_key(&mut self, raw: RawKey, at_ms: i64) -> KeyDecision {
        let state = HostState {
            mode: self.mode,
            foreground_exe: self.foreground_exe.clone(),
            is_sending: self.sending.load(Ordering::SeqCst),
            allow_terminal: self.allow_terminal,
            app_transform: app_policy_for(&self.foreground_exe, &self.app_policies)
                .map_or(AppTransformPolicy::Default, |policy| policy.transform),
            caps_lock: self.caps_lock,
            alt: self.alt,
            meta: self.meta,
            context_state: projection_state(&self.context_projection),
        };
        let decision = decide(&raw, &state);
        match &decision {
            KeyDecision::EatAndInject(kind) => {
                let composition_was_empty = matches!(kind, InputKind::Backspace)
                    && self.session.composition_text().is_empty();
                if self.apply_typed(kind.clone(), at_ms).is_err() {
                    return on_try_lock_fail(&decision);
                }
                if composition_was_empty && self.session.composition_text().is_empty() {
                    if !self.session.has_pending_reopen() {
                        self.last_injected_token.clear();
                        self.last_injected_hwnd = 0;
                    }
                    return KeyDecision::Pass;
                }
            }
            KeyDecision::CommitAndPass { delimiter } => {
                if self
                    .apply_typed(
                        InputKind::Boundary {
                            delimiter: *delimiter,
                        },
                        at_ms,
                    )
                    .is_err()
                {
                    return on_try_lock_fail(&decision);
                }
                if *delimiter == '\n' {
                    self.stack_trace.push("enter".into());
                }
            }
            KeyDecision::CaretBreakAndPass => {
                self.apply_caret_break(at_ms);
            }
            KeyDecision::Hotkey(hotkey) => {
                self.handle_hotkey(*hotkey, at_ms);
            }
            KeyDecision::Pass => {
                if state.context_state == ContextState::Sensitive
                    || (raw.down
                        && (raw.control || self.alt || self.meta || matches!(raw.vk, 0x09 | 0x1B)))
                {
                    self.cancel_reopen_anchor();
                }
            }
            KeyDecision::EatAndIgnore => {}
        }
        decision
    }

    fn accept_top(&mut self, at_ms: i64) {
        let composing = !self.sent.is_empty() || !self.session.composition_text().is_empty();
        if !composing
            && (self.hwnd != self.last_injected_hwnd || self.last_injected_token.is_empty())
        {
            return;
        }
        let checkpoint = self.session.checkpoint_for_inject(&InputKind::Reset);
        let allow_learning = self.allow_learning_for_foreground();
        let Some(visual) = self.session.accept_top_with_learning(at_ms, allow_learning) else {
            return;
        };
        let (cmds, sent, token) =
            commands_from_accept(&visual, &self.sent, &self.last_injected_token);
        if self.apply_commands(&cmds).is_err() {
            self.session.restore_inject_checkpoint(checkpoint);
            return;
        }
        if !allow_learning {
            self.session.restore_persistent_state(&checkpoint);
        }
        self.sent = sent;
        self.last_injected_token = token;
        self.last_injected_hwnd = self.hwnd;
    }

    fn undo_last(&mut self, at_ms: i64) {
        if self.hwnd != self.last_injected_hwnd || self.last_injected_token.is_empty() {
            return;
        }
        let checkpoint = self.session.checkpoint_for_inject(&InputKind::Reset);
        let allow_learning = self.allow_learning_for_foreground();
        let Some(visual) = self.session.undo_last_with_learning(at_ms, allow_learning) else {
            return;
        };
        let cmds = commands_from_undo(&visual, &self.last_injected_token);
        if self.apply_commands(&cmds).is_err() {
            self.session.restore_inject_checkpoint(checkpoint);
            return;
        }
        if !allow_learning {
            self.session.restore_persistent_state(&checkpoint);
        }
        self.last_injected_token = visual.show_nfc;
    }

    fn apply_typed(&mut self, kind: InputKind, at_ms: i64) -> Result<(), InjectError> {
        let wants_reopen =
            matches!(kind, InputKind::Key { .. }) && self.session.has_pending_reopen();
        let can_reopen = wants_reopen
            && !self.last_injected_token.is_empty()
            && self.hwnd == self.last_injected_hwnd;
        if wants_reopen && !can_reopen {
            self.session.cancel_pending_reopen();
        }
        let restoring_backspace = matches!(kind, InputKind::Backspace)
            && self.session.composition_text().is_empty()
            && !self.last_injected_token.is_empty();
        let last_token = self.last_injected_token.clone();
        let checkpoint = self.session.checkpoint_for_inject(&kind);
        let allow_learning = self.allow_learning_for_foreground();
        let context = InputContext {
            allow_transform: true,
            allow_learning,
        };
        let obs = self.session.inject(kind, context, at_ms);
        if can_reopen && !obs.snapshot.rendered.is_empty() {
            let cmds = [InjectCommand::Replace {
                backspace_graphemes: grapheme_len(&last_token),
                text_nfc: obs.snapshot.rendered.clone(),
            }];
            if let Err(error) = self.apply_commands(&cmds) {
                self.session.restore_inject_checkpoint(checkpoint);
                self.session.cancel_pending_reopen();
                return Err(error);
            }
            if !allow_learning {
                self.session.restore_persistent_state(&checkpoint);
            }
            self.sent.clone_from(&obs.snapshot.rendered);
            self.last_injected_token.clone_from(&obs.snapshot.rendered);
            self.last_injected_hwnd = self.hwnd;
            return Ok(());
        }
        if restoring_backspace && !obs.snapshot.rendered.is_empty() {
            let cmds = [InjectCommand::Replace {
                backspace_graphemes: grapheme_len(&last_token).saturating_add(1),
                text_nfc: obs.snapshot.rendered.clone(),
            }];
            if let Err(error) = self.apply_commands(&cmds) {
                self.session.restore_inject_checkpoint(checkpoint);
                return Err(error);
            }
            if !allow_learning {
                self.session.restore_persistent_state(&checkpoint);
            }
            self.sent.clone_from(&obs.snapshot.rendered);
            self.last_injected_token.clone_from(&obs.snapshot.rendered);
            self.last_injected_hwnd = self.hwnd;
            return Ok(());
        }
        let (cmds, sent) = commands_from_typed(&obs, &self.sent);
        if let Err(error) = self.apply_commands(&cmds) {
            self.session.restore_inject_checkpoint(checkpoint);
            return Err(error);
        }
        if !allow_learning {
            self.session.restore_persistent_state(&checkpoint);
        }
        if obs
            .engine_actions
            .iter()
            .any(|a| matches!(a, EngineAction::Commit { .. }))
        {
            if let Some(EngineAction::ReplaceRange(action)) = &obs.action {
                self.last_injected_token.clone_from(&action.replacement);
            } else if let Some(text) = obs.engine_actions.iter().find_map(|a| match a {
                EngineAction::Commit { text, .. } if !text.is_empty() => Some(text.clone()),
                _ => None,
            }) {
                self.last_injected_token = text;
            }
            self.last_injected_hwnd = self.hwnd;
        }
        self.sent = sent;
        Ok(())
    }

    fn apply_foreground_surface(&mut self, exe: String) {
        self.foreground_exe = exe;
        if !self.foreground_exe.eq_ignore_ascii_case("OpenViKey.exe") {
            self.last_external_exe.clone_from(&self.foreground_exe);
        }
        self.profile = app_policy_for(&self.foreground_exe, &self.app_policies).map_or_else(
            || profile_for_exe(&self.foreground_exe),
            |policy| match policy.inject_profile {
                AppInjectProfile::Auto => profile_for_exe(&self.foreground_exe),
                AppInjectProfile::Win32 => InjectProfile::Win32,
                AppInjectProfile::Electron => InjectProfile::Electron,
            },
        );
        self.session
            .set_intervention_config(self.intervention_for_foreground());
    }

    fn intervention_for_foreground(&self) -> InterventionConfig {
        if crate::policy::is_terminal_exe(&self.foreground_exe)
            || crate::policy::is_denylisted(&self.foreground_exe)
        {
            InterventionConfig::default()
        } else if self.profile == InjectProfile::Electron {
            InterventionConfig::electron()
        } else {
            InterventionConfig::win32()
        }
    }

    fn allow_learning_for_foreground(&self) -> bool {
        if self.mode != Mode::Viet || !crate::policy::allows_learning(&self.foreground_exe) {
            return false;
        }
        app_policy_for(&self.foreground_exe, &self.app_policies)
            .is_none_or(|policy| policy.learning != AppLearningPolicy::Block)
    }

    fn apply_caret_break(&mut self, at_ms: i64) {
        let (cmds, sent) = commands_from_caret_break(&self.sent);
        let _ = self.apply_commands(&cmds);
        self.sent = sent;
        self.inject_caret_break_without_persistence(at_ms);
        self.cancel_reopen_anchor();
    }

    fn cancel_reopen_anchor(&mut self) {
        self.session.cancel_pending_reopen();
        self.last_injected_token.clear();
        self.last_injected_hwnd = 0;
    }

    fn inject_caret_break_without_persistence(&mut self, at_ms: i64) {
        let checkpoint = self.session.checkpoint_for_inject(&InputKind::CursorMoved);
        let _ = self.session.inject(
            InputKind::CursorMoved,
            InputContext {
                allow_transform: true,
                allow_learning: false,
            },
            at_ms,
        );
        self.session.restore_persistent_state(&checkpoint);
    }

    fn apply_commands(&mut self, cmds: &[InjectCommand]) -> Result<(), InjectError> {
        if let Some(injector) = self.injector.as_mut() {
            injector.apply_commands(cmds, self.profile)?;
        }
        if !cmds.is_empty() {
            self.stack_trace.push("inject".into());
        }
        self.recorded.extend(cmds.iter().cloned());
        Ok(())
    }
}

fn projection_state(projection: &ContextProjection) -> ContextState {
    match projection {
        ContextProjection::Unsupported => ContextState::Unsupported,
        ContextProjection::Pending => ContextState::Pending,
        ContextProjection::Normal { .. } => ContextState::Normal,
        ContextProjection::Sensitive => ContextState::Sensitive,
        ContextProjection::Unavailable => ContextState::Unavailable,
    }
}

fn projection_blocks_input(projection: &ContextProjection) -> bool {
    matches!(
        projection,
        ContextProjection::Pending | ContextProjection::Sensitive | ContextProjection::Unavailable
    )
}

/// When `try_lock` fails: map from the already-known lock-free [`KeyDecision`].
#[must_use]
pub fn on_try_lock_fail(decision: &KeyDecision) -> KeyDecision {
    match decision {
        KeyDecision::EatAndInject(_) | KeyDecision::CaretBreakAndPass | KeyDecision::Pass => {
            KeyDecision::Pass
        }
        KeyDecision::CommitAndPass { .. } | KeyDecision::Hotkey(_) | KeyDecision::EatAndIgnore => {
            KeyDecision::EatAndIgnore
        }
    }
}

/// Try to lock the host; on failure use [`on_try_lock_fail`].
pub fn handle_key_locked(host: &Mutex<TypingHost>, raw: RawKey, at_ms: i64) -> KeyDecision {
    dispatch_locked_key(host, raw, at_ms, None)
}

fn seed_accepts(key: &RuleContextKey, count: u64) -> AdaptiveModel {
    let mut model = AdaptiveModel::default();
    for seq in 1..=count {
        model.apply_feedback(
            key,
            &FeedbackEvent {
                seq,
                at_ms: 0,
                kind: FeedbackKind::Accept { candidate_id: 1 },
            },
            true,
        );
    }
    model
}

fn paht1_rule(config: EngineConfig, lexicon: &Lexicon) -> (RuleContextKey, String) {
    let mut probe = LabSession::new(config, lexicon.clone());
    let mut last = None;
    for (index, logical) in "paht1".chars().enumerate() {
        last = Some(probe.inject(
            InputKind::Key {
                logical,
                physical: None,
            },
            InputContext::default(),
            i64::try_from(index).unwrap_or(0),
        ));
    }
    let observation = last.expect("paht1 produces observations");
    let top = observation
        .candidates
        .first()
        .expect("paht1 has a top candidate");
    (
        RuleContextKey {
            input_method: InputMethod::Vni,
            source: top.source,
            original_nfc: observation.snapshot.normalized.clone(),
            candidate_nfc: top.text.clone(),
            left_token_nfc: None,
            source_rule_id: top.evidence.split('+').next().unwrap_or("").to_string(),
        },
        top.text.clone(),
    )
}
