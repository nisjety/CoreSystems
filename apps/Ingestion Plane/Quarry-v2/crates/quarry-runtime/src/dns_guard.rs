//! SSRF-guard: resolves the target host and rejects private/loopback addresses.
//!
//! Called before any outbound request leaves the runtime so that an attacker
//! cannot coerce the scraper into probing internal infrastructure.
//!
//! This is the SSRF security *boundary* referenced by verevonv3 gateway's
//! `public_url.rs` (`apps/Frontend Plane/verevonv3/apps/gateway/src/public_url.rs`):
//! that module is a string/literal-IP pre-filter only, safe today solely
//! because every gateway call site forwards the normalized URL here (to
//! Quarry) instead of dialing it directly. `ResolvedTarget` /
//! `PinnedDnsResolver` below is what actually resolves DNS, vets the
//! resolved addresses, and pins the connection — closing the check-then-rebind
//! gap a string-only check cannot. The gateway's own
//! `tests/ssrf_forward_not_fetch.rs` enforces its half of that contract; there
//! is deliberately no shared crate between the two (see
//! `apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md` section 3, SSRF-2) — the
//! two modules have different threat models and this doc comment plus that
//! test are the documented contract instead.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use dashmap::DashMap;
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_security::heur::resolve_guard;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::Url;

const MAX_PINNED_HOSTS: usize = 4_096;

/// A DNS result that passed Quarry's public-address policy. The hostname is
/// retained for HTTP Host/TLS SNI while the addresses are supplied directly to
/// the transport resolver, preventing a second DNS lookup from changing the
/// connection target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub(crate) host: String,
    pub(crate) addresses: Vec<SocketAddr>,
}

/// Reqwest resolver that permits only addresses explicitly pinned by the
/// security preflight. It intentionally has no fallback resolver: a missing
/// pin is an availability error, never permission to perform a new lookup.
#[derive(Clone, Default)]
pub struct PinnedDnsResolver {
    pins: Arc<DashMap<String, Vec<SocketAddr>>>,
}

impl PinnedDnsResolver {
    pub fn pin(&self, target: ResolvedTarget) -> QuarryResult<()> {
        let addresses: Vec<IpAddr> = target.addresses.iter().map(SocketAddr::ip).collect();
        if let Some(reason) = resolve_guard(&addresses) {
            return Err(QuarryError::new(ErrorCode::SecurityBlocked, reason));
        }
        let host = normalize_host(&target.host);
        if host.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "pinned DNS target is missing a host",
            ));
        }
        if !self.pins.contains_key(&host) && self.pins.len() >= MAX_PINNED_HOSTS {
            // Bounded memory is preferable to retaining attacker-controlled
            // hostnames indefinitely. A concurrent request missing after this
            // eviction fails closed rather than resolving a new destination.
            self.pins.clear();
        }
        self.pins.insert(host, target.addresses);
        Ok(())
    }
}

impl Resolve for PinnedDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = normalize_host(name.as_str());
        let pinned = self.pins.get(&host).map(|entry| entry.value().clone());
        Box::pin(async move {
            let addresses = pinned.ok_or_else(|| {
                Box::new(io::Error::new(
                    io::ErrorKind::NotFound,
                    "DNS target was not security preflighted",
                )) as Box<dyn std::error::Error + Send + Sync>
            })?;
            if addresses.is_empty() {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::NotFound,
                    "DNS target has no pinned addresses",
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

/// Resolve every IP address the host maps to and return `Err` if any of them
/// falls inside a private, loopback, or link-local range.
///
/// # Errors
/// - `BadRequest`      — `url` has no host component.
/// - `Internal`        — DNS lookup failed (network or resolver error).
/// - `SecurityBlocked` — At least one resolved address is private/loopback.
pub async fn resolve_public_url(url: &Url) -> QuarryResult<ResolvedTarget> {
    let host = url
        .host_str()
        .ok_or_else(|| QuarryError::new(ErrorCode::BadRequest, "missing host in URL"))?;

    let port = url.port_or_known_default().unwrap_or(80);

    let addresses: Vec<SocketAddr> = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("dns lookup failed: {e}")))?
        .collect();
    if addresses.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Internal,
            "dns lookup returned no addresses",
        ));
    }

    let addrs: Vec<IpAddr> = addresses.iter().map(SocketAddr::ip).collect();

    if let Some(reason) = resolve_guard(&addrs) {
        return Err(QuarryError::new(ErrorCode::SecurityBlocked, reason));
    }

    Ok(ResolvedTarget {
        host: normalize_host(host),
        addresses,
    })
}

