//! Milestone 3: Golden Engine Tests for Telex and VNI.

use openvikey_core::engine::{Engine, EngineConfig};
use openvikey_core::types::*;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct GoldenCase {
    name: String,
    method: String,
    tone: String,
    input: String,
    expected: String,
}

fn run_fixture_file(relative_path: &str) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let file_path = Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(relative_path);

    let file = File::open(&file_path)
        .unwrap_or_else(|e| panic!("Failed to open fixture {}: {}", file_path.display(), e));
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line_content = line.expect("Failed to read line");
        if line_content.trim().is_empty() {
            continue;
        }

        let case: GoldenCase = serde_json::from_str(&line_content)
            .unwrap_or_else(|e| panic!("Failed to parse JSON '{line_content}': {e}"));

        let method = match case.method.as_str() {
            "telex" => InputMethod::Telex,
            "vni" => InputMethod::Vni,
            other => panic!("Unknown method: {other}"),
        };

        let tone_placement = match case.tone.as_str() {
            "modern" => TonePlacement::Modern,
            "classic" => TonePlacement::Classic,
            other => panic!("Unknown tone: {other}"),
        };

        let mut engine = Engine::new(EngineConfig {
            method,
            tone_placement,
        });

        for (idx, ch) in case.input.chars().enumerate() {
            let offset: i64 = i64::try_from(idx).unwrap_or(0);
            let event = InputEvent {
                seq: idx as u64,
                at_ms: 1000 + (offset * 50),
                kind: InputKind::Key {
                    logical: ch,
                    physical: None,
                },
                modifiers: Modifiers::empty(),
                is_repeat: false,
                context: InputContext::default(),
            };
            engine.process(&event);
        }

        assert_eq!(
            engine.rendered(),
            case.expected,
            "Failed golden case '{}' for input '{}'",
            case.name,
            case.input
        );
    }
}

#[test]
fn test_telex_golden_fixtures() {
    run_fixture_file("data/fixtures/engine/telex_golden.jsonl");
}

#[test]
fn test_vni_golden_fixtures() {
    run_fixture_file("data/fixtures/engine/vni_golden.jsonl");
}

#[test]
fn test_boundary_commit_and_delimiter() {
    let mut engine = Engine::new(EngineConfig::default());

    // Type "vieetj"
    for ch in "vieetj".chars() {
        let event = InputEvent {
            seq: 1,
            at_ms: 100,
            kind: InputKind::Key {
                logical: ch,
                physical: None,
            },
            modifiers: Modifiers::empty(),
            is_repeat: false,
            context: InputContext::default(),
        };
        engine.process(&event);
    }
    assert_eq!(engine.rendered(), "việt");

    // Hit space (boundary)
    let space_event = InputEvent {
        seq: 2,
        at_ms: 200,
        kind: InputKind::Key {
            logical: ' ',
            physical: None,
        },
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    };
    let actions = engine.process(&space_event);

    assert_eq!(actions.len(), 1);
    match &actions[0] {
        EngineAction::Commit {
            text, delimiter, ..
        } => {
            assert_eq!(text, "việt");
            assert_eq!(*delimiter, Some(' '));
        }
        other => panic!("Expected Commit action, got {other:?}"),
    }
    assert!(engine.is_empty());
}

#[test]
fn test_passthrough_when_transform_disabled() {
    let mut engine = Engine::new(EngineConfig::default());
    let context_no_transform = InputContext {
        allow_transform: false,
        allow_learning: false,
    };

    // Type "vieetj" with allow_transform=false (e.g. password field)
    let mut output = String::new();
    for ch in "vieetj".chars() {
        let event = InputEvent {
            seq: 1,
            at_ms: 100,
            kind: InputKind::Key {
                logical: ch,
                physical: None,
            },
            modifiers: Modifiers::empty(),
            is_repeat: false,
            context: context_no_transform,
        };
        let actions = engine.process(&event);
        for action in actions {
            if let EngineAction::Commit { text, .. } = action {
                output.push_str(&text);
            }
        }
    }

    assert_eq!(output, "vieetj");
    assert!(engine.is_empty());
}

#[test]
fn test_backspace_at_various_positions() {
    let mut engine = Engine::new(EngineConfig::default());

    // Type "dduowngf" -> "đường"
    for ch in "dduowngf".chars() {
        let event = InputEvent {
            seq: 1,
            at_ms: 100,
            kind: InputKind::Key {
                logical: ch,
                physical: None,
            },
            modifiers: Modifiers::empty(),
            is_repeat: false,
            context: InputContext::default(),
        };
        engine.process(&event);
    }
    assert_eq!(engine.rendered(), "đường");

    // Backspace 1 (pops 'f' tone mark) -> "đương"
    let bs = InputEvent {
        seq: 2,
        at_ms: 200,
        kind: InputKind::Backspace,
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    };
    engine.process(&bs);
    assert_eq!(engine.rendered(), "đương");

    // Backspace 2 (pops 'g') -> "đươn"
    engine.process(&bs);
    assert_eq!(engine.rendered(), "đươn");

    // Backspace 3 (pops 'n') -> "đuơ"
    engine.process(&bs);
    assert_eq!(engine.rendered(), "đuơ");
}
