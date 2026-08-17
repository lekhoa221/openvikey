//! OpenViKey Core Engine
//!
//! Privacy-local, deterministic Vietnamese input method engine and learning model.

pub mod decision;
pub mod engine;
pub mod generate;
pub mod lexicon;
pub mod model;
pub mod store;
pub mod types;

/// Package version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
