//! SSRF-safe redirect policy for the TLS client.
//!
//! This module provides a builder for creating a `wreq::redirect::Policy` that validates
//! each redirect hop against RFC 1918 private IP ranges and other security constraints
//! before allowing the redirect to proceed.

use quarry_security::heur::resolve_guard;
use std::net::{IpAddr, ToSocketAddrs};
use url::Url;
use wreq::redirect::{Attempt, Policy};

/// Builder for an SSRF-safe redirect policy.
///
/// By default:
/// - Redirects to localhost/127.0.0.1 are **denied**
/// - Redirects to RFC 1918 private ranges are **denied**
/// - Redirects to link-local, multicast, and IPv6 private ranges are **denied**
/// - Max 5 hops per redirect chain
#[derive(Debug, Clone)]
pub struct QuarryRedirectPolicy {
    max_hops: usize,
    allow_localhost_redirect: bool,
}

impl QuarryRedirectPolicy {
    /// Create a new policy with default settings:
    /// - max_hops: 5
    /// - allow_localhost_redirect: false
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_hops: 5,
            allow_localhost_redirect: false,
        }
    }

    /// Allow redirects to localhost (useful for testing).
    #[must_use]
    pub fn allow_localhost(self, allow: bool) -> Self {
        Self {
            allow_localhost_redirect: allow,
            ..self
        }
    }

    /// Set maximum redirect hops (default: 5).
    #[must_use]
    pub fn with_max_hops(self, hops: usize) -> Self {
        Self {
            max_hops: hops,
            ..self
        }
    }

    /// Convert this policy into a `wreq::redirect::Policy` ready for the client.
    pub fn into_policy(self) -> Policy {
        Policy::custom(move |attempt: Attempt<'_>| {
            // Check max hops
            if attempt.previous.len() >= self.max_hops {
                return attempt.error("too many redirects");
            }

            // Get the URI as a string
            let uri_str = attempt.uri.as_ref().to_string();

            // Parse the next URL
            let next_url = match uri_str.parse::<Url>() {
                Ok(url) => url,
                Err(_) => return attempt.error("invalid redirect URL"),
            };

            // Check scheme
            match next_url.scheme() {
                "http" | "https" => {}
                _ => return attempt.error("unsupported redirect scheme"),
            }

            // Parse the host
            let host_str = match next_url.host_str() {
                Some(h) => h,
                None => return attempt.error("no host in redirect URL"),
            };

            let port = next_url.port_or_known_default().unwrap_or(80);
            if let Err(reason) = self.check_host(host_str, port) {
                return attempt.error(reason);
            }

            // If we got here, the redirect is safe
            attempt.follow()
        })
    }

    /// Checks a redirect target's host for SSRF safety, returning `Err` with
    /// a reason when it must be denied. An IP-literal host is checked
    /// directly; an FQDN is resolved and *every* returned address is
    /// checked -- a hostname that resolves to a private/internal address
    /// must be denied exactly like an IP literal would be, which the
    /// previous "FQDN - acceptable by default" behavior did not do.
    ///
    /// Resolution here is blocking OS-level `to_socket_addrs`, not the
    /// async lookup used elsewhere in the crate: `wreq::redirect::Policy`'s
    /// callback (see `into_policy`) is synchronous, and a redirect hop is
    /// rare enough per crawl that a blocking resolve is an acceptable
    /// trade-off against bridging into async from a sync callback.
    fn check_host(&self, host_str: &str, port: u16) -> Result<(), &'static str> {
        match host_str.parse::<IpAddr>() {
            Ok(ip) => {
                if self.is_private_ip(ip) {
                    return Err("redirect to private IP range denied");
                }
            }
            Err(_) => match (host_str, port).to_socket_addrs() {
                Ok(addrs) => {
                    for addr in addrs {
                        if self.is_private_ip(addr.ip()) {
                            return Err("redirect to private IP range denied (via FQDN)");
                        }
                    }
                }
                Err(_) => return Err("redirect FQDN failed to resolve"),
            },
        }
        Ok(())
    }

    /// Check if an IP is in a private/reserved range. Delegates to
    /// `quarry_security::heur::resolve_guard`, the same check
    /// `dns_guard.rs`'s preflight uses -- this used to be a hand-rolled,
    /// second copy of the same range list (and required an `ipnetwork`
    /// dependency this crate didn't even have, which is part of why this
    /// module was never wired into the crate's module tree at all).
    /// `allow_localhost_redirect` is handled here as a narrow override:
    /// it permits loopback specifically (useful for testing against a
    /// local mock server) without weakening the check for every other
    /// private/internal range.
    fn is_private_ip(&self, ip: IpAddr) -> bool {
        if ip.is_loopback() {
            return !self.allow_localhost_redirect;
        }
        resolve_guard(&[ip]).is_some()
    }
}

