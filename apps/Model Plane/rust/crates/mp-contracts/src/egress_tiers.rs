//! Sandbox egress tiers (EP-9 D-E3) and the hosts each one admits (E3).
//!
//! # The vocabulary
//!
//! A closed, ordered set: `off < registries < allowlist < all`. Each tier
//! reaches strictly more of the network than the one before, so the tier a
//! call runs at is the minimum of every ceiling that applies to it (the
//! deployment switch, the lease, the run). A word this build does not know is
//! `off`, never a wider reach, because every reader here is deciding how much
//! network to grant.
//!
//! `all` is in the vocabulary so the wire can name it, but nothing grants it in
//! Wave 3: Control refuses to store it, sandbox-manager records it as `off`,
//! and the deployment switch reads it as `off`.
//!
//! # The hosts
//!
//! `registries` admits [`REGISTRY_HOSTS`], a short fixed list kept small on
//! purpose: a CONNECT proxy sees the host, not the path, so every listed host
//! is a host data can leave through. `allowlist` admits those plus the Space's
//! own list from Control. Matching is exact on the canonical host name: no
//! wildcard, no suffix match, no IP literal.

use crate::sandbox_report::NetworkTier;

/// Package registries the `registries` tier admits. Reviewed list; adding a
/// host widens every Space at that tier, so it is a ledger change, not a tweak.
pub const REGISTRY_HOSTS: &[&str] = &["pypi.org", "files.pythonhosted.org", "registry.npmjs.org"];

/// The most hosts a Space allow-list may hold. Control enforces the same bound
/// on write (E2); a longer list read here is refused as a whole.
pub const MAX_ALLOWLIST_HOSTS: usize = 32;

/// The legacy capability claim, which Control and sandbox-manager read as `off`.
/// The claim for `off` stays this word so a deployment with egress switched off
/// presents exactly the claim it did before tiers existed.
pub const LEGACY_OFF_CLAIM: &str = "disabled_by_default";

/// The position of `tier` in the order of reach.
#[must_use]
pub const fn rank(tier: NetworkTier) -> u8 {
    match tier {
        NetworkTier::Off => 0,
        NetworkTier::Registries => 1,
        NetworkTier::Allowlist => 2,
        NetworkTier::All => 3,
    }
}

/// The narrower of two tiers.
#[must_use]
pub const fn min_tier(a: NetworkTier, b: NetworkTier) -> NetworkTier {
    if rank(a) <= rank(b) {
        a
    } else {
        b
    }
}

/// A tier word as Control and sandbox-manager spell it (`off`, `registries`,
/// `allowlist`, `all`, or the legacy `disabled_by_default`). Anything else,
/// including a different case, is `off`.
#[must_use]
pub fn parse_wire(raw: &str) -> NetworkTier {
    match raw {
        "registries" => NetworkTier::Registries,
        "allowlist" => NetworkTier::Allowlist,
        "all" => NetworkTier::All,
        _ => NetworkTier::Off,
    }
}

/// The deployment switch (`EXECUTION_SANDBOX_EGRESS`). Only the exact words
/// `registries` and `allowlist` raise it; unset, empty, `all`, a typo or a
/// different case leave it `off`, because this switch grants egress.
#[must_use]
pub fn parse_switch(raw: Option<&str>) -> NetworkTier {
    match raw {
        Some("registries") => NetworkTier::Registries,
        Some("allowlist") => NetworkTier::Allowlist,
        _ => NetworkTier::Off,
    }
}

/// The `egress` value a backend claims for `tier` in its capability profile.
#[must_use]
pub const fn claim(tier: NetworkTier) -> &'static str {
    match tier {
        NetworkTier::Off => LEGACY_OFF_CLAIM,
        NetworkTier::Registries => "registries",
        NetworkTier::Allowlist => "allowlist",
        NetworkTier::All => "all",
    }
}

/// The canonical form of a host name, or `None` when `raw` is not one: ASCII
/// letters, digits and hyphens in dot-separated labels of 1 to 63 characters,
/// at least two labels, at most 253 characters, lower-cased, one trailing dot
/// dropped. An IP literal is not a host name (its last label is numeric, or it
/// holds a colon), so it is refused here and never reaches a resolver.
#[must_use]
pub fn canonical_host(raw: &str) -> Option<String> {
    let host = raw.strip_suffix('.').unwrap_or(raw).to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return None;
    }
    let valid_label = |label: &&str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    };
    if !labels.iter().all(valid_label) {
        return None;
    }
    // A top-level label is never all digits; one that is makes this an IPv4 literal.
    if labels
        .last()
        .is_some_and(|tld| tld.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    Some(host)
}

