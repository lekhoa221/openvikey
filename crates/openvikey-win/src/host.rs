//! Synchronous typing host: session + inject (record and/or SendInput).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use arc_swap::ArcSwap;
use openvikey_core::correction::InterventionConfig;
use openvikey_core::engine::EngineConfig;
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
    context: Arc<ContextProjectionSlot>,
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
    let (sending, mode, show_suggestions, allow_terminal) = match host.lock() {
        Ok(guard) => (
            Arc::clone(&guard.sending),
            Arc::clone(&guard.mode_flag),
            Arc::clone(&guard.suggestions_flag),
            Arc::clone(&guard.allow_terminal_flag),
        ),
        Err(poisoned) => {
            let guard = poisoned.into_inner();
            (
                Arc::clone(&guard.sending),
                Arc::clone(&guard.mode_flag),
                Arc::clone(&guard.suggestions_flag),
                Arc::clone(&guard.allow_terminal_flag),
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
        context,
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
        return (
            HostState {
                mode: mode_from_u8(rt.mode.load(Ordering::SeqCst)),
                foreground_exe,
                is_sending: rt.sending.load(Ordering::SeqCst),
                allow_terminal: rt.allow_terminal.load(Ordering::SeqCst),
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
    if notify_persist
        && let Some(rt) = RUNTIME.get()
        && let Some(notify) = rt.persist.get()
    {
        notify();
    }
    crate::overlay::push_overlay_lines(lines);
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
    let mut lines = guard.overlay_display_lines();
    if let Some(notice) = guard.session.take_learning_notice() {
        lines.insert(0, notice.display_text());
    }
    let mode = guard.mode;
    let notify_persist = guard.allow_learning_for_foreground();
    drop(guard);
    after_unlock(&lines, Some(mode), notify_persist);
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
#[derive(Debug, Clone)]
pub struct ControlSnapshot {
    pub mode: Mode,
    pub engine_config: EngineConfig,
    pub show_suggestions: bool,
    pub allow_terminal: bool,
    pub foreground_exe: String,
    pub learning_allowed: bool,
    pub learned_rows: Vec<ModelInspectionRow>,
}

#[must_use]
pub fn control_snapshot() -> Option<ControlSnapshot> {
    let rt = RUNTIME.get()?;
    let guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    Some(ControlSnapshot {
        mode: guard.mode,
        engine_config: guard.session.engine_config(),
        show_suggestions: guard.show_suggestions,
        allow_terminal: guard.allow_terminal,
        foreground_exe: guard.foreground_exe.clone(),
        learning_allowed: guard.allow_learning_for_foreground(),
        learned_rows: guard.session.model().inspection_rows(),
    })
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
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let _ = crate::settings::mutate_saved_settings(local_app_data.as_ref(), |s| {
        s.last_mode_viet = mode == Mode::Viet;
    });
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
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let _ = crate::settings::mutate_saved_settings(local_app_data.as_ref(), |s| {
        s.input_method = method;
    });
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
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let _ = crate::settings::mutate_saved_settings(local_app_data.as_ref(), |s| {
        s.tone_placement = tone;
    });
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
    after_unlock(&lines, None, false);
    if let Some(path) = ui_path {
        let _ = crate::persist::save_ui_prefs(
            &path,
            crate::persist::UiPrefs {
                show_suggestions: show,
            },
        );
    }
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let _ = crate::settings::mutate_saved_settings(local_app_data.as_ref(), |s| {
        s.show_suggestions = show;
    });
}

/// Set terminal transformation permission from settings.
pub fn set_allow_terminal_runtime(allow: bool) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    guard.allow_terminal = allow;
    guard.allow_terminal_flag.store(allow, Ordering::SeqCst);
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let _ = crate::settings::mutate_saved_settings(local_app_data.as_ref(), |s| {
        s.allow_terminal = allow;
    });
}

/// Forget the latest learned rule through the same session that owns typing.
pub fn forget_last_rule_runtime(at_ms: i64) {
    handle_tray_hotkey(HostHotkey::ForgetLastRule, at_ms);
}

fn handle_tray_hotkey(hotkey: HostHotkey, at_ms: i64) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let mut guard = rt.host.lock().unwrap_or_else(PoisonError::into_inner);
    guard.handle_hotkey(hotkey, at_ms);
    let mode = matches!(hotkey, HostHotkey::ToggleMode).then_some(guard.mode);
    let mut lines = guard.overlay_display_lines();
    if let Some(notice) = guard.session.take_learning_notice() {
        lines.insert(0, notice.display_text());
    }
    let ui_path = guard.ui_path.clone();
    let show_suggestions = guard.show_suggestions;
    drop(guard);
    after_unlock(&lines, mode, false);
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
    /// Latest host-validated TSF context projection for the active surface.
    pub context_projection: ContextProjection,
    pub mode: Mode,
    pub mode_flag: Arc<AtomicU8>,
    pub allow_terminal: bool,
    pub allow_terminal_flag: Arc<AtomicBool>,
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
            context_projection: ContextProjection::Unsupported,
            mode: Mode::Viet,
            mode_flag: Arc::new(AtomicU8::new(0)),
            allow_terminal: false,
            allow_terminal_flag: Arc::new(AtomicBool::new(false)),
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
                    self.last_injected_token.clear();
                    self.last_injected_hwnd = 0;
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
            KeyDecision::Pass | KeyDecision::EatAndIgnore => {}
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
        self.profile = profile_for_exe(&self.foreground_exe);
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
        self.mode == Mode::Viet && crate::policy::allows_learning(&self.foreground_exe)
    }

    fn apply_caret_break(&mut self, at_ms: i64) {
        let (cmds, sent) = commands_from_caret_break(&self.sent);
        let _ = self.apply_commands(&cmds);
        self.sent = sent;
        self.inject_caret_break_without_persistence(at_ms);
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
