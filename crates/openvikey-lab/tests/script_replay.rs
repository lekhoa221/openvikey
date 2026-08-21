//! M9 deterministic accept/reject/undo replay.

use openvikey_lab::script::run_script_jsonl;

const SCRIPT: &str = r#"
{"op":"accept","seq":1,"at_ms":100,"candidate_id":3000000,"rule":{"input_method":"Telex","source":"Fuzzy","original_nfc":"paht1","candidate_nfc":"phát","left_token_nfc":null,"source_rule_id":"fuzzy:weighted:paht1->phát"}}
{"op":"reject","seq":2,"at_ms":100,"candidate_id":3000000,"rule":{"input_method":"Telex","source":"Fuzzy","original_nfc":"paht1","candidate_nfc":"phát","left_token_nfc":null,"source_rule_id":"fuzzy:weighted:paht1->phát"}}
{"op":"auto_emission","edit_id":7,"at_ms":100,"rule":{"input_method":"Telex","source":"Fuzzy","original_nfc":"paht1","candidate_nfc":"phát","left_token_nfc":null,"source_rule_id":"fuzzy:weighted:paht1->phát"}}
{"op":"undo","seq":3,"at_ms":100,"edit_id":7,"rule":{"input_method":"Telex","source":"Fuzzy","original_nfc":"paht1","candidate_nfc":"phát","left_token_nfc":null,"source_rule_id":"fuzzy:weighted:paht1->phát"}}
"#;

#[test]
fn replay_is_byte_identical_and_has_a_frozen_model_hash() {
    let first = run_script_jsonl(SCRIPT).expect("valid script");
    let second = run_script_jsonl(SCRIPT).expect("repeat script");

    assert_eq!(
        first.to_pretty_json().unwrap(),
        second.to_pretty_json().unwrap()
    );
    assert_eq!(first.operations_applied, 4);
    assert_eq!(first.feedback_events_applied, 3);
    assert_eq!(first.script_sha256.len(), 64);
    assert_eq!(
        first.model_sha256,
        "3c41f3e70c40945e06b31193179c9ce13e24a0d348b99bb2944866dfd75761f8"
    );
}

#[test]
fn duplicate_feedback_sequence_is_idempotent() {
    let once = run_script_jsonl(SCRIPT).unwrap();
    let duplicated = run_script_jsonl(&format!("{SCRIPT}{}", SCRIPT.lines().nth(1).unwrap()))
        .expect("duplicate sequence is accepted as deterministic no-op");

    assert_eq!(once.model_sha256, duplicated.model_sha256);
}
