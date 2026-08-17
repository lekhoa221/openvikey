use openvikey_core::types::EngineAction;
use openvikey_session::session::{AcceptVisual, SessionObservation, UndoVisual};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectCommand {
    Replace {
        backspace_graphemes: usize,
        text_nfc: String,
    },
    AppendDelimiter { delimiter: char },
}

pub fn grapheme_len(s: &str) -> usize {
    s.graphemes(true).count()
}

pub fn commands_from_caret_break(_sent_nfc: &str) -> (Vec<InjectCommand>, String) {
    (Vec::new(), String::new())
}

pub fn commands_from_typed(
    obs: &SessionObservation,
    sent_nfc: &str,
) -> (Vec<InjectCommand>, String) {
    if let Some(EngineAction::ReplaceRange(action)) = &obs.action
        && obs
            .engine_actions
            .iter()
            .any(|a| matches!(a, EngineAction::Commit { .. }))
    {
        let mut cmds = vec![InjectCommand::Replace {
            backspace_graphemes: grapheme_len(sent_nfc),
            text_nfc: action.replacement.clone(),
        }];
        let delimiter = obs.engine_actions.iter().find_map(|a| match a {
            EngineAction::Commit { delimiter, .. } => *delimiter,
            _ => None,
        });
        if let Some(d) = delimiter
            && d != '\n'
        {
            cmds.push(InjectCommand::AppendDelimiter { delimiter: d });
        }
        return (cmds, String::new());
    }
    let mut sent = sent_nfc.to_string();
    let mut cmds = Vec::new();
    for action in &obs.engine_actions {
        match action {
            EngineAction::UpdateComposition { text, .. } => {
                cmds.push(InjectCommand::Replace {
                    backspace_graphemes: grapheme_len(&sent),
                    text_nfc: text.clone(),
                });
                sent.clone_from(text);
            }
            EngineAction::Commit { text, delimiter, .. } => {
                if sent != *text {
                    cmds.push(InjectCommand::Replace {
                        backspace_graphemes: grapheme_len(&sent),
                        text_nfc: text.clone(),
                    });
                    sent.clone_from(text);
                }
                if let Some(d) = *delimiter
                    && d != '\n'
                {
                    cmds.push(InjectCommand::AppendDelimiter { delimiter: d });
                }
                sent.clear();
            }
            EngineAction::ReplaceRange(_) | EngineAction::ShowSuggestions { .. } => {}
        }
    }
    (cmds, sent)
}

pub fn commands_from_accept(
    visual: &AcceptVisual,
    sent_nfc: &str,
    last_injected_token: &str,
) -> (Vec<InjectCommand>, String, String) {
    if visual.was_composing {
        (
            vec![
                InjectCommand::Replace {
                    backspace_graphemes: grapheme_len(sent_nfc),
                    text_nfc: visual.candidate_nfc.clone(),
                },
                InjectCommand::AppendDelimiter { delimiter: ' ' },
            ],
            String::new(),
            visual.candidate_nfc.clone(),
        )
    } else {
        (
            vec![InjectCommand::Replace {
                backspace_graphemes: grapheme_len(last_injected_token),
                text_nfc: visual.candidate_nfc.clone(),
            }],
            String::new(),
            visual.candidate_nfc.clone(),
        )
    }
}

pub fn commands_from_undo(visual: &UndoVisual, last_injected_token: &str) -> Vec<InjectCommand> {
    vec![InjectCommand::Replace {
        backspace_graphemes: grapheme_len(last_injected_token),
        text_nfc: visual.show_nfc.clone(),
    }]
}