impl Default for QuarryRedirectPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localhost_denied_by_default() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.is_private_ip("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_localhost_allowed_when_configured() {
        let policy = QuarryRedirectPolicy::new().allow_localhost(true);
        assert!(!policy.is_private_ip("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_rfc1918_10_denied() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(policy.is_private_ip("10.255.255.255".parse().unwrap()));
    }

    #[test]
    fn test_rfc1918_172_denied() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(policy.is_private_ip("172.31.255.255".parse().unwrap()));
    }

    #[test]
    fn test_rfc1918_192_denied() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.is_private_ip("192.168.0.1".parse().unwrap()));
        assert!(policy.is_private_ip("192.168.255.255".parse().unwrap()));
    }

    #[test]
    fn test_link_local_denied() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.is_private_ip("169.254.0.1".parse().unwrap()));
    }

    #[test]
    fn test_multicast_denied() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.is_private_ip("224.0.0.1".parse().unwrap()));
        assert!(policy.is_private_ip("255.255.255.255".parse().unwrap()));
    }

    #[test]
    fn test_ipv6_localhost_denied() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.is_private_ip("::1".parse().unwrap()));
    }

    #[test]
    fn test_ipv6_private_denied() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.is_private_ip("fc00::1".parse().unwrap()));
        assert!(policy.is_private_ip("fe80::1".parse().unwrap()));
    }

    #[test]
    fn test_max_hops_enforced() {
        let policy = QuarryRedirectPolicy::new().with_max_hops(3);
        assert_eq!(policy.max_hops, 3);
    }

    #[test]
    fn test_public_ip_allowed() {
        let policy = QuarryRedirectPolicy::new();
        assert!(!policy.is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!policy.is_private_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn test_ipv6_public_allowed() {
        let policy = QuarryRedirectPolicy::new();
        assert!(!policy.is_private_ip("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn test_fqdn_resolving_to_loopback_denied() {
        // "localhost" resolves via the OS hosts file (no network needed) to
        // 127.0.0.1/::1 -- the exact case the old "FQDN - acceptable by
        // default" branch let through unchecked.
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.check_host("localhost", 80).is_err());
    }

    #[test]
    fn test_fqdn_resolving_to_loopback_allowed_when_localhost_permitted() {
        let policy = QuarryRedirectPolicy::new().allow_localhost(true);
        assert!(policy.check_host("localhost", 80).is_ok());
    }

    #[test]
    fn test_fqdn_that_fails_to_resolve_denied() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy
            .check_host("this-host-does-not-exist.invalid", 80)
            .is_err());
    }

    #[test]
    fn test_ip_literal_host_still_checked_directly() {
        let policy = QuarryRedirectPolicy::new();
        assert!(policy.check_host("10.0.0.5", 80).is_err());
        assert!(policy.check_host("8.8.8.8", 80).is_ok());
    }

    #[test]
    fn test_builder_chain() {
        let policy = QuarryRedirectPolicy::new()
            .allow_localhost(true)
            .with_max_hops(10);
        assert_eq!(policy.max_hops, 10);
        assert!(policy.allow_localhost_redirect);
    }
}
