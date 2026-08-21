//! Lát 0 freeze of current learning behavior.
//!
//! Lát 2/9 may replace named tests listed in the v2 implementation plan.
//! Every other test here must stay green unless a later lát explicitly inverts it.
//!
//! Covered elsewhere (do not duplicate):
//! - `telex_fix::boundary_assist_picks_unique_abbrev_on_space`
//! - `telex_fix::boundary_assist_picks_unique_fuzzy_on_space`
//! - `telex_fix::boundary_assist_rejects_diacritics_even_when_unique`
//! - `session_capture::abbrev_boundary_assist_replaces_on_space_without_accept_mass`
//! - `session_capture::fuzzy_boundary_assist_replaces_unique_typo_on_space`
//! - `session_capture::ntn_space_does_not_boundary_assist_a_guess`
//! - `session_capture::diacritics_does_not_boundary_assist_on_space`
//! - `session_capture::two_abbrev_assist_undos_stop_further_space_auto`

use openvikey_core::correction::{
    InterventionConfig, boundary_assist_candidate, unique_telex_fix_candidate,
};
use openvikey_core::generate::telex_fix::TelexFixGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, InputMethod, TonePlacement,
};

fn snap(raw: &str, rendered: &str) -> CompositionSnapshot {
    CompositionSnapshot::new(1, raw.to_string(), rendered.to_string())
}

fn lex(tokens: &[&str]) -> Lexicon {
    Lexicon::from_entries(
        tokens.iter().map(|token| LexiconEntry {
            token_nfc: (*token).to_string(),
            frequency: 10,
        }),
        [],
        Some("char-v2"),
    )
}

#[test]
fn telex_fix_unique_is_boundary_assist_on_win32_space() {
    let generator = TelexFixGenerator::new(InputMethod::Telex, TonePlacement::Modern);
    let snapshot = snap("chfao", "chfao");
    let candidates = generator.generate(&snapshot, &LeftContext::default());
    let lexicon = lex(&["chào"]);
    assert!(unique_telex_fix_candidate(&candidates).is_some());
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lexicon,
            true,
        )
        .is_some()
    );
}

#[test]
fn diacritics_source_never_boundary_assists() {
    let snapshot = snap("khong", "khong");
    let candidates = vec![Candidate {
        id: 1,
        text: "không".into(),
        source: CandidateSource::Diacritics,
        evidence: "diac:khong".into(),
        base_score: 0.9,
        final_score: 0.9,
    }];
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lex(&["không"]),
            true,
        )
        .is_none()
    );
}
