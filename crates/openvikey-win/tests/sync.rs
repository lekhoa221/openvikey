use openvikey_core::types::{
    CompositionSnapshot, EditRange, EngineAction, RangeBasis, ReplaceRangeAction,
};
use openvikey_session::session::SessionObservation;
use openvikey_win::sync::{InjectCommand, commands_from_caret_break, commands_from_typed};

fn obs(engine_actions: Vec<EngineAction>, action: Option<EngineAction>) -> SessionObservation {
    SessionObservation {
        event_seq: 1,
        snapshot: CompositionSnapshot::new(1, String::new(), String::new()),
        engine_actions,
        candidates: Vec::new(),
        decision: None,
        action,
    }
}

#[test]
fn update_replaces_previous_sent() {
    let observation = obs(
        vec![EngineAction::UpdateComposition {
            revision: 1,
            text: "à".into(),
        }],
        None,
    );
    let (cmds, sent) = commands_from_typed(&observation, "a");
    assert_eq!(
        cmds,
        vec![InjectCommand::Replace {
            backspace_graphemes: 1,
            text_nfc: "à".into()
        }]
    );
    assert_eq!(sent, "à");
}

#[test]
fn space_commit_appends_space_and_clears_sent() {
    let observation = obs(
        vec![EngineAction::Commit {
            revision: 1,
            text: "xin".into(),
            delimiter: Some(' '),
        }],
        None,
    );
    let (cmds, sent) = commands_from_typed(&observation, "xin");
    assert_eq!(
        cmds,
        vec![InjectCommand::AppendDelimiter { delimiter: ' ' }]
    );
    assert_eq!(sent, "");
}

#[test]
fn auto_replace_uses_action_not_commit_text() {
    let action = EngineAction::ReplaceRange(ReplaceRangeAction {
        edit_id: 1,
        range: EditRange {
            basis: RangeBasis::ActiveComposition,
            start_grapheme: 0,
            length_grapheme: 2,
            revision: 1,
        },
        original: "ko".into(),
        replacement: "không".into(),
        delimiter: Some(' '),
    });
    let observation = obs(
        vec![EngineAction::Commit {
            revision: 1,
            text: "ko".into(),
            delimiter: Some(' '),
        }],
        Some(action),
    );
    let (cmds, sent) = commands_from_typed(&observation, "ko");
    assert_eq!(
        cmds,
        vec![
            InjectCommand::Replace {
                backspace_graphemes: 2,
                text_nfc: "không".into()
            },
            InjectCommand::AppendDelimiter { delimiter: ' ' },
        ]
    );
    assert_eq!(sent, "");
    assert!(!cmds.iter().any(|c| matches!(
        c,
        InjectCommand::Replace { text_nfc, .. } if text_nfc == "ko"
    )));
}

#[test]
fn caret_break_does_not_backspace_into_new_focus() {
    let (cmds, sent) = commands_from_caret_break("chào");
    assert!(cmds.is_empty());
    assert_eq!(sent, "");
}

#[test]
fn newline_commit_does_not_append_delimiter() {
    let observation = obs(
        vec![EngineAction::Commit {
            revision: 1,
            text: "xin".into(),
            delimiter: Some('\n'),
        }],
        None,
    );
    let (cmds, sent) = commands_from_typed(&observation, "xin");
    assert_eq!(cmds, Vec::<InjectCommand>::new());
    assert_eq!(sent, "");
}

#[test]
fn period_commit_appends_period() {
    let observation = obs(
        vec![EngineAction::Commit {
            revision: 1,
            text: "xin".into(),
            delimiter: Some('.'),
        }],
        None,
    );
    let (cmds, sent) = commands_from_typed(&observation, "xin");
    assert_eq!(
        cmds,
        vec![InjectCommand::AppendDelimiter { delimiter: '.' }]
    );
    assert_eq!(sent, "");
}
