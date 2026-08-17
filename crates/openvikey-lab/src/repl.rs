//! Raw-mode REPL adapter. Key mapping is pure so tests need no TTY.

use crate::persistence::DebouncedSaver;
use crate::session::{LabSession, extra_boundary_char};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{cursor, execute};
use openvikey_core::types::{InputContext, InputKind};
use std::io::{self, IsTerminal, Write};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplAction {
    Input(InputKind),
    AcceptTop,
    RejectTop,
    UndoLast,
    Quit,
}

#[derive(Debug, Error)]
pub enum ReplError {
    #[error("session needs an interactive terminal")]
    NotATty,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("save failed: {0}")]
    Save(String),
}

/// Maps a crossterm key press to a reducer/hotkey action. Ignores key release.
#[must_use]
pub fn key_event_to_action(event: &KeyEvent) -> Option<ReplAction> {
    if event.kind != KeyEventKind::Press {
        return None;
    }
    match (event.code, event.modifiers) {
        (KeyCode::Char('c' | 'd'), KeyModifiers::CONTROL) => Some(ReplAction::Quit),
        (KeyCode::Char('z'), KeyModifiers::CONTROL) => Some(ReplAction::UndoLast),
        (KeyCode::Tab, _) => Some(ReplAction::AcceptTop),
        (KeyCode::Esc, _) => Some(ReplAction::RejectTop),
        (KeyCode::Backspace, _) => Some(ReplAction::Input(InputKind::Backspace)),
        (KeyCode::Enter | KeyCode::Char(' '), _) => {
            Some(ReplAction::Input(InputKind::Boundary { delimiter: ' ' }))
        }
        (KeyCode::Char(logical), modifiers)
            if extra_boundary_char(logical)
                && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) =>
        {
            Some(ReplAction::Input(InputKind::Boundary {
                delimiter: logical,
            }))
        }
        (KeyCode::Char(logical), modifiers)
            if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
        {
            Some(ReplAction::Input(InputKind::Key {
                logical,
                physical: None,
            }))
        }
        _ => None,
    }
}

#[must_use]
pub fn render_line(session: &LabSession) -> String {
    let suggestion = session
        .top_suggestion()
        .map_or_else(String::new, |text| format!(" [gợi ý: {text}]"));
    let state = session
        .last_decision()
        .map_or_else(String::new, |state| format!(" ({state:?})"));
    format!(
        "{}{}{suggestion}{state}",
        session.document_text(),
        session.composition_text()
    )
}

pub fn run_repl(
    session: &mut LabSession,
    context: InputContext,
    model_saver: &DebouncedSaver,
    capture_saver: &DebouncedSaver,
) -> Result<(), ReplError> {
    let mut stdout = io::stdout();
    loop {
        draw(&mut stdout, session)?;
        let Event::Key(key) = crossterm::event::read()? else {
            continue;
        };
        let Some(action) = key_event_to_action(&key) else {
            continue;
        };
        let at_ms = next_event_at_ms(session.last_at_ms(), wall_clock_ms());
        match action {
            ReplAction::Quit => break,
            ReplAction::AcceptTop => session.accept_top(at_ms),
            ReplAction::RejectTop => session.reject_top(at_ms),
            ReplAction::UndoLast => session.undo_last(at_ms),
            ReplAction::Input(kind) => {
                session.inject(kind, context, at_ms);
            }
        }
        submit_savers(session, model_saver, capture_saver)?;
    }
    model_saver
        .flush()
        .map_err(|error| ReplError::Save(error.to_string()))?;
    capture_saver
        .flush()
        .map_err(|error| ReplError::Save(error.to_string()))?;
    execute!(
        stdout,
        cursor::MoveToColumn(0),
        Clear(ClearType::CurrentLine)
    )?;
    writeln!(stdout)?;
    Ok(())
}

fn submit_savers(
    session: &LabSession,
    model_saver: &DebouncedSaver,
    capture_saver: &DebouncedSaver,
) -> Result<(), ReplError> {
    let payload = session
        .model_payload()
        .map_err(|error| ReplError::Save(error.to_string()))?;
    model_saver
        .submit(payload)
        .map_err(|error| ReplError::Save(error.to_string()))?;
    let log = session
        .capture_log()
        .to_payload()
        .map_err(|error| ReplError::Save(error.to_string()))?;
    capture_saver
        .submit(log)
        .map_err(|error| ReplError::Save(error.to_string()))?;
    Ok(())
}

fn draw(stdout: &mut io::Stdout, session: &LabSession) -> io::Result<()> {
    execute!(
        stdout,
        cursor::MoveToColumn(0),
        Clear(ClearType::CurrentLine)
    )?;
    write!(stdout, "{}", render_line(session))?;
    stdout.flush()
}

pub fn require_tty() -> Result<(), ReplError> {
    if io::stdin().is_terminal() {
        Ok(())
    } else {
        Err(ReplError::NotATty)
    }
}

#[must_use]
pub fn next_event_at_ms(last_at_ms: i64, now_ms: i64) -> i64 {
    now_ms.max(last_at_ms.saturating_add(1))
}

#[must_use]
pub fn wall_clock_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

pub(crate) struct RawModeGuard;

impl RawModeGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

pub(crate) fn enter_raw_mode() -> io::Result<RawModeGuard> {
    RawModeGuard::enter()
}

pub fn read_hidden_passphrase() -> io::Result<Zeroizing<String>> {
    eprint!("Passphrase: ");
    io::stderr().flush()?;
    let _raw = RawModeGuard::enter()?;
    let mut buffer = Zeroizing::new(String::new());
    loop {
        let Event::Key(key) = crossterm::event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Enter => break,
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Char(logical)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                buffer.push(logical);
            }
            _ => {}
        }
    }
    execute!(
        io::stderr(),
        Clear(ClearType::CurrentLine),
        cursor::MoveToColumn(0)
    )?;
    eprintln!();
    Ok(buffer)
}
