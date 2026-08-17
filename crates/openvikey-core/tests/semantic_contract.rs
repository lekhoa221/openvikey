use openvikey_core::types::*;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

#[test]
fn test_input_event_round_trip() {
    let context = InputContext {
        allow_transform: true,
        allow_learning: true,
    };
    let event = InputEvent {
        seq: 42,
        at_ms: 1_700_000_000_000,
        kind: InputKind::Key {
            logical: 'a',
            physical: Some(0x04),
        },
        modifiers: Modifiers::empty(),
        is_repeat: false,
        context,
    };

    let json = serde_json::to_string(&event).expect("serialize");
    let deserialized: InputEvent = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(event, deserialized);
}

#[test]
fn test_composition_snapshot_preserves_nfc_and_original_raw() {
    let raw = "tooi";
    let rendered = "tôi";
    let snapshot = CompositionSnapshot::new(1, raw.to_string(), rendered.to_string());

    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.raw_keys, "tooi");
    assert_eq!(snapshot.rendered, "tôi");
    // Ensure normalized is strict NFC
    assert_eq!(snapshot.normalized, "tôi".nfc().collect::<String>());
}

#[test]
fn test_edit_range_basis_distinction() {
    let active_range = EditRange {
        basis: RangeBasis::ActiveComposition,
        start_grapheme: 0,
        length_grapheme: 3,
        revision: 5,
    };

    let committed_range = EditRange {
        basis: RangeBasis::CommittedBeforeCaret,
        start_grapheme: 0,
        length_grapheme: 3,
        revision: 5,
    };

    assert_ne!(active_range.basis, committed_range.basis);
    assert_eq!(active_range.grapheme_count(), 3);
}

#[test]
fn test_replace_range_inverse_generation() {
    let range = EditRange {
        basis: RangeBasis::CommittedBeforeCaret,
        start_grapheme: 0,
        length_grapheme: "ko".graphemes(true).count(),
        revision: 10,
    };

    let edit = ReplaceRangeAction {
        edit_id: 101,
        range,
        original: "ko".to_string(),
        replacement: "không".to_string(),
        delimiter: Some(' '),
    };

    let inverse = edit.to_inverse(11);
    assert_eq!(inverse.edit_id, 101);
    assert_eq!(inverse.original, "không");
    assert_eq!(inverse.replacement, "ko");
    assert_eq!(inverse.delimiter, Some(' '));
    assert_eq!(inverse.range.revision, 11);
    assert_eq!(
        inverse.range.length_grapheme,
        "không".graphemes(true).count()
    );
}

#[test]
fn test_undo_state_manager_rejects_stale_revision_or_cursor_break() {
    let mut undo_manager = UndoTracker::new(10);

    let edit = ReplaceRangeAction {
        edit_id: 1,
        range: EditRange {
            basis: RangeBasis::CommittedBeforeCaret,
            start_grapheme: 0,
            length_grapheme: 2,
            revision: 1,
        },
        original: "teh".to_string(),
        replacement: "the".to_string(),
        delimiter: None,
    };

    undo_manager.record_edit(edit.clone());

    // Valid undo matching top of stack
    let inverse_action = undo_manager.pop_undo(1);
    assert!(inverse_action.is_some());
    let inv = inverse_action.unwrap();
    assert_eq!(inv.original, "the");
    assert_eq!(inv.replacement, "teh");

    // Once popped or after cursor move, undoing again fails
    assert!(undo_manager.pop_undo(1).is_none());

    // Invalidation on cursor move
    undo_manager.record_edit(edit);
    undo_manager.invalidate_due_to_cursor_movement();
    assert!(undo_manager.pop_undo(1).is_none());
}

#[test]
fn test_multi_grapheme_and_multi_word_replacement_exact_reconstruction() {
    let original = "ntn";
    let replacement = "như thế nào";
    let edit = ReplaceRangeAction {
        edit_id: 202,
        range: EditRange {
            basis: RangeBasis::CommittedBeforeCaret,
            start_grapheme: 0,
            length_grapheme: original.graphemes(true).count(),
            revision: 20,
        },
        original: original.to_string(),
        replacement: replacement.to_string(),
        delimiter: Some(','),
    };

    let inverse = edit.to_inverse(21);
    assert_eq!(inverse.original, "như thế nào");
    assert_eq!(inverse.replacement, "ntn");
    assert_eq!(
        inverse.range.length_grapheme,
        replacement.graphemes(true).count()
    );
    assert_eq!(inverse.delimiter, Some(','));
}
