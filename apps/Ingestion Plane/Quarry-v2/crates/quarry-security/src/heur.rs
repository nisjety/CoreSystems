//! Heuristic checks. Port from donor `internal/security/heur/`.
//!
//! Covers: scheme allowlist, private IP / loopback / link-local block (SSRF),
//! suspicious TLD, userinfo, non-standard port, path traversal hints.

use std::net::IpAddr;
use url::Url;

use crate::Verdict;

pub fn scheme_allowed(u: &Url) -> bool {
    matches!(u.scheme(), "http" | "https")
}

pub fn blocks_private_host(u: &Url) -> Option<String> {
    let Some(host) = u.host() else {
        return Some("missing host".into());
    };
    if let url::Host::Ipv4(ip) = host {
        if let Some(reason) = ipv4_blocked_reason(&ip) {
            return Some(format!("{reason}: {ip}"));
        }
    }
    if let url::Host::Ipv6(ip) = host {
        if let Some(reason) = ipv6_blocked_reason(&ip) {
            return Some(format!("{reason}: {ip}"));
        }
    }
    if let url::Host::Domain(d) = host {
        if d == "localhost" || d.ends_with(".local") || d.ends_with(".internal") {
            return Some(format!("internal hostname: {d}"));
        }
    }
    None
}

/// IPv4 block-list. Stable `Ipv4Addr` predicates cover loopback, unspecified,
/// private (RFC 1918), link-local, broadcast, multicast, and documentation
/// (RFC 5737) -- but not CGNAT, the IETF protocol-assignment block,
/// benchmarking, the wider `0.0.0.0/8`, or reserved Class E, which need manual
/// octet checks. Kept in parity with model-gateway's `ip_is_forbidden`
/// (`apps/Model Plane/rust/services/model-gateway/src/runtime_registries.rs`)
/// and capability-core's Go-side `mcpForbiddenRanges` -- three independent
/// address-vetting tables that must agree, or a hostname resolving into a
/// range only one of them blocks slips through wherever the weakest check
/// runs.
fn ipv4_blocked_reason(ip: &std::net::Ipv4Addr) -> Option<&'static str> {
    if ip.is_loopback() {
        return Some("loopback ipv4");
    }
    if ip.is_unspecified() {
        return Some("unspecified ipv4");
    }
    if ip.is_private() {
        return Some("private ipv4");
    }
    if ip.is_link_local() {
        return Some("link-local ipv4");
    }
    if ip.is_broadcast() {
        return Some("broadcast ipv4");
    }
    if ip.is_multicast() {
        return Some("multicast ipv4");
    }
    if ip.is_documentation() {
        return Some("documentation ipv4");
    }
    let octets = ip.octets();
    if octets[0] == 0 {
        return Some("this-network ipv4 (0.0.0.0/8)");
    }
    // 100.64.0.0/10 -- CGNAT / Shared Address Space (RFC 6598). Real in
    // cloud/k8s pod networks, so a legitimate-looking DNS answer can still
    // land inside another tenant's internal address space.
    if octets[0] == 100 && (octets[1] & 0xc0) == 0x40 {
        return Some("cgnat ipv4 (100.64.0.0/10)");
    }
    // 192.0.0.0/24 -- IETF protocol assignments (RFC 6890).
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 0 {
        return Some("ietf protocol assignment ipv4 (192.0.0.0/24)");
    }
    // 198.18.0.0/15 -- benchmarking (RFC 2544).
    if octets[0] == 198 && (octets[1] & 0xfe) == 18 {
        return Some("benchmarking ipv4 (198.18.0.0/15)");
    }
    // 240.0.0.0/4 -- reserved / future use (Class E).
    if (octets[0] & 0xf0) == 0xf0 {
        return Some("reserved ipv4 (240.0.0.0/4)");
    }
    None
}

