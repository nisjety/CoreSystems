//! WS7.C: the operator web-policy baseline and the enforcement mode for the scrape family.
//!
//! The baseline is an operator layer read once at boot from
//! `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON`, a [`PolicyLayer`] wire document (unset
//! means no layer). `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` picks what a decision does:
//!
//! * `enforce` (the default): the baseline and the request's `policy` ask both
//!   apply; a denial is a 403.
//! * `observe`: the run proceeds exactly as under `off`, and the decision that
//!   `enforce` would take is computed beside it (the shadow) and counted.
//! * `off`: the baseline and the ask are ignored. The invariants and the ZDR floor
//!   still apply, because they are not layers an operator or a caller can turn off.
//!
//! With the default (empty) baseline and no ask, `enforce` changes nothing: the
//! resolution is the catalogue default and its digest leaves the run fingerprint
//! unchanged (see `RunPolicy::from(&ResolvedPolicy)` in quarry-runtime).
//!
//! Errors name the environment variable, never its value.

use quarry_core::web_policy::{
    resolve, OrgInput, PolicyDenial, PolicyInputs, PolicyLayer, Principal, ResolvedPolicy,
};

/// The operator baseline, as a [`PolicyLayer`] wire document.
pub const BASELINE_ENV: &str = "QUARRY_EDGE__WEB_POLICY_BASELINE_JSON";
/// `off`, `observe` or `enforce`.
pub const ENFORCEMENT_ENV: &str = "QUARRY_EDGE__WEB_POLICY_ENFORCEMENT";

/// What a decision does with the policy it resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    Off,
    Observe,
    Enforce,
}

impl Enforcement {
    /// Parse the environment value. Only the three lowercase names are accepted.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "off" => Ok(Self::Off),
            "observe" => Ok(Self::Observe),
            "enforce" => Ok(Self::Enforce),
            _ => Err(format!("{ENFORCEMENT_ENV} must be off, observe or enforce")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Enforce => "enforce",
        }
    }
}

/// The boot-time configuration of web policy. Held on `AppState` as `policy`.
#[derive(Debug, Clone)]
pub struct PolicyBaseline {
    layer: PolicyLayer,
    enforcement: Enforcement,
}

impl Default for PolicyBaseline {
    /// No operator layer, `enforce`. With no ask this reproduces today's behaviour.
    fn default() -> Self {
        Self {
            layer: PolicyLayer::default(),
            enforcement: Enforcement::Enforce,
        }
    }
}

impl PolicyBaseline {
    /// Read both variables through `lookup`. An unparsable value fails the boot:
    /// a baseline that silently fell back to "no layer" would be a loosened policy.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let enforcement = match lookup(ENFORCEMENT_ENV) {
            Some(raw) => Enforcement::parse(&raw)?,
            None => Enforcement::Enforce,
        };
        let layer = match lookup(BASELINE_ENV) {
            Some(raw) if !raw.trim().is_empty() => serde_json::from_str::<PolicyLayer>(&raw)
                .map_err(|_| format!("{BASELINE_ENV} is not a valid web policy layer"))?,
            _ => PolicyLayer::default(),
        };
        Ok(Self { layer, enforcement })
    }

    /// [`PolicyBaseline::from_lookup`] over the process environment.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn enforcement(&self) -> Enforcement {
        self.enforcement
    }

    pub fn baseline(&self) -> &PolicyLayer {
        &self.layer
    }

    /// Resolve the policy for one request. `request` is the ask already converted
    /// to a layer (the shape is checked by the edge before this call).
    ///
    /// * `enforce`: the run runs under the full resolution; no shadow.
    /// * `observe`: the run runs under the relaxed resolution; the full resolution
    ///   is the shadow, checked and counted by the edge.
    /// * `off`: the run runs under the relaxed resolution; no shadow.
    ///
    /// The relaxed resolution is the invariants and the ZDR floor alone.
    pub fn decide(
        &self,
        principal: Principal,
        zdr: bool,
        request: Option<PolicyLayer>,
    ) -> Result<Decision, PolicyDenial> {
        let full = PolicyInputs {
            principal,
            zdr,
            baseline: self.layer.clone(),
            org: OrgInput::Absent,
            grant: None,
            request,
        };
        let relaxed = PolicyInputs {
            principal,
            zdr,
            baseline: PolicyLayer::default(),
            org: OrgInput::Absent,
            grant: None,
            request: None,
        };
        match self.enforcement {
            Enforcement::Enforce => Ok(Decision {
                policy: resolve(&full)?,
                shadow: None,
            }),
            Enforcement::Observe => Ok(Decision {
                policy: resolve(&relaxed)?,
                shadow: Some(resolve(&full)?),
            }),
            Enforcement::Off => Ok(Decision {
                policy: resolve(&relaxed)?,
                shadow: None,
            }),
        }
    }
}

/// The decision for one request: the policy the run runs under, and under
/// `observe` the policy `enforce` would have applied.
#[derive(Debug, Clone)]
pub struct Decision {
    policy: ResolvedPolicy,
    shadow: Option<ResolvedPolicy>,
}

impl Decision {
    /// The policy the run runs under.
    pub fn policy(&self) -> &ResolvedPolicy {
        &self.policy
    }

    /// Under `observe`, the policy that `enforce` would have applied. Never enforced.
    pub fn shadow(&self) -> Option<&ResolvedPolicy> {
        self.shadow.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforcement_accepts_only_the_three_names() {
        assert_eq!(Enforcement::parse("off").unwrap(), Enforcement::Off);
        assert_eq!(Enforcement::parse(" observe ").unwrap(), Enforcement::Observe);
        assert_eq!(Enforcement::parse("enforce").unwrap(), Enforcement::Enforce);
        assert!(Enforcement::parse("ENFORCE").is_err());
        assert!(Enforcement::parse("sometimes").is_err());
    }

    #[test]
    fn unset_variables_mean_enforce_and_no_layer() {
        let baseline = PolicyBaseline::from_lookup(|_| None).unwrap();
        assert_eq!(baseline.enforcement(), Enforcement::Enforce);
        assert_eq!(baseline.baseline(), &PolicyLayer::default());
    }
}
