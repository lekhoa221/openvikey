//! Milestone 3: Golden Engine Tests for Telex and VNI.

use openvikey_core::engine::{Engine, EngineConfig};
use openvikey_core::types::*;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

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

    let bs = InputEvent {
        seq: 2,
        at_ms: 200,
        kind: InputKind::Backspace,
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    };

    // Each Backspace removes one visible grapheme while preserving modifiers
    // that still belong to the visible prefix.
    let remaining = [
        ("đườn", "dduownf"),
        ("đườ", "dduowf"),
        ("đư", "dduw"),
        ("đ", "dd"),
        ("", ""),
    ];
    for (expected_rendered, expected_raw) in remaining {
        engine.process(&bs);
        assert_eq!(engine.rendered(), expected_rendered);
        assert_eq!(engine.raw_keys().iter().collect::<String>(), expected_raw);
    }
    assert!(engine.is_empty());
}

fn assert_composed_grapheme_backspace(method: InputMethod, raw: &str) {
    let mut engine = Engine::new(EngineConfig {
        method,
        tone_placement: TonePlacement::Modern,
    });
    for (seq, ch) in raw.chars().enumerate() {
        engine.process(&key_event(seq as u64, ch));
    }
    assert_eq!(engine.rendered(), "kể", "setup failed for {method:?}");

    engine.process(&InputEvent {
        seq: 10,
        at_ms: 100,
        kind: InputKind::Backspace,
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    });

    assert_eq!(
        engine.rendered(),
        "k",
        "Backspace must remove visible `ể`, not only its final input modifier for {method:?}"
    );
}

#[test]
fn repeated_backspace_reduces_exactly_one_visible_grapheme() {
    let cases = [
        (InputMethod::Telex, "keer"),
        (InputMethod::Telex, "dduowngf"),
        (InputMethod::Telex, "vieetj"),
        (InputMethod::Telex, "nghieengs"),
        (InputMethod::Telex, "aaa"),
        (InputMethod::Vni, "ke63"),
        (InputMethod::Vni, "d9u7o7ng2"),
        (InputMethod::Vni, "vie6t5"),
    ];

    for (method, raw) in cases {
        let mut engine = Engine::new(EngineConfig {
            method,
            tone_placement: TonePlacement::Modern,
        });
        engine.restore_raw_keys(raw);
        while !engine.is_empty() {
            let before_graphemes = engine.rendered().graphemes(true).count();
            let before_raw = engine.raw_keys().len();
            engine.process(&InputEvent {
                seq: 1,
                at_ms: 1,
                kind: InputKind::Backspace,
                modifiers: Modifiers::empty(),
                is_repeat: false,
                context: InputContext::default(),
            });
            assert_eq!(
                engine.rendered().graphemes(true).count(),
                before_graphemes - 1,
                "wrong visible deletion for {method:?} raw `{raw}`"
            );
            assert!(
                engine.raw_keys().len() < before_raw,
                "Backspace must consume raw history for {method:?} raw `{raw}`"
            );
        }
    }
}

#[test]
fn typing_resumes_from_raw_history_after_visible_backspace() {
    let mut engine = Engine::new(EngineConfig::default());
    for (seq, ch) in "dduowngf".chars().enumerate() {
        engine.process(&key_event(seq as u64, ch));
    }
    let backspace = InputEvent {
        seq: 20,
        at_ms: 200,
        kind: InputKind::Backspace,
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    };
    engine.process(&backspace);
    engine.process(&backspace);
    assert_eq!(engine.rendered(), "đườ");
    assert_eq!(engine.raw_keys().iter().collect::<String>(), "dduowf");

    engine.process(&key_event(21, 'n'));
    assert_eq!(engine.rendered(), "đườn");
    engine.process(&key_event(22, 'g'));
    assert_eq!(engine.rendered(), "đường");
}

#[test]
fn telex_backspace_removes_the_last_visible_composed_grapheme() {
    assert_composed_grapheme_backspace(InputMethod::Telex, "keer");
}

#[test]
fn vni_backspace_removes_the_last_visible_composed_grapheme() {
    assert_composed_grapheme_backspace(InputMethod::Vni, "ke63");
}

fn key_event(seq: u64, ch: char) -> InputEvent {
    InputEvent {
        seq,
        at_ms: i64::try_from(seq).unwrap_or(0) * 10,
        kind: InputKind::Key {
            logical: ch,
            physical: None,
        },
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    }
}

#[test]
fn test_composed_snapshot_normalized_is_nfc() {
    let mut engine = Engine::new(EngineConfig::default());
    for (idx, ch) in "hoas".chars().enumerate() {
        engine.process(&key_event(idx as u64, ch));
    }
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.rendered, "hoá");
    assert_eq!(snapshot.normalized, "hoá".nfc().collect::<String>());
    assert_eq!(
        snapshot.normalized,
        snapshot.rendered.nfc().collect::<String>()
    );
}

