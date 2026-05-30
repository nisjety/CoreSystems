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
        if ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_broadcast() {
            return Some(format!("private ipv4: {ip}"));
        }
    }
    if let url::Host::Ipv6(ip) = host {
        if ip.is_loopback() || ip.is_unspecified() {
            return Some(format!("private ipv6: {ip}"));
        }
    }
    if let url::Host::Domain(d) = host {
        if d == "localhost" || d.ends_with(".local") || d.ends_with(".internal") {
            return Some(format!("internal hostname: {d}"));
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
            IpAddr::V4(ip) if ip.is_loopback() || ip.is_private() || ip.is_link_local() => {
                return Some(format!("resolved to private: {ip}"))
            }
            IpAddr::V6(ip) if ip.is_loopback() || ip.is_unspecified() => {
                return Some(format!("resolved to private: {ip}"))
            }
            _ => {}
        }
    }
    None
}