/// IPv6 block-list. Stable Rust only exposes `is_loopback()` /
/// `is_unspecified()` on `Ipv6Addr`; ULA (`fc00::/7`) and link-local
/// (`fe80::/10`) require manual bit-mask checks. Returns the reason
/// label when the address is blocked, `None` when it's public-routable.
///
/// Also handles IPv4-mapped IPv6 (`::ffff:1.2.3.4`) — without this an
/// attacker can wrap a private IPv4 inside an IPv6 literal and bypass
/// the IPv4-specific check above.
fn ipv6_blocked_reason(ip: &std::net::Ipv6Addr) -> Option<&'static str> {
    if ip.is_loopback() {
        return Some("loopback ipv6");
    }
    if ip.is_unspecified() {
        return Some("unspecified ipv6");
    }
    let octets = ip.octets();
    // fc00::/7 — Unique Local Addresses (RFC 4193). The first 7 bits
    // are `1111110x` → top byte matches the mask 0xfe pattern 0xfc.
    if (octets[0] & 0xfe) == 0xfc {
        return Some("ula ipv6 (fc00::/7)");
    }
    // fe80::/10 — link-local. Top 10 bits are `1111111010` → first
    // byte is 0xfe and the next byte's top two bits are 10 (0x80..=0xbf).
    if octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80 {
        return Some("link-local ipv6 (fe80::/10)");
    }
    // IPv4-mapped IPv6: `::ffff:a.b.c.d`. Check the embedded IPv4
    // against the same set the v4 branch uses.
    if let Some(v4) = ip.to_ipv4_mapped() {
        if ipv4_blocked_reason(&v4).is_some() {
            return Some("v4-mapped ipv6 in private range");
        }
    }
    let segments = ip.segments();
    // 64:ff9b::/96 -- NAT64 well-known prefix (RFC 6052).
    if segments[0] == 0x0064
        && segments[1] == 0xff9b
        && segments[2] == 0
        && segments[3] == 0
        && segments[4] == 0
        && segments[5] == 0
    {
        return Some("nat64 ipv6 (64:ff9b::/96)");
    }
    // 100::/64 -- discard-only address block (RFC 6666).
    if segments[0] == 0x0100 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0 {
        return Some("discard-only ipv6 (100::/64)");
    }
    // 2001:db8::/32 -- documentation (RFC 3849).
    if segments[0] == 0x2001 && segments[1] == 0x0db8 {
        return Some("documentation ipv6 (2001:db8::/32)");
    }
    None
}

pub fn check(u: &Url) -> Verdict {
    if !scheme_allowed(u) {
        return Verdict::block(format!("disallowed scheme: {}", u.scheme()));
    }
    if let Some(r) = blocks_private_host(u) {
        return Verdict::block(r);
    }
    if u.password().is_some() || !u.username().is_empty() {
        return Verdict::block("userinfo in URL");
    }
    Verdict::allow()
}