#[test]
fn test_nfd_key_input_has_nfc_matching_form() {
    let mut engine = Engine::new(EngineConfig::default());
    let nfd: String = "é".nfd().collect();
    for (idx, ch) in nfd.chars().enumerate() {
        engine.process(&key_event(idx as u64, ch));
    }

    let snapshot = engine.snapshot();
    assert_eq!(snapshot.raw_keys, nfd);
    assert_eq!(snapshot.normalized, "é");
}

#[test]
fn test_reset_clears_composition_and_advances_revision() {
    let mut engine = Engine::new(EngineConfig::default());
    engine.process(&key_event(1, 'a'));
    let before_reset = engine.revision();

    let actions = engine.process(&InputEvent {
        seq: 2,
        at_ms: 20,
        kind: InputKind::Reset,
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    });

    assert!(engine.is_empty());
    assert_eq!(
        actions,
        vec![EngineAction::UpdateComposition {
            revision: before_reset + 1,
            text: String::new(),
        }]
    );
}

#[test]
fn test_revision_increases_on_each_key_and_boundary() {
    let mut engine = Engine::new(EngineConfig::default());
    assert_eq!(engine.revision(), 0);
    engine.process(&key_event(1, 'a'));
    let after_a = engine.revision();
    engine.process(&key_event(2, 's'));
    let after_s = engine.revision();
    assert!(after_a > 0);
    assert!(after_s > after_a);

    let commit = engine.process(&key_event(3, ' '));
    match &commit[0] {
        EngineAction::Commit { revision, .. } => {
            assert!(*revision > after_s);
        }
        other => panic!("expected Commit, got {other:?}"),
    }
}

#[test]
fn test_punctuation_commits_url_pieces_not_one_token() {
    let mut engine = Engine::new(EngineConfig::default());
    let mut commits = Vec::new();
    for (idx, ch) in "https://a".chars().enumerate() {
        for action in engine.process(&key_event(idx as u64, ch)) {
            if let EngineAction::Commit {
                text, delimiter, ..
            } = action
            {
                commits.push((text, delimiter));
            }
        }
    }
    assert!(
        commits.iter().any(|(_, delim)| *delim == Some(':')),
        "':' is a boundary; URL is not one composing token: {commits:?}"
    );
    assert!(
        commits.iter().any(|(_, delim)| *delim == Some('/')),
        "'/' is a boundary; URL is not one composing token: {commits:?}"
    );
}

#[test]
fn test_code_and_mixed_text_passthrough_when_policy_disables_transform() {
    let mut engine = Engine::new(EngineConfig::default());
    let context = InputContext {
        allow_transform: false,
        allow_learning: false,
    };
    let input = "case foo->bar a1b2";
    let mut output = String::new();
    for (idx, ch) in input.chars().enumerate() {
        let actions = engine.process(&InputEvent {
            seq: idx as u64,
            at_ms: i64::try_from(idx).unwrap_or(i64::MAX),
            kind: InputKind::Key {
                logical: ch,
                physical: None,
            },
            modifiers: Modifiers::empty(),
            is_repeat: false,
            context,
        });
        for action in actions {
            if let EngineAction::Commit {
                text, delimiter, ..
            } = action
            {
                output.push_str(&text);
                if let Some(delimiter) = delimiter {
                    output.push(delimiter);
                }
            }
        }
    }
    assert_eq!(output, input);
}

#[test]
fn test_insert_text_nfd_preserves_original_bytes_on_commit() {
    let mut engine = Engine::new(EngineConfig::default());
    let nfd: String = "é".nfd().collect();
    assert_ne!(nfd, "é".nfc().collect::<String>());

    let actions = engine.process(&InputEvent {
        seq: 1,
        at_ms: 1,
        kind: InputKind::InsertText { text: nfd.clone() },
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context: InputContext::default(),
    });

    match &actions[0] {
        EngineAction::Commit { text, .. } => {
            assert_eq!(text, &nfd, "undo/original path must keep the inserted form");
        }
        other => panic!("expected Commit, got {other:?}"),
    }
}

#[test]
fn restore_raw_keys_rebuilds_rendered_and_bumps_revision() {
    let mut engine = Engine::new(EngineConfig::default());
    let before = engine.revision();
    engine.restore_raw_keys("chaof");
    assert_eq!(engine.raw_keys().iter().collect::<String>(), "chaof");
    assert_eq!(engine.rendered(), "chào");
    assert!(engine.revision() > before);
    assert_eq!(engine.snapshot().raw_keys, "chaof");
}
