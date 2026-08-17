//! Synchronous typing host: session + inject (record and/or SendInput).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{AdaptiveModel, RuleContextKey};
use openvikey_core::types::{
    EngineAction, FeedbackEvent, FeedbackKind, InputContext, InputKind, InputMethod, TonePlacement,
};
use openvikey_session::session::{LabSession, SessionCursors};

use crate::classify::profile_for_exe;
use crate::focus::FocusCache;
use crate::inject::{CommandInjector, InjectError, InjectProfile};
use crate::policy::{decide, HostHotkey, HostState, KeyDecision, Mode, RawKey};
use crate::sync::{
    commands_from_accept, commands_from_caret_break, commands_from_typed, commands_from_undo,
    InjectCommand,
};

struct HostRuntime {
    host: Arc<Mutex<TypingHost>>,
    focus: Arc<FocusCache>,
}

static RUNTIME: OnceLock<HostRuntime> = OnceLock::new();

/// Bind host + focus for LL callbacks (call once before installing hooks).
pub fn bind_runtime(host: Arc<Mutex<TypingHost>>, focus: Arc<FocusCache>) {
    let _ = RUNTIME.set(HostRuntime { host, focus });
}

/// Sync HWND/exe + caps/alt/meta under `try_lock` (only lock site outside [`handle_key_locked`]).
pub fn sync_runtime_locked(at_ms: i64, caps_lock: bool, alt: bool, meta: bool) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let Ok(mut guard) = rt.host.try_lock() else {
        return;
    };
    if let Some((hwnd, exe)) = rt.focus.try_get() {
        guard.set_hwnd(u64::try_from(hwnd).unwrap_or(0), exe, at_ms);
    }
    guard.caps_lock = caps_lock;
    guard.alt = alt;
    guard.meta = meta;
}

/// Live keyboard path: [`handle_key_locked`] against the bound runtime host.
pub fn handle_runtime_key_locked(raw: RawKey, at_ms: i64) -> KeyDecision {
    let Some(rt) = RUNTIME.get() else {
        return KeyDecision::Pass;
    };
    handle_key_locked(&rt.host, raw, at_ms)
}

/// Mouse caret-break under a single `try_lock` (focus + modifiers + notify).
pub fn caret_break_runtime_locked(at_ms: i64, caps_lock: bool, alt: bool, meta: bool) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let Ok(mut guard) = rt.host.try_lock() else {
        return;
    };
    if let Some((hwnd, exe)) = rt.focus.try_get() {
        guard.set_hwnd(u64::try_from(hwnd).unwrap_or(0), exe, at_ms);
    }
    guard.caps_lock = caps_lock;
    guard.alt = alt;
    guard.meta = meta;
    guard.notify_caret_break(at_ms);
}

/// In-process host used by unit tests and the LL hook callback.
#[allow(clippy::struct_excessive_bools)]
pub struct TypingHost {
    pub session: LabSession,
    pub sent: String,
    pub last_injected_token: String,
    pub last_injected_hwnd: u64,
    pub hwnd: u64,
    pub foreground_exe: String,
    pub mode: Mode,
    pub caps_lock: bool,
    pub alt: bool,
    pub meta: bool,
    /// Shared with [`crate::inject::SendingGuard`] so policy sees in-flight SendInput.
    pub sending: Arc<AtomicBool>,
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
            foreground_exe: "notepad.exe".into(),
            mode: Mode::Viet,
            caps_lock: false,
            alt: false,
            meta: false,
            sending: Arc::new(AtomicBool::new(false)),
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
            let (cmds, sent) = commands_from_caret_break(&self.sent);
            let _ = self.apply_commands(&cmds);
            self.sent = sent;
            self.last_injected_token.clear();
            self.last_injected_hwnd = 0;
            let _ = self
                .session
                .inject(InputKind::CursorMoved, InputContext::default(), at_ms);
            self.hwnd = hwnd;
        }
        self.foreground_exe = exe;
        self.profile = profile_for_exe(&self.foreground_exe);
    }

    pub fn handle_hotkey(&mut self, hotkey: HostHotkey, at_ms: i64) {
        match hotkey {
            HostHotkey::AcceptTop => self.accept_top(at_ms),
            HostHotkey::RejectTop => self.session.reject_top(at_ms),
            HostHotkey::UndoLast => self.undo_last(at_ms),
            HostHotkey::ToggleMode => {
                self.mode = match self.mode {
                    Mode::Viet => Mode::English,
                    Mode::English => Mode::Viet,
                };
            }
        }
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
            caps_lock: self.caps_lock,
            alt: self.alt,
            meta: self.meta,
        };
        let decision = decide(&raw, &state);
        match &decision {
            KeyDecision::EatAndInject(kind) => {
                if self.apply_typed(kind.clone(), at_ms).is_err() {
                    return on_inject_fail(&raw);
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
                    return on_inject_fail(&raw);
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
        let Some(visual) = self.session.accept_top(at_ms) else {
            return;
        };
        let (cmds, sent, token) =
            commands_from_accept(&visual, &self.sent, &self.last_injected_token);
        let _ = self.apply_commands(&cmds);
        self.sent = sent;
        self.last_injected_token = token;
        self.last_injected_hwnd = self.hwnd;
    }

    fn undo_last(&mut self, at_ms: i64) {
        if self.hwnd != self.last_injected_hwnd || self.last_injected_token.is_empty() {
            return;
        }
        let Some(visual) = self.session.undo_last(at_ms) else {
            return;
        };
        let cmds = commands_from_undo(&visual, &self.last_injected_token);
        let _ = self.apply_commands(&cmds);
        self.last_injected_token = visual.show_nfc;
    }

    fn apply_typed(&mut self, kind: InputKind, at_ms: i64) -> Result<(), InjectError> {
        let checkpoint = self.session.checkpoint_for_inject(&kind);
        let obs = self.session.inject(kind, InputContext::default(), at_ms);
        let (cmds, sent) = commands_from_typed(&obs, &self.sent);
        if let Err(error) = self.apply_commands(&cmds) {
            self.session.restore_inject_checkpoint(checkpoint);
            return Err(error);
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

    fn apply_caret_break(&mut self, at_ms: i64) {
        let (cmds, sent) = commands_from_caret_break(&self.sent);
        let _ = self.apply_commands(&cmds);
        self.sent = sent;
        let _ = self
            .session
            .inject(InputKind::CursorMoved, InputContext::default(), at_ms);
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

/// When `try_lock` fails: letters Pass; Enter eats (do not Pass / CommitAndPass).
#[must_use]
pub fn on_try_lock_fail(raw: &RawKey) -> KeyDecision {
    if raw.down && raw.vk == 0x0D {
        KeyDecision::EatAndIgnore
    } else {
        KeyDecision::Pass
    }
}

/// When SendInput fails after a decision that would eat: same mapping as try_lock fail.
#[must_use]
pub fn on_inject_fail(raw: &RawKey) -> KeyDecision {
    on_try_lock_fail(raw)
}

/// Try to lock the host; on failure use [`on_try_lock_fail`].
pub fn handle_key_locked(host: &Mutex<TypingHost>, raw: RawKey, at_ms: i64) -> KeyDecision {
    let Ok(mut guard) = host.try_lock() else {
        return on_try_lock_fail(&raw);
    };
    guard.handle_key(raw, at_ms)
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
            source_rule_id: top
                .evidence
                .split('+')
                .next()
                .unwrap_or("")
                .to_string(),
        },
        top.text.clone(),
    )
}
