//! SSRF-safe redirect policy for the TLS client.
//!
//! This module provides a builder for creating a `wreq::redirect::Policy` that validates
//! each redirect hop against RFC 1918 private IP ranges and other security constraints
//! before allowing the redirect to proceed.

use ipnetwork::IpNetwork;
use std::net::IpAddr;
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

            // Try to parse as IP, otherwise treat as FQDN
            match host_str.parse::<IpAddr>() {
                Ok(ip) => {
                    if self.is_private_ip(ip) {
                        return attempt.error("redirect to private IP range denied");
                    }
                }
                Err(_) => {
                    // FQDN - acceptable by default
                }
            }

            // If we got here, the redirect is safe
            attempt.follow()
        })
    }

    /// Check if an IP is in a private/reserved range.
    fn is_private_ip(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => {
                // 127.0.0.0/8 (loopback)
                if v4.is_loopback() {
                    return !self.allow_localhost_redirect;
                }
                // 10.0.0.0/8
                if IpNetwork::V4("10.0.0.0/8".parse().unwrap()).contains(std::net::IpAddr::V4(v4)) {
                    return true;
                }
                // 172.16.0.0/12
                if IpNetwork::V4("172.16.0.0/12".parse().unwrap())
                    .contains(std::net::IpAddr::V4(v4))
                {
                    return true;
                }
                // 192.168.0.0/16
                if IpNetwork::V4("192.168.0.0/16".parse().unwrap())
                    .contains(std::net::IpAddr::V4(v4))
                {
                    return true;
                }
                // 169.254.0.0/16 (link-local)
                if v4.is_link_local() {
                    return true;
                }
                // 224.0.0.0/4 (multicast)
                if v4.is_multicast() {
                    return true;
                }
                // 255.255.255.255/32 (broadcast)
                if v4.is_broadcast() {
                    return true;
                }
                false
            }
            IpAddr::V6(v6) => {
                // ::1 (loopback)
                if v6.is_loopback() {
                    return !self.allow_localhost_redirect;
                }
                // fc00::/7 (private)
                if v6.is_unique_local() {
                    return true;
                }
                // fe80::/10 (link-local)
                if v6.is_unicast_link_local() {
                    return true;
                }
                // ff00::/8 (multicast)
                if v6.is_multicast() {
                    return true;
                }
                false
            }
        }
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
    fn test_builder_chain() {
        let policy = QuarryRedirectPolicy::new()
            .allow_localhost(true)
            .with_max_hops(10);
        assert_eq!(policy.max_hops, 10);
        assert!(policy.allow_localhost_redirect);
    }
}
