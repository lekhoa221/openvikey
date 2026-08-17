//! Pure candidate generators.
//!
//! Wave 0 locks the seam: a generator sees a composition snapshot and
//! left context only. It must not take a model. Concrete generators
//! (abbrev / telex_fix / fuzzy / diacritics) arrive in M5 and M7.

use crate::types::{Candidate, CandidateSource, CompositionSnapshot};

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
