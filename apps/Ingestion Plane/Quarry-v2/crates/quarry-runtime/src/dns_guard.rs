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

use async_trait::async_trait;
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
    resolve_public_url_via(url, &SystemHostLookup).await
}

/// The DNS lookup the guard checks. `SystemHostLookup` is the only production
/// implementation; the seam exists so a test can serve a second, different
/// answer for the same name (DNS rebinding, D2-059) without real DNS.
#[async_trait]
pub trait HostLookup: Send + Sync {
    /// Every socket address `host:port` maps to right now. `host` is the URL's
    /// host string as given (an IPv6 literal keeps its brackets).
    async fn lookup(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;
}

/// The operating system resolver (`tokio::net::lookup_host`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemHostLookup;

#[async_trait]
impl HostLookup for SystemHostLookup {
    async fn lookup(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        Ok(tokio::net::lookup_host(format!("{host}:{port}"))
            .await?
            .collect())
    }
}

/// Whether a navigation must pass the DNS guard (D2-059). `allow_private_hosts`
/// is a dev/test switch for allow-list runs that target a local fixture; it never
/// widens an open-web grant, whose whole authority is "any PUBLIC host": a public
/// name that resolves to a private address is refused there whatever the switch.
pub fn dns_guard_required(allow_private_hosts: bool, open_web: bool) -> bool {
    open_web || !allow_private_hosts
}

/// `resolve_public_url` through an explicit lookup. Each call resolves afresh
/// and vets that answer: an earlier public answer for the same name grants
/// nothing, so a name rebound to a private address is refused on the second
/// resolution.
pub async fn resolve_public_url_via(
    url: &Url,
    lookup: &dyn HostLookup,
) -> QuarryResult<ResolvedTarget> {
    let host = url
        .host_str()
        .ok_or_else(|| QuarryError::new(ErrorCode::BadRequest, "missing host in URL"))?;

    let port = url.port_or_known_default().unwrap_or(80);

    let addresses: Vec<SocketAddr> = lookup
        .lookup(host, port)
        .await
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("dns lookup failed: {e}")))?;
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

    /// A lookup that serves its answers in order, one per call, the way a
    /// rebinding DNS server does: a public address first, a private one next.
    struct ScriptedLookup {
        answers: std::sync::Mutex<std::collections::VecDeque<Vec<SocketAddr>>>,
    }

    impl ScriptedLookup {
        fn new(answers: Vec<Vec<SocketAddr>>) -> Self {
            Self {
                answers: std::sync::Mutex::new(answers.into()),
            }
        }
    }

    #[async_trait]
    impl HostLookup for ScriptedLookup {
        async fn lookup(&self, _host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
            let next = self
                .answers
                .lock()
                .expect("scripted lookup lock")
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no scripted answer"))?;
            Ok(next
                .into_iter()
                .map(|address| SocketAddr::new(address.ip(), port))
                .collect())
        }
    }

    fn public_v4() -> SocketAddr {
        SocketAddr::from((Ipv4Addr::new(93, 184, 216, 34), 0))
    }

    #[tokio::test]
    async fn dns_guard_refuses_a_second_resolution_to_a_private_address() {
        // D2-059: the name answered public once, then rebinds to RFC 1918 and to
        // the cloud metadata address. Each resolution is vetted on its own: the
        // first public answer grants the later ones nothing.
        let lookup = ScriptedLookup::new(vec![
            vec![public_v4()],
            vec![SocketAddr::from((Ipv4Addr::new(10, 0, 0, 7), 0))],
            vec![SocketAddr::from((Ipv4Addr::new(169, 254, 169, 254), 0))],
        ]);
        let target = url("https://rebind.example/login");

        let first = resolve_public_url_via(&target, &lookup)
            .await
            .expect("the first, public answer preflights");
        assert_eq!(first.host, "rebind.example");
        assert_eq!(
            first.addresses,
            vec![SocketAddr::new(public_v4().ip(), 443)]
        );

        let second = resolve_public_url_via(&target, &lookup)
            .await
            .expect_err("the rebound private answer must be refused");
        assert_eq!(second.code, ErrorCode::SecurityBlocked);

        let third = resolve_public_url_via(&target, &lookup)
            .await
            .expect_err("the rebound metadata answer must be refused");
        assert_eq!(third.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn dns_guard_refuses_a_mixed_answer_with_one_private_address() {
        // A rebinding server can also answer with both: one public address to
        // pass a naive "any public" check, one private for the connect to pick.
        let lookup = ScriptedLookup::new(vec![vec![
            public_v4(),
            SocketAddr::from((Ipv4Addr::new(192, 168, 1, 1), 0)),
        ]]);
        let err = resolve_public_url_via(&url("http://mixed.example/"), &lookup)
            .await
            .expect_err("one private address taints the whole answer");
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rebinding_cannot_replace_a_vetted_pin_with_a_private_address() {
        // The pinned resolver is the transport half: after the rebound answer is
        // refused, the connection still dials only the address vetted first.
        let lookup = ScriptedLookup::new(vec![
            vec![public_v4()],
            vec![SocketAddr::from((Ipv4Addr::LOCALHOST, 0))],
        ]);
        let target = url("https://rebind.example/");
        let resolver = PinnedDnsResolver::default();

        let vetted = resolve_public_url_via(&target, &lookup)
            .await
            .expect("first answer is public");
        resolver.pin(vetted.clone()).expect("public pin");

        assert!(resolve_public_url_via(&target, &lookup).await.is_err());
        let rebound = ResolvedTarget {
            host: "rebind.example".to_owned(),
            addresses: vec![SocketAddr::from((Ipv4Addr::LOCALHOST, 443))],
        };
        assert_eq!(
            resolver.pin(rebound).expect_err("private pin refused").code,
            ErrorCode::SecurityBlocked
        );

        let name = "rebind.example".parse().expect("valid DNS name");
        let addresses: Vec<_> = Resolve::resolve(&resolver, name)
            .await
            .expect("the vetted pin survives")
            .collect();
        assert_eq!(addresses, vetted.addresses);
    }

    #[test]
    fn dns_guard_stays_on_for_open_web_under_allow_private_hosts() {
        // allow_private_hosts only relaxes allow-list runs (local fixtures).
        assert!(dns_guard_required(false, false));
        assert!(!dns_guard_required(true, false));
        assert!(dns_guard_required(false, true));
        assert!(
            dns_guard_required(true, true),
            "an open-web grant is public-only whatever allow_private_hosts says"
        );
    }
}
