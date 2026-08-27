//! Sovereignty posture for Data Plane v2 retrieval requests.
//!
//! `RetrieveRequest.sovereign_required` (proto field 14) is the counterpart to
//! `zdr_mode`: ZDR is a promise about RETENTION (does the provider keep the
//! content), sovereignty is a promise about JURISDICTION (which soil the
//! processing happens on). A provider can satisfy one without the other.
//!
//! Data Plane v2 resolves an absent value as `true` — a deliberate fail-closed
//! default, because absence of proof is not proof that egress is permitted.
//! Until field 14 existed no gRPC caller could say anything on this axis, so
//! every Model Plane retrieval over gRPC resolved to `sovereign_required =
//! true`, which Azure-hosted Cohere Embed v4 can never satisfy: every dense
//! query failed before retrieval ran. This module is how the Model Plane says
//! something.
//!
//! Every value produced here is `Some`. Sending nothing is the defect being
//! fixed, so the API cannot express it.

/// What the Model Plane sends when NOTHING on the request says anything about
/// sovereignty — no signed `sovereign` claim, no caller-declared value.
///
/// This is `false`, and that is a real decision rather than an oversight:
///
/// * `true` is what the wire already meant by silence, and it is what took the
///   dense arm down. Restating it here would ship the outage as a constant.
/// * Leaving the field absent is byte-identical to `true` at Data Plane v2, so
///   it is the same outage with less honesty about it.
/// * `false` says what is actually true of a Model Plane request today: the
///   plane holds no attestation that this content is sovereignty-restricted.
///
/// It is not a blanket opt-out. It is only reached when BOTH inputs are silent:
///
/// * a signed `sovereign = true` claim still floors the value at `true` and
///   cannot be relaxed by any request field, and
/// * a caller that declares the axis for one request still wins over this.
///
/// Sovereignty enforcement for MODEL serving is a separate, live mechanism —
/// `PrivacyTier`/`Residency` in inference-core — and is unaffected by this. The
/// gap this default admits is narrow and specific: a caller whose credential
/// never mentions sovereignty and who declares nothing gets non-sovereign
/// retrieval EMBEDDING. Close it by having auth-core mint the `sovereign`
/// claim (Control Plane work), at which point this constant stops being
/// reachable for any principal that has one.
pub const SOVEREIGN_REQUIRED_WITHOUT_SIGNAL: bool = false;

/// `PrivacyTier::PRIVACY_TIER_SOVEREIGN` on the shared `model_plane.v1` wire.
/// Duplicated as a plain integer rather than imported so this module stays
/// usable from crates that do not pull in the generated enum; the test below
/// pins the two together.
const PRIVACY_TIER_SOVEREIGN: i32 = 4;

/// Merge a verified sovereignty claim with a caller-declared value into the
/// single boolean the Data Plane wire carries.
///
/// Monotonic, mirroring the `AuthContext::effective_sovereign_required` that
/// retrieval-engine-rs applies on receipt, so both sides of the call agree on
/// what a value means:
///
/// * `claim = Some(true)` — signed floor. Wins outright; no request field
///   relaxes it.
/// * a declared `requested` — the caller speaking for its own data, which is
///   exactly what the field is for. May opt IN to the stricter posture on top
///   of a signed `Some(false)`.
/// * `claim = Some(false)` with nothing requested — signed as not required.
/// * both silent — [`SOVEREIGN_REQUIRED_WITHOUT_SIGNAL`].
///
/// Nothing here can produce `false` out of a signed `true`, which is the one
/// property worth checking when reading this.
#[must_use]
pub const fn effective_sovereign_required(claim: Option<bool>, requested: Option<bool>) -> bool {
    match (claim, requested) {
        (Some(true), _) => true,
        (_, Some(declared)) => declared,
        (Some(false), None) => false,
        (None, None) => SOVEREIGN_REQUIRED_WITHOUT_SIGNAL,
    }
}

/// Read a caller-declared sovereignty posture out of a request's minimum
/// privacy tier.
///
/// Only the sovereign tier produces an opinion. Tiers 1–3 are a FLOOR for
/// model serving ("at least this"), not a statement that sovereignty is
/// unnecessary, so reading them as `Some(false)` would let a privacy-tier
/// request quietly weaken a posture it never spoke about. `None` defers to the
/// signed claim, and to [`SOVEREIGN_REQUIRED_WITHOUT_SIGNAL`] below that.
#[must_use]
pub const fn sovereign_required_from_privacy_tier(min_privacy_tier: i32) -> Option<bool> {
    if min_privacy_tier == PRIVACY_TIER_SOVEREIGN {
        Some(true)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        effective_sovereign_required, sovereign_required_from_privacy_tier, PRIVACY_TIER_SOVEREIGN,
        SOVEREIGN_REQUIRED_WITHOUT_SIGNAL,
    };

    /// The property the whole module exists to hold. A signed sovereign claim
    /// is a floor, so no caller-declared value may lower it — including the
    /// `Some(false)` that the no-signal default would otherwise produce.
    #[test]
    fn a_signed_sovereign_claim_cannot_be_relaxed_by_any_request_value() {
        for requested in [None, Some(false), Some(true)] {
            assert!(
                effective_sovereign_required(Some(true), requested),
                "requested={requested:?} lowered a signed sovereign floor"
            );
        }
    }

    /// The opposite direction is allowed on purpose: a principal signed as not
    /// sovereignty-restricted may still declare the stricter posture for one
    /// request, which is what the wire field is for.
    #[test]
    fn a_caller_may_opt_in_to_sovereignty_over_an_unrestricted_claim() {
        assert!(effective_sovereign_required(Some(false), Some(true)));
        assert!(effective_sovereign_required(None, Some(true)));
        assert!(!effective_sovereign_required(Some(false), None));
        assert!(!effective_sovereign_required(Some(false), Some(false)));
    }

    /// Silence at both levels lands on the declared default and nowhere else.
    /// Pinned as its own test so changing that constant is a deliberate act
    /// with a failing test attached, not a quiet edit.
    #[test]
    fn total_silence_lands_on_the_declared_no_signal_default() {
        assert_eq!(
            effective_sovereign_required(None, None),
            SOVEREIGN_REQUIRED_WITHOUT_SIGNAL
        );
        assert!(
            !SOVEREIGN_REQUIRED_WITHOUT_SIGNAL,
            "flipping this to true reproduces the outage this field was added to fix: Data \
             Plane v2 cannot serve a dense query under sovereignty with an Azure-hosted \
             embedding provider"
        );
    }

    /// A privacy floor below sovereign says nothing about jurisdiction, so it
    /// must not answer for the axis at all. Reading tier 1–3 as `Some(false)`
    /// would let an ordinary chat request out-vote a signed claim's absence.
    #[test]
    fn only_the_sovereign_tier_speaks_for_the_sovereignty_axis() {
        for tier in [0, 1, 2, 3, -1, 99] {
            assert_eq!(
                sovereign_required_from_privacy_tier(tier),
                None,
                "tier {tier} must defer, not decide"
            );
        }
        assert_eq!(
            sovereign_required_from_privacy_tier(PRIVACY_TIER_SOVEREIGN),
            Some(true)
        );
    }

    /// The numeric above is a copy of a generated enum discriminant; pin it to
    /// the generated value so a renumbering in `inference.proto` fails here
    /// rather than silently disarming the tier→sovereignty derivation.
    #[test]
    fn the_sovereign_tier_numeric_matches_the_generated_enum() {
        assert_eq!(
            PRIVACY_TIER_SOVEREIGN,
            crate::model_plane::v1::PrivacyTier::Sovereign as i32
        );
    }
}
