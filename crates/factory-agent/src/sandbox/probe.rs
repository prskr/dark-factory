//! Detects the Landlock ABI of the running kernel without restricting anything.

#[cfg(target_os = "linux")]
use landlock::{ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, Scope};

/// The ABI the harness ruleset needs.
pub const REQUIRED_ABI: u8 = 6;

/// What the kernel offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandlockSupport {
    /// Not built in or not enabled.
    Unavailable,
    /// The highest ABI up to [`REQUIRED_ABI`] the kernel supports.
    Abi(u8),
}

impl LandlockSupport {
    pub fn is_sufficient(self) -> bool {
        matches!(self, Self::Abi(abi) if abi >= REQUIRED_ABI)
    }

    /// The text of the `sandbox.landlock_unavailable` notice.
    pub fn unavailable_message(self) -> String {
        let found = match self {
            Self::Unavailable => "Landlock is not available in this kernel.".to_string(),
            Self::Abi(abi) => format!("Landlock ABI {abi} found, {REQUIRED_ABI} required."),
        };
        format!("{found} Set `sandbox.landlock: disabled` to run without it.")
    }
}

/// Reports what the kernel supports. A single-method trait so that the runner can be tested
/// with any ABI.
pub trait LandlockProbe: Send + Sync {
    fn support(&self) -> LandlockSupport;
}

/// Asks the kernel: creates (but never enforces) a ruleset for each ABI, highest first.
#[derive(Debug, Clone, Copy)]
pub struct KernelProbe;

impl LandlockProbe for KernelProbe {
    /// Landlock exists on Linux only; elsewhere it is unavailable, so an enforcing runtime fails
    /// closed like on a kernel without it.
    fn support(&self) -> LandlockSupport {
        #[cfg(target_os = "linux")]
        {
            const ABIS: [(u8, ABI); 6] = [
                (6, ABI::V6),
                (5, ABI::V5),
                (4, ABI::V4),
                (3, ABI::V3),
                (2, ABI::V2),
                (1, ABI::V1),
            ];
            ABIS.iter()
                .find(|(_, abi)| can_create(*abi))
                .map_or(LandlockSupport::Unavailable, |(n, _)| {
                    LandlockSupport::Abi(*n)
                })
        }
        #[cfg(not(target_os = "linux"))]
        LandlockSupport::Unavailable
    }
}

#[cfg(target_os = "linux")]
fn can_create(abi: ABI) -> bool {
    let ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(abi))
        .and_then(|ruleset| {
            let scopes = Scope::from_all(abi);
            if scopes.is_empty() {
                Ok(ruleset)
            } else {
                ruleset.scope(scopes)
            }
        });
    ruleset.and_then(Ruleset::create).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_abi_six_and_above_is_sufficient() {
        assert!(LandlockSupport::Abi(6).is_sufficient());
        assert!(!LandlockSupport::Abi(5).is_sufficient());
        assert!(!LandlockSupport::Unavailable.is_sufficient());
    }

    #[test]
    fn the_notice_names_the_detected_abi_and_the_opt_out() {
        let abi5 = LandlockSupport::Abi(5).unavailable_message();
        assert!(abi5.contains("ABI 5 found, 6 required"), "{abi5}");
        let none = LandlockSupport::Unavailable.unavailable_message();
        assert!(none.contains("not available"), "{none}");
        assert!(none.contains("sandbox.landlock: disabled"), "{none}");
    }

    /// The runtime probes in its own process: the probe must create rulesets, never enforce one.
    #[test]
    fn probing_leaves_the_process_unrestricted() -> anyhow::Result<()> {
        let _support = KernelProbe.support();
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("file"), "still writable")?;
        Ok(())
    }
}