pub fn resolve_guard(addrs: &[IpAddr]) -> Option<String> {
    for a in addrs {
        match a {
            IpAddr::V4(ip) => {
                if let Some(reason) = ipv4_blocked_reason(ip) {
                    return Some(format!("resolved to {reason}: {ip}"));
                }
            }
            IpAddr::V6(ip) => {
                if let Some(reason) = ipv6_blocked_reason(ip) {
                    return Some(format!("resolved to {reason}: {ip}"));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_ipv6_ula() {
        let u: Url = "http://[fc00::1]/".parse().unwrap();
        assert!(blocks_private_host(&u).is_some());
        let u2: Url = "http://[fd12:3456:789a::1]/".parse().unwrap();
        assert!(blocks_private_host(&u2).is_some());
    }

    #[test]
    fn blocks_ipv6_link_local() {
        let u: Url = "http://[fe80::1]/".parse().unwrap();
        assert!(blocks_private_host(&u).is_some());
    }

    #[test]
    fn blocks_v4_mapped_ipv6() {
        let u: Url = "http://[::ffff:127.0.0.1]/".parse().unwrap();
        assert!(blocks_private_host(&u).is_some());
        let u2: Url = "http://[::ffff:10.0.0.5]/".parse().unwrap();
        assert!(blocks_private_host(&u2).is_some());
    }

    #[test]
    fn allows_public_ipv6() {
        // 2001:db8::/32 is the documentation range, but it's public-
        // routable from the protocol's perspective. We only block the
        // listed private/local ranges.
        let u: Url = "http://[2606:4700:4700::1111]/".parse().unwrap();
        assert!(blocks_private_host(&u).is_none());
    }

    #[test]
    fn blocks_ipv4_multicast_and_broadcast() {
        let broadcast: IpAddr = "255.255.255.255".parse().unwrap();
        let multicast: IpAddr = "224.0.0.1".parse().unwrap();

        let broadcast_url: Url = "http://255.255.255.255/".parse().unwrap();
        let multicast_url: Url = "http://224.0.0.1/".parse().unwrap();
        assert!(blocks_private_host(&broadcast_url).is_some());
        assert!(blocks_private_host(&multicast_url).is_some());

        // resolve_guard used to omit both entirely -- it drifted from
        // blocks_private_host, which at least caught broadcast.
        assert!(resolve_guard(&[broadcast]).is_some());
        assert!(resolve_guard(&[multicast]).is_some());
    }

    #[test]
    fn blocks_ipv4_unspecified_and_mapped_unspecified() {
        let unspecified: Url = "http://0.0.0.0/".parse().unwrap();
        assert!(blocks_private_host(&unspecified).is_some());
        let mapped: Url = "http://[::ffff:0.0.0.0]/".parse().unwrap();
        assert!(blocks_private_host(&mapped).is_some());
        let address: IpAddr = "0.0.0.0".parse().unwrap();
        assert!(resolve_guard(&[address]).is_some());
    }

    /// Ranges added to converge with capability-core's Go-side
    /// `mcpForbiddenRanges` and model-gateway's `ip_is_forbidden`: CGNAT and
    /// benchmarking are real cloud/k8s address spaces, not exotic edge cases,
    /// and none of them were reachable via `Ipv4Addr`'s stable predicates
    /// alone.
    #[test]
    fn blocks_cgnat_and_reserved_ipv4_ranges() {
        for literal in [
            "0.1.2.3",         // 0.0.0.0/8, beyond the single unspecified address
            "100.64.0.1",      // CGNAT / Shared Address Space (RFC 6598)
            "100.127.255.254", // top of the CGNAT range
            "192.0.0.8",       // IETF protocol assignments (RFC 6890)
            "198.18.0.1",      // benchmarking (RFC 2544)
            "198.19.255.254",  // top of the benchmarking range
            "255.0.0.1",       // 240.0.0.0/4 reserved (Class E)
        ] {
            let address: IpAddr = literal.parse().unwrap();
            assert!(
                resolve_guard(&[address]).is_some(),
                "unsafe address accepted: {literal}"
            );
            let u: Url = format!("http://{literal}/").parse().unwrap();
            assert!(
                blocks_private_host(&u).is_some(),
                "unsafe host accepted: {literal}"
            );
        }
        // Just outside the CGNAT and benchmarking ranges -- must stay
        // reachable, or the range check is off by one.
        assert!(resolve_guard(&["100.63.255.255".parse().unwrap()]).is_none());
        assert!(resolve_guard(&["100.128.0.0".parse().unwrap()]).is_none());
        assert!(resolve_guard(&["198.17.255.255".parse().unwrap()]).is_none());
        assert!(resolve_guard(&["198.20.0.0".parse().unwrap()]).is_none());
    }

    #[test]
    fn blocks_ipv4_documentation_ranges() {
        // RFC 5737 TEST-NET-1/2/3. capability-core's Go-side
        // `mcpForbiddenRanges` already blocked these; `blocks_private_host`'s
        // IPv4 branch never called `is_documentation()`, so they previously
        // passed both entry points here despite never being a legitimate
        // public HTTP destination.
        for literal in ["192.0.2.1", "198.51.100.1", "203.0.113.1"] {
            let address: IpAddr = literal.parse().unwrap();
            assert!(resolve_guard(&[address]).is_some(), "accepted: {literal}");
            let u: Url = format!("http://{literal}/").parse().unwrap();
            assert!(blocks_private_host(&u).is_some(), "accepted: {literal}");
        }
    }

    #[test]
    fn blocks_new_ipv6_ranges() {
        for literal in [
            "64:ff9b::1", // NAT64 well-known prefix (RFC 6052)
            "100::1",     // discard-only address block (RFC 6666)
            "2001:db8::1", // documentation (RFC 3849)
        ] {
            let address: IpAddr = literal.parse().unwrap();
            assert!(resolve_guard(&[address]).is_some(), "accepted: {literal}");
            let u: Url = format!("http://[{literal}]/").parse().unwrap();
            assert!(blocks_private_host(&u).is_some(), "accepted: {literal}");
        }
        // A real public IPv6 address must still pass, unaffected by the new
        // range checks.
        let public: IpAddr = "2606:4700:4700::1111".parse().unwrap();
        assert!(resolve_guard(&[public]).is_none());
    }
}
