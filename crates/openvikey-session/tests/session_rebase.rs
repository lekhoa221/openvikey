use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::types::InputContext;
use openvikey_session::session::LabSession;

fn contextual_session() -> LabSession {
    let lexicon = Lexicon::from_entries(
        [
            LexiconEntry {
                token_nfc: "chào".to_owned(),
                frequency: 1,
            },
            LexiconEntry {
                token_nfc: "bàn".to_owned(),
                frequency: 1,
            },
            LexiconEntry {
                token_nfc: "bạn".to_owned(),
                frequency: 1,
            },
        ],
        [(("chào".to_owned(), "bạn".to_owned()), 1.0)],
        Some("session-rebase-test"),
    );
    LabSession::new(EngineConfig::default(), lexicon)
}

#[test]
fn rebase_uses_one_normalized_external_token_without_persisting() {
    let mut session = contextual_session();
    let before = session.save_snapshot();

    session.rebase_left_context(Some("ignored cha\u{300}o".to_owned()));

    assert_eq!(session.save_snapshot(), before);
    let observations = session.type_text("ban", InputContext::default(), 10);
    assert_eq!(observations.last().unwrap().candidates[0].text, "bạn");
}

#[test]
fn rebase_drops_an_oversized_external_token() {
    let mut session = contextual_session();
    session.rebase_left_context(Some("a".repeat(129)));

    let observations = session.type_text("ban", InputContext::default(), 10);
    assert_ne!(observations.last().unwrap().candidates[0].text, "bạn");
}

#[test]
fn chart_assessment_reuses_the_last_planner_result_for_the_same_rule() {
    let mut session = contextual_session();
    session.rebase_left_context(Some("chào".to_owned()));
    let observations = session.type_text("ban", InputContext::default(), 10);
    let top = observations
        .last()
        .and_then(|observation| observation.candidates.first())
        .expect("ranked candidate");
    let row = session
        .model()
        .inspection_rows()
        .into_iter()
        .find(|row| row.original_nfc == "ban" && row.candidate_nfc == top.text)
        .expect("recorded decision row");

    let assessment = session.chart_assessment(&row).expect("planner assessment");

    assert!((assessment.breakdown.generator_base - top.base_score).abs() < f64::EPSILON);
    assert!((assessment.breakdown.final_score - top.final_score).abs() < f64::EPSILON);
}
