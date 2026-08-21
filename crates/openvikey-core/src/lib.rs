//! OpenViKey Core Engine
//!
//! Privacy-local, deterministic Vietnamese input method engine and learning model.

pub mod correction;
pub mod correction_memory;
pub mod decision;
pub mod engine;
pub mod feedback;
pub mod generate;
pub mod intervention;
pub mod learning_config;
pub mod lexicon;
pub mod model;
pub mod rank;
pub mod store;
pub mod types;

/// Package version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
