//! Pure candidate generators.
//!
//! Wave 0 locks the seam: a generator sees a composition snapshot and
//! left context only. It must not take a model. Concrete generators
//! (abbrev / telex_fix / fuzzy / diacritics / personal) stay table-driven.

pub mod abbrev;
pub mod diacritics;
pub mod fuzzy;
pub mod personal;
pub mod telex_fix;
mod vietnamese;

use crate::types::{Candidate, CandidateSource, CompositionSnapshot, InputContext};

/// Tokens already committed to the left of the active composition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LeftContext {
    pub prev_token_nfc: Option<String>,
}

/// Pure generator. Implementations must not read personal model state.
pub trait Generator {
    fn source(&self) -> CandidateSource;

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        left_context: &LeftContext,
    ) -> Vec<Candidate>;
}

/// Entry point that honors `allow_transform`. Rank/decision must not run on the empty result.
#[must_use]
pub fn collect_candidates(
    snapshot: &CompositionSnapshot,
    left_context: &LeftContext,
    context: InputContext,
    generators: &[&dyn Generator],
) -> Vec<Candidate> {
    if !context.allow_transform {
        return Vec::new();
    }
    generators
        .iter()
        .flat_map(|generator| generator.generate(snapshot, left_context))
        .collect()
}