pub async fn guard_url(url: &Url) -> QuarryResult<()> {
    resolve_public_url(url).await.map(|_| ())
}

fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr};

    use reqwest::dns::Resolve;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[tokio::test]
    async fn blocks_loopback() {
        let err = guard_url(&url("http://127.0.0.1/foo")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn blocks_localhost() {
        let err = guard_url(&url("http://localhost/")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rejects_missing_host() {
        // "file:///etc/passwd" has no host
        let err = guard_url(&url("file:///etc/passwd")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn pinned_resolver_returns_only_the_preflighted_address() {
        let resolver = PinnedDnsResolver::default();
        // Not an RFC 5737 TEST-NET literal: `resolve_guard` now blocks the
        // documentation ranges (parity with capability-core's Go-side
        // `mcpForbiddenRanges`), so a 203.0.113.0/24 address is no longer
        // "vetted" — it would fail `pin` instead of demonstrating a
        // successful preflight.
        let vetted = SocketAddr::from((Ipv4Addr::new(93, 184, 216, 34), 443));
        resolver
            .pin(ResolvedTarget {
                host: "rebind.example".to_string(),
                addresses: vec![vetted],
            })
            .expect("public address can be pinned");

        let name = "rebind.example".parse().expect("valid DNS name");
        let addresses: Vec<_> = Resolve::resolve(&resolver, name)
            .await
            .expect("pinned hostname resolves")
            .collect();
        assert_eq!(addresses, vec![vetted]);

        let missing = "unvetted.example".parse().expect("valid DNS name");
        assert!(Resolve::resolve(&resolver, missing).await.is_err());
    }

    #[test]
    fn pinned_resolver_rejects_private_addresses() {
        let resolver = PinnedDnsResolver::default();
        let err = resolver
            .pin(ResolvedTarget {
                host: "private.example".to_string(),
                addresses: vec![SocketAddr::from((Ipv4Addr::LOCALHOST, 443))],
            })
            .expect_err("private addresses must not enter the pinned resolver");
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn resolve_public_url_returns_the_addresses_it_vetted() {
        // The blocking cases are covered above; this is the positive path,
        // which is what the pin is actually built from — a preflight that
        // returned an empty or loopback set while reporting success would hand
        // the driver something unusable.
        //
        // Uses a real (non-reserved) IP literal rather than a real hostname on
        // purpose. An IP literal is resolved locally by `lookup_host`, so this
        // test performs NO DNS and cannot hang or fail offline. Deliberately
        // NOT an RFC 5737 TEST-NET-1/2/3 literal (192.0.2.0/24, 198.51.100.0/24,
        // 203.0.113.0/24): `resolve_guard` now blocks the documentation ranges
        // too (parity with capability-core's Go-side `mcpForbiddenRanges`), so
        // those would fail this preflight for the right reason instead of
        // passing it by accident.
        let target = resolve_public_url(&url("https://93.184.216.34/some/path"))
            .await
            .expect("a public, non-private literal must preflight cleanly");
        assert_eq!(target.host, "93.184.216.34");
        assert!(
            !target.addresses.is_empty(),
            "a successful preflight must yield at least one address to pin to"
        );
        assert!(
            target
                .addresses
                .iter()
                .all(|address| !address.ip().is_loopback()),
            "no vetted address may be loopback"
        );
        assert!(
            target.addresses.iter().all(|address| address.port() == 443),
            "the scheme's default port must be carried into the pin"
        );
    }

    #[tokio::test]
    async fn resolve_public_url_blocks_loopback_same_as_guard_url() {
        // The two entry points must agree: a caller that preflights through
        // resolve_public_url cannot end up with a target guard_url would have
        // refused. No DNS needed - the literal is already an address.
        let url: Url = "http://127.0.0.1/foo".parse().expect("valid url");
        let err = resolve_public_url(&url)
            .await
            .expect_err("loopback must be blocked on the resolve path too");

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }
}
