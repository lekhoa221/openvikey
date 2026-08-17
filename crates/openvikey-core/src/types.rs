//! Core semantic types and action definitions.

use serde::{Deserialize, Serialize};

/// Engine input method type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMethod {
    Telex,
    Vni,
}