/// Whether a call at `tier` may reach `host`. `allowlist` is the Space's list
/// as Control returned it; entries that are not canonical host names never
/// match. `host` must already be canonical ([`canonical_host`]).
#[must_use]
pub fn host_admitted(tier: NetworkTier, host: &str, allowlist: &[String]) -> bool {
    let registry = || REGISTRY_HOSTS.contains(&host);
    match tier {
        NetworkTier::Off => false,
        NetworkTier::Registries => registry(),
        NetworkTier::Allowlist => {
            registry()
                || (allowlist.len() <= MAX_ALLOWLIST_HOSTS
                    && allowlist
                        .iter()
                        .any(|entry| canonical_host(entry).as_deref() == Some(host)))
        }
        // Not storable in Wave 3; nothing produces it, and it is named here
        // only so the match is total.
        NetworkTier::All => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIERS: [NetworkTier; 4] = [
        NetworkTier::Off,
        NetworkTier::Registries,
        NetworkTier::Allowlist,
        NetworkTier::All,
    ];

    #[test]
    fn the_order_of_reach_is_strict() {
        for pair in TIERS.windows(2) {
            assert!(rank(pair[0]) < rank(pair[1]));
        }
    }

    #[test]
    fn min_tier_takes_the_narrower_whatever_the_order() {
        for a in TIERS {
            for b in TIERS {
                let narrower = min_tier(a, b);
                assert_eq!(narrower, min_tier(b, a));
                assert_eq!(rank(narrower), rank(a).min(rank(b)));
            }
        }
    }

    #[test]
    fn the_switch_is_raised_only_by_its_two_exact_words() {
        assert_eq!(parse_switch(Some("registries")), NetworkTier::Registries);
        assert_eq!(parse_switch(Some("allowlist")), NetworkTier::Allowlist);
        for raw in [
            None,
            Some(""),
            Some("off"),
            Some("all"),
            Some("true"),
            Some("Registries"),
            Some(" registries"),
            Some("registries "),
        ] {
            assert_eq!(parse_switch(raw), NetworkTier::Off, "{raw:?}");
        }
    }

    #[test]
    fn an_unknown_wire_word_is_off() {
        assert_eq!(parse_wire("registries"), NetworkTier::Registries);
        assert_eq!(parse_wire("allowlist"), NetworkTier::Allowlist);
        assert_eq!(parse_wire("all"), NetworkTier::All);
        for raw in ["off", LEGACY_OFF_CLAIM, "", "ALL", "open", "allow_list"] {
            assert_eq!(parse_wire(raw), NetworkTier::Off, "{raw}");
        }
    }

    #[test]
    fn the_off_claim_is_the_legacy_word_and_the_others_round_trip() {
        assert_eq!(claim(NetworkTier::Off), LEGACY_OFF_CLAIM);
        for tier in TIERS {
            assert_eq!(parse_wire(claim(tier)), tier);
        }
    }

    #[test]
    fn canonical_host_lowercases_and_refuses_what_is_not_a_name() {
        assert_eq!(canonical_host("PyPI.org.").as_deref(), Some("pypi.org"));
        assert_eq!(
            canonical_host("files.pythonhosted.org").as_deref(),
            Some("files.pythonhosted.org")
        );
        for raw in [
            "",
            "localhost",
            "10.0.0.1",
            "169.254.169.254",
            "[::1]",
            "::1",
            "pypi.org:443",
            "user@pypi.org",
            "*.pypi.org",
            "-bad.org",
            "bad-.org",
            "a..org",
            "pypi.org/simple",
            "pyp\u{ed}.org",
        ] {
            assert!(canonical_host(raw).is_none(), "{raw} must not be a host");
        }
        assert!(canonical_host(&format!("{}.org", "a".repeat(64))).is_none());
    }

    #[test]
    fn off_admits_nothing() {
        for host in REGISTRY_HOSTS {
            assert!(!host_admitted(NetworkTier::Off, host, &[]));
        }
    }

    #[test]
    fn registries_admits_exactly_the_reviewed_list() {
        for host in REGISTRY_HOSTS {
            assert!(host_admitted(NetworkTier::Registries, host, &[]));
        }
        for host in [
            "example.org",
            "evil.pypi.org",
            "pypi.org.evil.com",
            "npmjs.org",
        ] {
            assert!(!host_admitted(NetworkTier::Registries, host, &[]), "{host}");
        }
        // The Space list does not widen the registries tier.
        let list = vec!["example.org".to_owned()];
        assert!(!host_admitted(
            NetworkTier::Registries,
            "example.org",
            &list
        ));
    }

    #[test]
    fn allowlist_admits_the_registries_and_the_space_list_exactly() {
        let list = vec!["Data.Example.org".to_owned(), "10.0.0.1".to_owned()];
        assert!(host_admitted(NetworkTier::Allowlist, "pypi.org", &list));
        assert!(host_admitted(
            NetworkTier::Allowlist,
            "data.example.org",
            &list
        ));
        assert!(!host_admitted(NetworkTier::Allowlist, "example.org", &list));
        assert!(!host_admitted(
            NetworkTier::Allowlist,
            "x.data.example.org",
            &list
        ));
        assert!(!host_admitted(NetworkTier::Allowlist, "10.0.0.1", &list));
    }

    #[test]
    fn an_over_long_space_list_admits_none_of_its_hosts() {
        let list: Vec<String> = (0..=MAX_ALLOWLIST_HOSTS)
            .map(|index| format!("host{index}.example.org"))
            .collect();
        assert!(!host_admitted(
            NetworkTier::Allowlist,
            "host0.example.org",
            &list
        ));
        // The registries are still reachable: the bound refuses the list, not the tier.
        assert!(host_admitted(NetworkTier::Allowlist, "pypi.org", &list));
    }
}
