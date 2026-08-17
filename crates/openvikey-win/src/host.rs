//! Synchronous typing host (no OS hook): session + inject command recording.

use std::sync::Mutex;

use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{AdaptiveModel, RuleContextKey};
use openvikey_core::types::{
    EngineAction, FeedbackEvent, FeedbackKind, InputContext, InputKind, InputMethod, TonePlacement,
};
use openvikey_session::session::{LabSession, SessionCursors};

use crate::policy::{decide, HostHotkey, HostState, KeyDecision, Mode, RawKey};
use crate::sync::{
    commands_from_accept, commands_from_caret_break, commands_from_typed, commands_from_undo,
    InjectCommand,
};

/// In-process host used by unit tests and (later) the LL hook callback.
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
    pub is_sending: bool,
    /// Commands applied during this host's lifetime (tests assert on these).
    pub recorded: Vec<InjectCommand>,
    /// Call-stack ordering markers (`inject` before `enter` on CommitAndPass).
    pub stack_trace: Vec<String>,
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

    fn new_with_session(session: LabSession) -> Self {
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
            is_sending: false,
            recorded: Vec::new(),
            stack_trace: Vec::new(),
        }
    }

    /// Focus change: caret-break forgets `sent` without backspacing into the new app.
    pub fn set_hwnd(&mut self, hwnd: u64, exe: String, at_ms: i64) {
        if hwnd != self.hwnd {
            let (cmds, sent) = commands_from_caret_break(&self.sent);
            self.apply_commands(&cmds);
            self.sent = sent;
            self.last_injected_token.clear();
            self.last_injected_hwnd = 0;
            let _ = self.session.inject(
                InputKind::CursorMoved,
                InputContext::default(),
                at_ms,
            );
            self.hwnd = hwnd;
        }
        self.foreground_exe = exe;
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

    /// Synchronous key handling: inject runs on this stack before the decision is returned.
    pub fn handle_key(&mut self, raw: RawKey, at_ms: i64) -> KeyDecision {
        let state = HostState {
            mode: self.mode,
            foreground_exe: self.foreground_exe.clone(),
            is_sending: self.is_sending,
            caps_lock: self.caps_lock,
            alt: self.alt,
            meta: self.meta,
        };
        let decision = decide(&raw, &state);
        match &decision {
            KeyDecision::EatAndInject(kind) => {
                self.apply_typed(kind.clone(), at_ms);
            }
            KeyDecision::CommitAndPass { delimiter } => {
                self.apply_typed(
                    InputKind::Boundary {
                        delimiter: *delimiter,
                    },
                    at_ms,
                );
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
        self.apply_commands(&cmds);
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
        self.apply_commands(&cmds);
        self.last_injected_token = visual.show_nfc;
    }

    fn apply_typed(&mut self, kind: InputKind, at_ms: i64) {
        let obs = self
            .session
            .inject(kind, InputContext::default(), at_ms);
        let (cmds, sent) = commands_from_typed(&obs, &self.sent);
        self.apply_commands(&cmds);
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
    }

    fn apply_caret_break(&mut self, at_ms: i64) {
        let (cmds, sent) = commands_from_caret_break(&self.sent);
        self.apply_commands(&cmds);
        self.sent = sent;
        let _ = self
            .session
            .inject(InputKind::CursorMoved, InputContext::default(), at_ms);
    }

    fn apply_commands(&mut self, cmds: &[InjectCommand]) {
        if !cmds.is_empty() {
            self.stack_trace.push("inject".into());
        }
        self.recorded.extend(cmds.iter().cloned());
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
