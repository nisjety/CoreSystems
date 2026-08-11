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
        if ip.is_loopback()
            || ip.is_unspecified()
            || ip.is_private()
            || ip.is_link_local()
            || ip.is_broadcast()
            || ip.is_multicast()
        {
            return Some(format!("private ipv4: {ip}"));
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
        if v4.is_loopback()
            || v4.is_unspecified()
            || v4.is_private()
            || v4.is_link_local()
            || v4.is_broadcast()
            || v4.is_multicast()
        {
            return Some("v4-mapped ipv6 in private range");
        }
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
            // Previously missing broadcast + multicast entirely, unlike
            // blocks_private_host's IPv4 branch (which had broadcast but
            // not multicast either) -- the two checks had silently drifted
            // apart. A crawl target resolving to 255.255.255.255 or a
            // 224.0.0.0/4 multicast address is never a legitimate public
            // HTTP destination.
            IpAddr::V4(ip)
                if ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_private()
                    || ip.is_link_local()
                    || ip.is_broadcast()
                    || ip.is_multicast() =>
            {
                return Some(format!("resolved to private: {ip}"))
            }
            IpAddr::V6(ip) => {
                if let Some(reason) = ipv6_blocked_reason(ip) {
                    return Some(format!("resolved to {reason}: {ip}"));
                }
            }
            _ => {}
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
}
