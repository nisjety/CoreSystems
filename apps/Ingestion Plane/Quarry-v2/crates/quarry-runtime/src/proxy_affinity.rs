//! Per-agent "dedicated cloud computer" proxy affinity (W1).
//!
//! Each agent run gets a deterministic `ProxyAffinity { pool, sticky_key }`
//! keyed on `(org_id, run_id, host)`. The pool is a per-agent identifier the
//! upstream `EgressBroker` uses to bound its candidate set; the sticky key
//! pins the same egress identity across the run, so the agent's outbound IP
//! stays stable while the run is alive.
//!
//! Privacy rule: when `PrivacyPolicy::allow_third_party_processing` is
//! `false`, the pool resolves to `egress.first_party` regardless of the
//! configured `proxy_pool` env. This is the dedicated-computer default:
//! an agent that is not allowed to dial a third-party processor may not
//! share that dial with any other agent on the same host.

use quarry_core::lease::ProxyAffinity;
use quarry_core::privacy::PrivacyPolicy;

/// Pool label for the dedicated first-party egress class. Operators
/// may also set this in `EgressBroker`'s catalog as a first-party
/// network policy. Any agent whose `PrivacyPolicy` denies
/// third-party processing is forced onto this pool.
pub const FIRST_PARTY_POOL: &str = "egress.first_party";

/// Default pool name when the org has approved third-party processors
/// (proxies / cloud-browser providers) and the operator has wired
/// the catalog entry.
pub const APPROVED_NETWORK_POOL: &str = "egress.approved_network";

/// Derive the `ProxyAffinity` for a given agent run.
///
/// Inputs:
/// - `org_id`: the verified tenant id from the agent's JWT.
/// - `run_id`: the agent run id; each run gets its own sticky key so
///   runs never share an outbound IP even when they target the same
///   host.
/// - `host`: the destination host. `host` may be the configured
///   `proxy_pool` env name (so a single proxy can be addressable as
///   one "host" for routing purposes) or a real DNS name.
/// - `policy`: the agent's privacy policy. When it denies third-party
///   processing, the affinity pool is forced to `egress.first_party`.
/// - `operator_pool`: the pool the operator wired via
///   `QUARRY_PROXY_POOL` (or `""` if none).
pub fn derive(
    org_id: &str,
    run_id: &str,
    host: &str,
    policy: &PrivacyPolicy,
    operator_pool: &str,
) -> ProxyAffinity {
    let pool = if !policy.allow_third_party_processing {
        FIRST_PARTY_POOL.to_string()
    } else if !operator_pool.is_empty() {
        operator_pool.to_string()
    } else {
        // Org approves third-party but operator has no proxy wired.
        // First-party is the safe default; an audit will surface the
        // missing `QUARRY_PROXY_POOL` env in the run events.
        FIRST_PARTY_POOL.to_string()
    };

    let sticky = sticky_key(org_id, run_id, host);
    ProxyAffinity {
        pool,
        sticky_key: Some(sticky),
    }
}

/// 16-byte hex BLAKE3 digest of `(org_id, run_id, host)`. The digest
/// is the per-run per-host identifier the EgressBroker / cloud-browser
/// provider should pin to. Keeping it short (16 bytes → 32 hex chars)
/// means it fits in HTTP headers and provider session-id paths.
pub fn sticky_key(org_id: &str, run_id: &str, host: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(org_id.as_bytes());
    hasher.update(b"\x00");
    hasher.update(run_id.as_bytes());
    hasher.update(b"\x00");
    hasher.update(host.as_bytes());
    let digest = hasher.finalize();
    let bytes = &digest.as_bytes()[..16];
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::privacy::PrivacyPolicy;

    #[test]
    fn sticky_key_is_stable_for_same_triple() {
        let a = sticky_key("org_a", "run_01", "example.com");
        let b = sticky_key("org_a", "run_01", "example.com");
        assert_eq!(a, b);
        assert_eq!(a.len(), 32, "expected 16-byte hex (32 chars)");
    }

    #[test]
    fn sticky_key_distinguishes_org_run_host() {
        let base = sticky_key("org_a", "run_01", "example.com");
        assert_ne!(base, sticky_key("org_b", "run_01", "example.com"));
        assert_ne!(base, sticky_key("org_a", "run_02", "example.com"));
        assert_ne!(base, sticky_key("org_a", "run_01", "other.com"));
    }

    #[test]
    fn first_party_pool_when_third_party_denied() {
        let policy = PrivacyPolicy {
            allow_third_party_processing: false,
            ..Default::default()
        };
        let aff = derive("org_a", "run_01", "example.com", &policy, "QUARRY_PROXY_POOL");
        assert_eq!(aff.pool, FIRST_PARTY_POOL);
        assert!(aff.sticky_key.is_some());
    }

    #[test]
    fn approved_pool_when_third_party_allowed_and_operator_wired() {
        let policy = PrivacyPolicy {
            allow_third_party_processing: true,
            ..Default::default()
        };
        let aff = derive("org_a", "run_01", "example.com", &policy, "QUARRY_PROXY_POOL");
        assert_eq!(aff.pool, "QUARRY_PROXY_POOL");
    }

    #[test]
    fn approved_pool_falls_back_to_first_party_when_no_operator_pool() {
        let policy = PrivacyPolicy {
            allow_third_party_processing: true,
            ..Default::default()
        };
        let aff = derive("org_a", "run_01", "example.com", &policy, "");
        assert_eq!(aff.pool, FIRST_PARTY_POOL);
    }

    #[test]
    fn deny_third_party_overrides_operator_pool() {
        // Even when the operator wired a third-party pool, a policy
        // that denies third-party processing MUST be honored. This is
        // the dedicated-computer invariant: no per-run egress
        // identity may ever leak to a third-party processor without
        // explicit, auditable approval.
        let policy = PrivacyPolicy {
            allow_third_party_processing: false,
            ..Default::default()
        };
        let aff = derive("org_a", "run_01", "example.com", &policy, "QUARRY_PROXY_POOL");
        assert_eq!(
            aff.pool, FIRST_PARTY_POOL,
            "third-party denied; first-party pool must win"
        );
    }
}
