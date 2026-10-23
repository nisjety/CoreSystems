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

use crate::auth::WebPolicyClaim;

/// The operator baseline, as a [`PolicyLayer`] wire document.
pub const BASELINE_ENV: &str = "QUARRY_EDGE__WEB_POLICY_BASELINE_JSON";
/// `off`, `observe` or `enforce`.
pub const ENFORCEMENT_ENV: &str = "QUARRY_EDGE__WEB_POLICY_ENFORCEMENT";
/// `1`/`true` or `0`/`false`. On: a request whose token has no `web_policy` claim is refused
/// (403 `policy_unreadable`) under `enforce`; under `observe` the refusal is only counted.
pub const REQUIRE_CLAIM_ENV: &str = "QUARRY_EDGE__WEB_POLICY_REQUIRE_CLAIM";

/// `1`/`true` or `0`/`false` (WS7.G). On: a page run preflights the final URL of a fetch
/// again when a redirect moved it off the requested URL, so a redirect to a host the policy
/// or the security engine refuses ends the run before anything is stored. Unset: on in a
/// development environment (`ENVIRONMENT` = dev, development, local or test), off elsewhere,
/// so a production deployment turns it on deliberately after the corpus of known-good
/// redirects (http to https, www) has been checked.
pub const FINAL_URL_CHECK_ENV: &str = "QUARRY_EDGE__WEB_POLICY_FINAL_URL_CHECK";

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
    require_claim: bool,
    final_url_check: bool,
}

impl Default for PolicyBaseline {
    /// No operator layer, `enforce`. With no ask this reproduces today's behaviour.
    fn default() -> Self {
        Self {
            layer: PolicyLayer::default(),
            enforcement: Enforcement::Enforce,
            require_claim: false,
            final_url_check: false,
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
        let require_claim = match lookup(REQUIRE_CLAIM_ENV) {
            Some(raw) => parse_flag(&raw)?,
            None => false,
        };
        let final_url_check = match lookup(FINAL_URL_CHECK_ENV) {
            Some(raw) => {
                parse_flag(&raw).map_err(|_| format!("{FINAL_URL_CHECK_ENV} must be 1 or 0"))?
            }
            None => lookup("ENVIRONMENT").is_some_and(|name| is_development(&name)),
        };
        Ok(Self {
            layer,
            enforcement,
            require_claim,
            final_url_check,
        })
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

    /// Whether a page run rechecks the final URL of a redirected fetch (WS7.G).
    pub fn final_url_check(&self) -> bool {
        self.final_url_check
    }

    /// Whether a token without the `web_policy` claim is refused (WS7.J).
    pub fn requires_claim(&self) -> bool {
        self.require_claim
    }

    /// WS7.J: the org layer for one request, read from the token's `web_policy` claim.
    ///
    /// A readable claim is the org layer. An absent claim is no org layer, unless the baseline
    /// requires the claim, which refuses the request. An unreadable claim is `Unreadable`: it
    /// refuses under `enforce` and is only counted under `observe`.
    pub fn org_input(&self, claim: Result<Option<WebPolicyClaim>, PolicyDenial>) -> OrgInput {
        match claim {
            Ok(Some(claim)) => OrgInput::Present {
                layer: claim.layer,
                rev: claim.rev,
            },
            Ok(None) if self.require_claim => OrgInput::Unreadable,
            Ok(None) => OrgInput::Absent,
            Err(_) => OrgInput::Unreadable,
        }
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
        self.decide_with_org(principal, zdr, OrgInput::Absent, request)
    }

    /// [`PolicyBaseline::decide`] with the org layer given (WS7.J: from the token's claim).
    pub fn decide_with_org(
        &self,
        principal: Principal,
        zdr: bool,
        org: OrgInput,
        request: Option<PolicyLayer>,
    ) -> Result<Decision, PolicyDenial> {
        let full = PolicyInputs {
            principal,
            zdr,
            baseline: self.layer.clone(),
            org,
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
                shadow_denial: None,
            }),
            Enforcement::Observe => {
                // The shadow never refuses the run: a shadow that cannot be resolved (an
                // unreadable claim) is kept as the would-be denial and counted by the edge.
                let (shadow, shadow_denial) = match resolve(&full) {
                    Ok(shadow) => (Some(shadow), None),
                    Err(denial) => (None, Some(denial)),
                };
                Ok(Decision {
                    policy: resolve(&relaxed)?,
                    shadow,
                    shadow_denial,
                })
            }
            Enforcement::Off => Ok(Decision {
                policy: resolve(&relaxed)?,
                shadow: None,
                shadow_denial: None,
            }),
        }
    }
}

/// `1`/`true` or `0`/`false`, case-insensitive. Anything else fails the boot.
fn parse_flag(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(format!("{REQUIRE_CLAIM_ENV} must be 1 or 0")),
    }
}

/// A development environment name, in any case.
fn is_development(name: &str) -> bool {
    ["dev", "development", "local", "test"]
        .iter()
        .any(|dev| name.trim().eq_ignore_ascii_case(dev))
}

/// The decision for one request: the policy the run runs under, and under
/// `observe` the policy `enforce` would have applied.
#[derive(Debug, Clone)]
pub struct Decision {
    policy: ResolvedPolicy,
    shadow: Option<ResolvedPolicy>,
    shadow_denial: Option<PolicyDenial>,
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

    /// Under `observe`, the denial that `enforce` would have raised when the shadow itself
    /// cannot be resolved (an unreadable org claim). Never enforced.
    pub fn shadow_denial(&self) -> Option<&PolicyDenial> {
        self.shadow_denial.as_ref()
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
    fn require_claim_flag_defaults_off_and_parses_strictly() {
        assert!(!PolicyBaseline::from_lookup(|_| None).unwrap().requires_claim());
        let on = |_: &str| Some("1".to_owned());
        assert!(PolicyBaseline::from_lookup(on).unwrap().requires_claim());
        assert!(PolicyBaseline::from_lookup(|name| (name == REQUIRE_CLAIM_ENV).then(|| "TRUE".to_owned()))
            .unwrap()
            .requires_claim());
        assert!(PolicyBaseline::from_lookup(|name| (name == REQUIRE_CLAIM_ENV).then(|| "yes".to_owned())).is_err());
    }

    #[test]
    fn final_url_check_is_on_in_development_and_off_elsewhere_unless_set() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            PolicyBaseline::from_lookup(move |name| {
                pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned())
            })
        };
        assert!(!env(&[]).unwrap().final_url_check(), "unlabelled means off");
        assert!(!PolicyBaseline::default().final_url_check());
        assert!(env(&[("ENVIRONMENT", "dev")]).unwrap().final_url_check());
        assert!(!env(&[("ENVIRONMENT", "production")]).unwrap().final_url_check());
        assert!(env(&[("ENVIRONMENT", "production"), (FINAL_URL_CHECK_ENV, "1")])
            .unwrap()
            .final_url_check());
        assert!(!env(&[("ENVIRONMENT", "dev"), (FINAL_URL_CHECK_ENV, "0")])
            .unwrap()
            .final_url_check());
        assert!(env(&[(FINAL_URL_CHECK_ENV, "maybe")]).is_err());
    }

    #[test]
    fn unset_variables_mean_enforce_and_no_layer() {
        let baseline = PolicyBaseline::from_lookup(|_| None).unwrap();
        assert_eq!(baseline.enforcement(), Enforcement::Enforce);
        assert_eq!(baseline.baseline(), &PolicyLayer::default());
    }
}
