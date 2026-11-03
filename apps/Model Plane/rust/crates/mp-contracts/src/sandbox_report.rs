//! The per-call sandbox enforcement report (EP-9 E1).
//!
//! # What it says
//!
//! A `code_interpreter` result says whether the program ran inside the OS
//! sandbox, which network tier the run was allowed, and how much of that the
//! sandbox confirmed. It describes the confinement of the run. It says nothing
//! about the program's output.
//!
//! # Why it is closed
//!
//! Every field takes one of the values below. A reader that meets anything else
//! must show the run as unknown, never as isolated. The strings are the wire
//! contract the SPA's tool card matches on, so a rename fails this file's tests
//! first.

use serde::{Deserialize, Serialize};

/// The network tier a run was allowed. Mirrors the egress tiers of EP-9 D-E3.
/// Only `off` is produced today; the other tiers wait for the allow-list proxy
/// (EP-9 E3), which is also the first producer of `registries`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkTier {
    /// No network. The sandbox removes the network namespace.
    Off,
    /// Package registries only.
    Registries,
    /// The Space's allow-list.
    Allowlist,
    /// Unrestricted network.
    All,
}

/// How much of the run's policy the sandbox confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Enforcement {
    /// The process was wrapped and its network is `off`, so the whole policy holds.
    Full,
    /// The process was wrapped, but the sandbox does not confirm the network
    /// boundary: the allow-list is advisory, or the network is unrestricted.
    Partial,
    /// The process was not wrapped by the sandbox at all.
    #[serde(rename = "none")]
    Unenforced,
}

/// The `sandbox` object of a code result: `{sandboxed, network, enforcement}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxReport {
    /// Whether the sandbox wrapped the process.
    pub sandboxed: bool,
    /// The network tier the run's policy allowed.
    pub network: NetworkTier,
    /// How much of that the sandbox confirmed.
    pub enforcement: Enforcement,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire strings are what the tool card matches on.
    #[test]
    fn the_vocabulary_serialises_to_the_named_strings() {
        let tiers = [
            (NetworkTier::Off, "off"),
            (NetworkTier::Registries, "registries"),
            (NetworkTier::Allowlist, "allowlist"),
            (NetworkTier::All, "all"),
        ];
        for (tier, wire) in tiers {
            assert_eq!(
                serde_json::to_string(&tier).expect("serializes"),
                format!("\"{wire}\"")
            );
        }
        let levels = [
            (Enforcement::Full, "full"),
            (Enforcement::Partial, "partial"),
            (Enforcement::Unenforced, "none"),
        ];
        for (level, wire) in levels {
            assert_eq!(
                serde_json::to_string(&level).expect("serializes"),
                format!("\"{wire}\"")
            );
        }
    }

    #[test]
    fn a_report_has_the_documented_shape() {
        let report = SandboxReport {
            sandboxed: true,
            network: NetworkTier::Off,
            enforcement: Enforcement::Full,
        };
        assert_eq!(
            serde_json::to_string(&report).expect("serializes"),
            r#"{"sandboxed":true,"network":"off","enforcement":"full"}"#
        );
    }

    /// The closed set is enforced on read as well as on write, so a value this
    /// build does not know cannot be accepted as one it does.
    #[test]
    fn values_outside_the_vocabulary_are_refused() {
        for raw in [r#""on""#, r#""Off""#, r#""proxy""#, r#""""#] {
            assert!(
                serde_json::from_str::<NetworkTier>(raw).is_err(),
                "{raw} must not parse as a network tier"
            );
        }
        for raw in [r#""complete""#, r#""FULL""#, r#""unenforced""#] {
            assert!(
                serde_json::from_str::<Enforcement>(raw).is_err(),
                "{raw} must not parse as an enforcement level"
            );
        }
    }
}
