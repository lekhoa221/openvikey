//! Pure context classification and bridge contracts for the Windows host.

use serde::{Deserialize, Serialize};

/// Host treatment of the active TSF edit context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextState {
    /// The foreground application has no active TSF bridge.
    Unsupported,
    /// A TSF context exists but its first read has not completed.
    Pending,
    /// The context is safe for normal transform and learning policy.
    Normal,
    /// A password or PIN context; OpenViKey must pass through without reading text.
    Sensitive,
    /// The active TSF context could not be read.
    Unavailable,
}

/// Map Windows `InputScope` numeric values to a host context state.
#[must_use]
pub fn classify_input_scopes(scopes: &[i32]) -> ContextState {
    const SENSITIVE_SCOPES: [i32; 4] = [31, 64, 65, 66];
    if scopes.iter().any(|scope| SENSITIVE_SCOPES.contains(scope)) {
        ContextState::Sensitive
    } else {
        ContextState::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextState, classify_input_scopes};

    #[test]
    fn password_and_pin_scopes_are_sensitive() {
        let sensitive_scopes: &[&[i32]] = &[&[31], &[64], &[65], &[66], &[0, 31], &[65, 0]];
        for scopes in sensitive_scopes {
            assert_eq!(classify_input_scopes(scopes), ContextState::Sensitive);
        }
    }
}
