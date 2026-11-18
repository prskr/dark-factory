//! What the control plane and the sandbox runtime share about the sandbox: its filesystem paths,
//! the path globs of the push policy and the Landlock mode. Kept small so that the runtime does
//! not depend on the control plane's domain crate.

use serde::{Deserialize, Serialize};

pub mod glob;
pub mod paths;

/// Landlock confinement of the harness (instance-wide).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LandlockMode {
    /// Every harness runs in a Landlock domain; a run fails if the kernel can't provide one.
    #[default]
    Enforce,
    /// The harness runs unrestricted.
    Disabled,
}

impl LandlockMode {
    /// The spelling used in the instance config and in `DF_LANDLOCK`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::Disabled => "disabled",
        }
    }
}

/// A Landlock mode other than `enforce` or `disabled`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Landlock mode {0:?}: expected `enforce` or `disabled`")]
pub struct ParseLandlockModeError(String);

impl std::str::FromStr for LandlockMode {
    type Err = ParseLandlockModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [Self::Enforce, Self::Disabled]
            .into_iter()
            .find(|mode| mode.as_str() == value)
            .ok_or_else(|| ParseLandlockModeError(value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landlock_mode_parses_from_its_two_spellings_only() {
        assert_eq!("enforce".parse(), Ok(LandlockMode::Enforce));
        assert_eq!("disabled".parse(), Ok(LandlockMode::Disabled));
        assert!("off".parse::<LandlockMode>().is_err());
    }

    #[test]
    fn landlock_mode_spellings_match_the_config() {
        assert_eq!(LandlockMode::Disabled.as_str(), "disabled");
        assert_eq!(LandlockMode::Enforce.as_str(), "enforce");
    }
}
