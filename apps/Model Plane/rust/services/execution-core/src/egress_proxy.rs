//! The per-call egress proxy (EP-9 E3, D-E1): the host half of a tiered
//! `code_interpreter` call's network boundary.
//!
//! # The boundary
//!
//! A tiered call runs with its network namespace unshared, so it has loopback
//! and no route out. Its one path out is a Unix socket this module listens on
//! and the sandbox binds in, reached from inside through `mp-netbridge` on
//! `127.0.0.1:3128` where the program's proxy variables point. Everything the
//! program sends leaves through here or not at all, so the allow-list is a
//! boundary, not advice the program may ignore.
//!
//! # What the proxy admits
//!
//! `CONNECT host:443` and nothing else: no plain-HTTP forwarding, no other
//! port, no IP literal. The host must be a canonical name the call's tier
//! admits ([`mp_contracts::egress_tiers::host_admitted`]). The proxy resolves
//! the name itself and refuses it when ANY answer is a private, loopback,
//! link-local, metadata or otherwise non-public address, then connects to the
//! address it checked, so a name cannot be re-pointed between check and use.
//! This is the idea of Quarry's DNS guard, written fresh here with its own
//! tests: no crate crosses the plane boundary.
//!
//! # What it keeps
//!
//! Counts of distinct allowed and denied hosts, returned in the tool result,
//! and a log line per decision naming the host only. A CONNECT carries no path,
//! and nothing from inside the tunnel is read, so no content is kept anywhere:
//! there is nothing here for zero data retention to propagate to. ZDR and
//! waived runs never get a tier above off in the first place (the run ceiling
//! in `runtime_loop::agent`).
//!
//! # Residual risk
//!
//! A CONNECT proxy sees the host, not the request inside the tunnel, so data
//! can leave inside a request to an admitted host. That is why the registries
//! list is short and reviewed, `all` is not grantable, and every run that must
//! not leak (ZDR, waived, plan mode, tainted, locked down) is held at off.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(unix)]
use std::time::Duration;

use mp_contracts::egress_tiers;
use mp_contracts::sandbox_report::NetworkTier;

/// The deployment switch: `off` (default), `registries` or `allowlist`. Only
/// those exact words raise it; anything else is off, because it grants egress.
pub const EGRESS_SWITCH_ENV: &str = "EXECUTION_SANDBOX_EGRESS";

/// The bridge binary the sandbox image ships (`rust/tools/mp-netbridge`).
pub const NETBRIDGE_BIN: &str = "/usr/local/bin/mp-netbridge";

/// Where the program's proxy variables point inside the sandbox.
pub const SANDBOX_PROXY_URL: &str = "http://127.0.0.1:3128";

/// The only port a tunnel may open: package registries and allow-listed hosts
/// are reached over HTTPS.
pub const TUNNEL_PORT: u16 = 443;

/// The host directory every call's socket directory is made under; covered by
/// an empty tmpfs inside every sandbox (see `sandbox::EgressBridge`).
const SOCKET_ROOT_NAME: &str = "verevon-egress";

/// The longest request head read before a CONNECT is refused.
#[cfg(unix)]
const MAX_REQUEST_HEAD: usize = 8 * 1024;

/// How long a client may take to send its CONNECT, and the upstream to accept.
#[cfg(unix)]
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(unix)]
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The deployment switch, read once: it is a property of the deployment.
#[must_use]
pub fn switch_tier() -> NetworkTier {
    static TIER: OnceLock<NetworkTier> = OnceLock::new();
    *TIER.get_or_init(|| {
        egress_tiers::parse_switch(std::env::var(EGRESS_SWITCH_ENV).ok().as_deref())
    })
}

/// Whether the bridge binary is present in this image, decided once. A host
/// without it cannot enforce a tier and claims off.
#[must_use]
pub fn bridge_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| cfg!(unix) && Path::new(NETBRIDGE_BIN).is_file())
}

/// What one call may reach: its tier, and the Space's list when the tier is
/// `allowlist`. Built by [`CallEgress::resolve`], never from tool input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEgress {
    pub tier: NetworkTier,
    pub allowlist: Vec<String>,
}

impl CallEgress {
    /// No network: what every call gets unless all three ceilings allow more.
    #[must_use]
    pub fn off() -> Self {
        Self {
            tier: NetworkTier::Off,
            allowlist: Vec::new(),
        }
    }

    /// The tier a call runs at: the narrowest of the deployment switch, the
    /// lease (what Control granted the Space and sandbox-manager recorded) and
    /// the run (forced off for ZDR, waived, plan-mode, tainted and locked-down
    /// runs). The Space's list is kept only when the result is `allowlist`.
    #[must_use]
    pub fn resolve(
        switch: NetworkTier,
        lease: NetworkTier,
        run: NetworkTier,
        allowlist: &[String],
    ) -> Self {
        let tier = egress_tiers::min_tier(egress_tiers::min_tier(switch, lease), run);
        // `all` is not grantable in Wave 3, whichever ceiling named it.
        let tier = if tier == NetworkTier::All {
            NetworkTier::Off
        } else {
            tier
        };
        Self {
            tier,
            allowlist: if tier == NetworkTier::Allowlist {
                allowlist.to_vec()
            } else {
                Vec::new()
            },
        }
    }

    /// Whether the call needs a proxy at all.
    #[must_use]
    pub fn is_tiered(&self) -> bool {
        self.tier != NetworkTier::Off
    }

    /// The hosts the call may reach, for the policy's `AllowDomains` and the
    /// report. Informational: the proxy decides per connection.
    #[must_use]
    pub fn admitted_hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = egress_tiers::REGISTRY_HOSTS
            .iter()
            .map(|host| (*host).to_owned())
            .collect();
        if self.tier == NetworkTier::Allowlist {
            hosts.extend(
                self.allowlist
                    .iter()
                    .filter_map(|entry| egress_tiers::canonical_host(entry)),
            );
        }
        if self.tier == NetworkTier::Off {
            hosts.clear();
        }
        hosts
    }

    /// The proxy variables the program runs with. Every common spelling, so
    /// pip, npm, curl and Python's `urllib` all find the bridge.
    #[must_use]
    pub fn child_env(&self) -> Vec<(String, String)> {
        if !self.is_tiered() {
            return Vec::new();
        }
        [
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "npm_config_https_proxy",
            "npm_config_proxy",
        ]
        .iter()
        .map(|key| ((*key).to_owned(), SANDBOX_PROXY_URL.to_owned()))
        .collect()
    }
}

/// The tier a Space lease recorded, from `AcquireLeaseResponse.egress_tier`.
/// Unspecified (an older sandbox-manager, or its switch off), `all` (never
/// recorded in Wave 3) and a value this build does not know are all `off`.
#[must_use]
pub fn lease_tier(raw: i32) -> NetworkTier {
    use mp_contracts::model_plane::v1::SandboxEgressTier;
    match SandboxEgressTier::try_from(raw) {
        Ok(SandboxEgressTier::Registries) => NetworkTier::Registries,
        Ok(SandboxEgressTier::Allowlist) => NetworkTier::Allowlist,
        _ => NetworkTier::Off,
    }
}

/// What a run is at the moment a call dispatches, as far as its egress goes.
/// Independent facts, any one of which holds the call at off.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RunEgressFacts {
    /// The run is zero data retention.
    pub zdr: bool,
    /// The run's route is waived (DEC-32): it counts as zero retention for
    /// every new egress.
    pub waived: bool,
    /// The run is in plan mode.
    pub plan_mode: bool,
    /// The run has read flagged content: a connector read in this round or an
    /// earlier one, or a content-safety screen (WS23-S3).
    pub tainted: bool,
    /// The run states the read-only autonomy rung: where plan mode lands, and
    /// where an org's lockdown or read-only mode cap is meant to land.
    pub read_only_rung: bool,
}

/// The run's ceiling for its next call (EP-9 D-E3): off for a run that must
/// not leak, otherwise `all`, which here means "no run-level limit" (the
/// deployment switch and the lease still bound the call, and the call tier is
/// never `all`).
#[must_use]
pub fn run_ceiling(facts: RunEgressFacts) -> NetworkTier {
    if facts.zdr || facts.waived || facts.plan_mode || facts.tainted || facts.read_only_rung {
        NetworkTier::Off
    } else {
        NetworkTier::All
    }
}

/// What one Do run's `code_interpreter` calls may reach, held for the run's
/// life: the run's own ceiling (set by the run loop, lowered to off once the
/// run is tainted) and what its Space lease recorded (set when the lease is
/// acquired). Kept here rather than threaded through every dispatch layer, and
/// read at the spawn site, so no tool input can name or widen it.
///
/// A run with no entry runs at off: an `ExecuteStep` call, a run on a
/// deployment with the switch off, and a run whose loop has already ended.
#[derive(Debug, Default)]
pub struct RunEgressRegistry {
    runs: Mutex<std::collections::HashMap<(String, String), RunEntry>>,
}

#[derive(Debug, Clone)]
struct RunEntry {
    ceiling: NetworkTier,
    lease: NetworkTier,
    allowlist: Vec<String>,
}

/// The process-wide registry.
#[must_use]
pub fn run_egress() -> &'static RunEgressRegistry {
    static REGISTRY: OnceLock<RunEgressRegistry> = OnceLock::new();
    REGISTRY.get_or_init(RunEgressRegistry::default)
}

/// Removes a run's entry when the run loop that registered it ends, by any
/// path: a terminal, an error or a dropped future.
#[derive(Debug)]
#[must_use = "the run's egress entry is removed when the guard drops"]
pub struct RunEgressGuard {
    registry: &'static RunEgressRegistry,
    key: (String, String),
}

impl Drop for RunEgressGuard {
    fn drop(&mut self) {
        if let Ok(mut runs) = self.registry.runs.lock() {
            runs.remove(&self.key);
        }
    }
}

impl RunEgressRegistry {
    /// Holds an entry for the run until the guard drops. Both halves start at
    /// off: the ceiling is set per call by [`RunEgressRegistry::set_ceiling`],
    /// the lease half by [`RunEgressRegistry::record_lease`].
    pub fn register(&'static self, org_id: &str, run_id: &str) -> RunEgressGuard {
        let key = (org_id.to_owned(), run_id.to_owned());
        if let Ok(mut runs) = self.runs.lock() {
            runs.insert(
                key.clone(),
                RunEntry {
                    ceiling: NetworkTier::Off,
                    lease: NetworkTier::Off,
                    allowlist: Vec::new(),
                },
            );
        }
        RunEgressGuard {
            registry: self,
            key,
        }
    }

    /// Sets the run's ceiling for the call about to dispatch. The run loop
    /// computes it before every `code_interpreter` call from what the run is
    /// at that moment ([`run_ceiling`]), so a taint or plan mode that arrives
    /// mid-run holds the next call at off.
    pub fn set_ceiling(&self, org_id: &str, run_id: &str, ceiling: NetworkTier) {
        if let Ok(mut runs) = self.runs.lock() {
            if let Some(entry) = runs.get_mut(&(org_id.to_owned(), run_id.to_owned())) {
                entry.ceiling = ceiling;
            }
        }
    }

    /// Records what the run's Space lease permits: the tier sandbox-manager
    /// recorded and the Space's list from the Control decision the lease was
    /// granted on. Ignored for a run that is not registered, so it stays off.
    pub fn record_lease(
        &self,
        org_id: &str,
        run_id: &str,
        tier: NetworkTier,
        allowlist: &[String],
    ) {
        if let Ok(mut runs) = self.runs.lock() {
            if let Some(entry) = runs.get_mut(&(org_id.to_owned(), run_id.to_owned())) {
                entry.lease = tier;
                entry.allowlist = allowlist.to_vec();
            }
        }
    }

    /// What the run's next call may reach under the deployment switch.
    #[must_use]
    pub fn call_egress(&self, org_id: &str, run_id: &str) -> CallEgress {
        self.call_egress_with(switch_tier(), org_id, run_id)
    }

    /// [`RunEgressRegistry::call_egress`] with the switch injected.
    #[must_use]
    pub fn call_egress_with(&self, switch: NetworkTier, org_id: &str, run_id: &str) -> CallEgress {
        let entry = self
            .runs
            .lock()
            .ok()
            .and_then(|runs| runs.get(&(org_id.to_owned(), run_id.to_owned())).cloned());
        match entry {
            Some(entry) => {
                CallEgress::resolve(switch, entry.lease, entry.ceiling, &entry.allowlist)
            }
            None => CallEgress::off(),
        }
    }
}

/// Distinct hosts the proxy admitted and refused during one call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct EgressCounts {
    pub allowed_hosts: usize,
    pub denied_hosts: usize,
}

/// Why a CONNECT was refused. The client sees only the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    /// The head was not a well-formed HTTP request, or was too long.
    Malformed,
    /// A method other than CONNECT (plain-HTTP forwarding is not offered).
    NotConnect,
    /// A port other than [`TUNNEL_PORT`].
    Port,
    /// The target is not a canonical host name (an IP literal, for one).
    NotAHostName,
    /// The call's tier does not admit the host.
    NotListed,
    /// The name did not resolve.
    Unresolved,
    /// The name resolved to an address that is not public.
    NonPublicAddress,
    /// The upstream did not accept the connection.
    Upstream,
}

impl Denial {
    /// The response the client gets.
    #[must_use]
    pub const fn response(self) -> &'static [u8] {
        match self {
            Self::Malformed => b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n",
            Self::NotConnect => b"HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n",
            Self::Port | Self::NotAHostName | Self::NotListed | Self::NonPublicAddress => {
                b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n"
            }
            Self::Unresolved | Self::Upstream => {
                b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n"
            }
        }
    }

    /// A short label for the log line.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::NotConnect => "not_connect",
            Self::Port => "port",
            Self::NotAHostName => "not_a_host_name",
            Self::NotListed => "not_listed",
            Self::Unresolved => "unresolved",
            Self::NonPublicAddress => "non_public_address",
            Self::Upstream => "upstream",
        }
    }
}

/// The success reply that opens the tunnel.
#[cfg(unix)]
const ESTABLISHED: &[u8] = b"HTTP/1.1 200 Connection established\r\n\r\n";

/// The target of a CONNECT request head: `CONNECT host:port HTTP/1.x`. Only
/// the request line is read; headers are ignored.
///
/// # Errors
/// [`Denial::Malformed`] or [`Denial::NotConnect`].
pub fn parse_connect(head: &[u8]) -> Result<(String, u16), Denial> {
    let text = std::str::from_utf8(head).map_err(|_| Denial::Malformed)?;
    let line = text.split("\r\n").next().ok_or(Denial::Malformed)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Denial::Malformed);
    };
    if !version.starts_with("HTTP/1.") {
        return Err(Denial::Malformed);
    }
    if method != "CONNECT" {
        return Err(Denial::NotConnect);
    }
    // An IPv6 literal is bracketed; it is split here and refused as a non-name.
    let (host, port) = target.rsplit_once(':').ok_or(Denial::Malformed)?;
    let port: u16 = port.parse().map_err(|_| Denial::Malformed)?;
    if host.is_empty() {
        return Err(Denial::Malformed);
    }
    Ok((host.to_owned(), port))
}

/// The canonical host a call may tunnel to, before any resolution.
///
/// # Errors
/// [`Denial::Port`], [`Denial::NotAHostName`] or [`Denial::NotListed`].
pub fn admit_target(egress: &CallEgress, host: &str, port: u16) -> Result<String, Denial> {
    if port != TUNNEL_PORT {
        return Err(Denial::Port);
    }
    let host = egress_tiers::canonical_host(host).ok_or(Denial::NotAHostName)?;
    if !egress_tiers::host_admitted(egress.tier, &host, &egress.allowlist) {
        return Err(Denial::NotListed);
    }
    Ok(host)
}

/// Whether `ip` is a public unicast address a sandbox may reach. Refuses
/// loopback, private (RFC 1918, ULA), link-local (which holds the cloud
/// metadata address 169.254.169.254), carrier-grade NAT, unspecified,
/// broadcast, multicast, documentation and benchmarking ranges, the
/// `0.0.0.0/8` and `240.0.0.0/4` blocks, and an IPv4-mapped or NAT64 IPv6
/// address whose IPv4 part is any of those.
#[must_use]
pub fn is_public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        || a >= 240
        // 100.64.0.0/10, carrier-grade NAT.
        || (a == 100 && (64..=127).contains(&b))
        // 192.0.0.0/24, IETF protocol assignments.
        || (a == 192 && b == 0 && c == 0)
        // 198.18.0.0/15, benchmarking.
        || (a == 198 && (b == 18 || b == 19)))
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let segments = ip.segments();
    // 64:ff9b::/96, NAT64: the embedded IPv4 address decides.
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        let [_, _, _, _, _, _, high, low] = segments;
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        return is_public_v4(Ipv4Addr::new(a, b, c, d));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        // fc00::/7, unique local.
        || (segments[0] & 0xfe00) == 0xfc00
        // fe80::/10, link-local.
        || (segments[0] & 0xffc0) == 0xfe80
        // 2001:db8::/32, documentation.
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        // ::/96, the deprecated IPv4-compatible block.
        || segments[..6] == [0, 0, 0, 0, 0, 0])
}

/// The address to connect to, chosen from a name's answers. Refuses the name
/// when ANY answer fails `permitted`: a name that answers with one public and
/// one private address is treated as an attempt to reach the private one.
///
/// # Errors
/// [`Denial::Unresolved`] for no answers, [`Denial::NonPublicAddress`] otherwise.
pub fn choose_address(
    answers: &[SocketAddr],
    permitted: fn(IpAddr) -> bool,
) -> Result<SocketAddr, Denial> {
    let first = answers.first().ok_or(Denial::Unresolved)?;
    if answers.iter().any(|answer| !permitted(answer.ip())) {
        return Err(Denial::NonPublicAddress);
    }
    Ok(*first)
}

/// Resolves a host name. Injected so tests can answer with any address.
pub type Resolver = Arc<
    dyn Fn(String, u16) -> futures::future::BoxFuture<'static, std::io::Result<Vec<SocketAddr>>>
        + Send
        + Sync,
>;

/// The system resolver.
fn system_resolver() -> Resolver {
    Arc::new(|host: String, port: u16| {
        Box::pin(async move {
            Ok(tokio::net::lookup_host((host.as_str(), port))
                .await?
                .collect::<Vec<_>>())
        })
    })
}

/// Distinct hosts seen by one call's proxy.
#[derive(Debug, Default)]
struct Tally {
    allowed: BTreeSet<String>,
    denied: BTreeSet<String>,
}

/// The longest host name kept in the tally. A refused target can be anything
/// the program wrote, so the key is bounded.
const MAX_TALLIED_HOST: usize = 253;

impl Tally {
    #[cfg_attr(not(unix), allow(dead_code))]
    fn record(&mut self, host: &str, allowed: bool) {
        let key: String = host
            .chars()
            .take(MAX_TALLIED_HOST)
            .collect::<String>()
            .to_ascii_lowercase();
        if allowed {
            self.allowed.insert(key);
        } else {
            self.denied.insert(key);
        }
    }

    fn counts(&self) -> EgressCounts {
        EgressCounts {
            allowed_hosts: self.allowed.len(),
            denied_hosts: self.denied.len(),
        }
    }
}

/// One call's running proxy. Dropping it stops the proxy and removes the
/// socket directory; [`EgressProxy::finish`] does the same and returns the
/// counts.
pub struct EgressProxy {
    root: PathBuf,
    dir: PathBuf,
    socket: PathBuf,
    tally: Arc<Mutex<Tally>>,
    task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for EgressProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EgressProxy")
            .field("socket", &self.socket)
            .field("live", &self.is_live())
            .finish_non_exhaustive()
    }
}

/// The host directory every call's socket directory is made under.
#[must_use]
pub fn socket_root() -> PathBuf {
    std::env::temp_dir().join(SOCKET_ROOT_NAME)
}

impl EgressProxy {
    /// Start the proxy for one call: a fresh private directory under
    /// [`socket_root`], a socket in it, and an accept loop admitting what
    /// `egress` allows.
    ///
    /// # Errors
    /// `Unsupported` for an untiered call or a host without Unix sockets; the
    /// filesystem or bind error otherwise. The call must not run without it.
    pub fn start(egress: &CallEgress) -> std::io::Result<Self> {
        Self::start_with(egress, &socket_root(), system_resolver(), is_public_address)
    }

    /// [`EgressProxy::start`] with the root, the resolver and the address
    /// guard injected: the testable seam. Production always passes
    /// [`is_public_address`].
    pub(crate) fn start_with(
        egress: &CallEgress,
        root: &Path,
        resolver: Resolver,
        permitted: fn(IpAddr) -> bool,
    ) -> std::io::Result<Self> {
        if !egress.is_tiered() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "an untiered call has no egress proxy",
            ));
        }
        Self::listen(egress.clone(), root, resolver, permitted)
    }

    #[cfg(unix)]
    fn listen(
        egress: CallEgress,
        root: &Path,
        resolver: Resolver,
        permitted: fn(IpAddr) -> bool,
    ) -> std::io::Result<Self> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)?;
        let dir = root.join(format!(
            "call-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        // A fresh directory, never a reused one: `create`, not `create_all`.
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let socket = dir.join(crate::sandbox::SANDBOX_SOCKET_NAME);
        let listener = match tokio::net::UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(error);
            }
        };
        // The sandbox user may differ from ours under a user namespace; the
        // directory's 0700 is what keeps other host processes out.
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666))?;
        let tally = Arc::new(Mutex::new(Tally::default()));
        let egress = Arc::new(egress);
        let task = {
            let tally = Arc::clone(&tally);
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    let (egress, tally, resolver) = (
                        Arc::clone(&egress),
                        Arc::clone(&tally),
                        Arc::clone(&resolver),
                    );
                    tokio::spawn(async move {
                        serve(stream, &egress, &tally, &resolver, permitted).await;
                    });
                }
            })
        };
        Ok(Self {
            root: root.to_path_buf(),
            dir,
            socket,
            tally,
            task,
        })
    }

    #[cfg(not(unix))]
    fn listen(
        _egress: CallEgress,
        _root: &Path,
        _resolver: Resolver,
        _permitted: fn(IpAddr) -> bool,
    ) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "the egress proxy needs Unix sockets, which this host does not have",
        ))
    }

    /// The host directory every call's socket lives under.
    #[must_use]
    pub fn socket_root(&self) -> &Path {
        &self.root
    }

    /// This call's socket on the host.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    /// Whether the accept loop is still running. The spawn gate checks this,
    /// not a proxy environment variable, before it admits a network policy.
    #[must_use]
    pub fn is_live(&self) -> bool {
        !self.task.is_finished() && self.socket.exists()
    }

    /// Stop the proxy and return the call's counts.
    #[must_use]
    pub fn finish(self) -> EgressCounts {
        let counts = self
            .tally
            .lock()
            .map(|tally| tally.counts())
            .unwrap_or_default();
        // `self` drops here: the loop stops and the directory goes.
        counts
    }
}

impl Drop for EgressProxy {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Serve one connection: read the CONNECT, decide, then tunnel or refuse.
#[cfg(unix)]
async fn serve(
    mut stream: tokio::net::UnixStream,
    egress: &CallEgress,
    tally: &Mutex<Tally>,
    resolver: &Resolver,
    permitted: fn(IpAddr) -> bool,
) {
    use tokio::io::AsyncWriteExt;

    let decision = match tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut stream)).await {
        Ok(Ok(head)) => decide(&head, egress, resolver, permitted).await,
        Ok(Err(_)) | Err(_) => Err((None, Denial::Malformed)),
    };
    match decision {
        Ok((host, mut upstream)) => {
            record(tally, &host, true);
            tracing::info!(host = %host, "sandbox egress: tunnel opened");
            if stream.write_all(ESTABLISHED).await.is_ok() {
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
            }
        }
        Err((host, denial)) => {
            if let Some(host) = &host {
                record(tally, host, false);
            }
            tracing::info!(
                host = host.as_deref().unwrap_or(""),
                reason = denial.label(),
                "sandbox egress: refused"
            );
            let _ = stream.write_all(denial.response()).await;
        }
    }
}

#[cfg(unix)]
fn record(tally: &Mutex<Tally>, host: &str, allowed: bool) {
    if let Ok(mut tally) = tally.lock() {
        tally.record(host, allowed);
    }
}

/// The decision for one request head: the canonical host and an open upstream
/// connection, or the denial with the host it named (when it named one).
#[cfg(unix)]
async fn decide(
    head: &[u8],
    egress: &CallEgress,
    resolver: &Resolver,
    permitted: fn(IpAddr) -> bool,
) -> Result<(String, tokio::net::TcpStream), (Option<String>, Denial)> {
    let (raw_host, port) = parse_connect(head).map_err(|denial| (None, denial))?;
    let host = admit_target(egress, &raw_host, port).map_err(|denial| (Some(raw_host), denial))?;
    let answers = (resolver.as_ref())(host.clone(), port)
        .await
        .map_err(|_| (Some(host.clone()), Denial::Unresolved))?;
    let address =
        choose_address(&answers, permitted).map_err(|denial| (Some(host.clone()), denial))?;
    match tokio::time::timeout(
        UPSTREAM_CONNECT_TIMEOUT,
        tokio::net::TcpStream::connect(address),
    )
    .await
    {
        Ok(Ok(upstream)) => Ok((host, upstream)),
        _ => Err((Some(host), Denial::Upstream)),
    }
}

/// Read up to the end of the request head (`\r\n\r\n`). A CONNECT client sends
/// nothing after its head until it has the reply, so nothing is lost by
/// stopping there.
#[cfg(unix)]
async fn read_head(stream: &mut tokio::net::UnixStream) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;

    let mut head = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_REQUEST_HEAD {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "request head too long",
            ));
        }
        if stream.read(&mut byte).await? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        head.push(byte[0]);
    }
    Ok(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registries() -> CallEgress {
        CallEgress::resolve(
            NetworkTier::Registries,
            NetworkTier::Registries,
            NetworkTier::All,
            &[],
        )
    }

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn the_call_tier_is_the_narrowest_ceiling() {
        use NetworkTier::{All, Allowlist, Off, Registries};
        assert_eq!(CallEgress::resolve(Off, Allowlist, All, &[]).tier, Off);
        assert_eq!(CallEgress::resolve(Allowlist, Off, All, &[]).tier, Off);
        assert_eq!(
            CallEgress::resolve(Allowlist, Allowlist, Off, &[]).tier,
            Off
        );
        assert_eq!(
            CallEgress::resolve(Registries, Allowlist, All, &[]).tier,
            Registries
        );
        assert_eq!(
            CallEgress::resolve(Allowlist, Registries, All, &[]).tier,
            Registries
        );
        assert_eq!(
            CallEgress::resolve(Allowlist, Allowlist, All, &[]).tier,
            Allowlist
        );
        // `all` is never granted, even when every ceiling names it.
        assert_eq!(CallEgress::resolve(All, All, All, &[]).tier, Off);
    }

    #[test]
    fn the_space_list_is_kept_only_at_the_allowlist_tier() {
        let list = vec!["data.example.org".to_owned()];
        let at_registries = CallEgress::resolve(
            NetworkTier::Allowlist,
            NetworkTier::Registries,
            NetworkTier::All,
            &list,
        );
        assert!(at_registries.allowlist.is_empty());
        let at_allowlist = CallEgress::resolve(
            NetworkTier::Allowlist,
            NetworkTier::Allowlist,
            NetworkTier::All,
            &list,
        );
        assert_eq!(at_allowlist.allowlist, list);
        assert!(at_allowlist
            .admitted_hosts()
            .contains(&"data.example.org".to_owned()));
    }

    #[test]
    fn an_untiered_call_gets_no_proxy_variables_and_no_hosts() {
        let off = CallEgress::off();
        assert!(!off.is_tiered());
        assert!(off.child_env().is_empty());
        assert!(off.admitted_hosts().is_empty());
        let tiered = registries().child_env();
        assert!(tiered.contains(&("HTTPS_PROXY".to_owned(), SANDBOX_PROXY_URL.to_owned())));
        assert!(tiered.contains(&(
            "npm_config_https_proxy".to_owned(),
            SANDBOX_PROXY_URL.to_owned()
        )));
    }

    #[test]
    fn only_a_well_formed_connect_parses() {
        assert_eq!(
            parse_connect(b"CONNECT pypi.org:443 HTTP/1.1\r\nHost: pypi.org:443\r\n\r\n"),
            Ok(("pypi.org".to_owned(), 443))
        );
        assert_eq!(
            parse_connect(b"GET http://pypi.org/simple/ HTTP/1.1\r\n\r\n"),
            Err(Denial::NotConnect)
        );
        for head in [
            &b"CONNECT pypi.org HTTP/1.1\r\n\r\n"[..],
            b"CONNECT pypi.org:https HTTP/1.1\r\n\r\n",
            b"CONNECT :443 HTTP/1.1\r\n\r\n",
            b"CONNECT pypi.org:443 SPDY/3\r\n\r\n",
            b"CONNECT pypi.org:443\r\n\r\n",
            b"\xff\xfe",
        ] {
            assert_eq!(parse_connect(head), Err(Denial::Malformed), "{head:?}");
        }
    }

    #[test]
    fn the_proxy_admits_a_listed_host_and_refuses_an_unlisted_one() {
        let egress = registries();
        assert_eq!(
            admit_target(&egress, "PyPI.org", 443),
            Ok("pypi.org".to_owned())
        );
        assert_eq!(
            admit_target(&egress, "example.org", 443),
            Err(Denial::NotListed)
        );
        assert_eq!(admit_target(&egress, "pypi.org", 80), Err(Denial::Port));
        assert_eq!(
            admit_target(&CallEgress::off(), "pypi.org", 443),
            Err(Denial::NotListed)
        );
    }

    /// `all` is never granted in Wave 3. `resolve` already holds it at off, and a call
    /// built without `resolve` must not admit a host either.
    #[test]
    fn a_call_carrying_all_admits_no_host() {
        let egress = CallEgress {
            tier: NetworkTier::All,
            allowlist: Vec::new(),
        };
        for host in ["pypi.org", "example.org"] {
            assert_eq!(
                admit_target(&egress, host, 443),
                Err(Denial::NotListed),
                "{host}"
            );
        }
    }

    #[test]
    fn the_proxy_refuses_an_ip_literal() {
        let egress = registries();
        for literal in [
            "151.101.0.223",
            "10.0.0.1",
            "169.254.169.254",
            "[2a04:4e42::223]",
            "::1",
        ] {
            assert_eq!(
                admit_target(&egress, literal, 443),
                Err(Denial::NotAHostName),
                "{literal}"
            );
        }
    }

    #[test]
    fn a_name_resolving_to_a_private_or_metadata_address_is_refused() {
        for ip in [
            v4(10, 0, 0, 1),
            v4(169, 254, 169, 254),
            v4(127, 0, 0, 1),
            v4(192, 168, 1, 1),
            v4(172, 16, 0, 1),
            v4(100, 64, 0, 1),
            v4(0, 0, 0, 0),
            v4(255, 255, 255, 255),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6("fd00::1".parse().unwrap()),
            IpAddr::V6("fe80::1".parse().unwrap()),
            IpAddr::V6("::ffff:169.254.169.254".parse().unwrap()),
            IpAddr::V6("64:ff9b::a00:1".parse().unwrap()),
        ] {
            assert!(!is_public_address(ip), "{ip} must not be public");
            assert_eq!(
                choose_address(&[SocketAddr::new(ip, 443)], is_public_address),
                Err(Denial::NonPublicAddress),
                "{ip}"
            );
        }
        for ip in [
            v4(151, 101, 0, 223),
            IpAddr::V6("2a04:4e42::223".parse().unwrap()),
        ] {
            assert!(is_public_address(ip), "{ip} is public");
        }
    }

    #[test]
    fn one_private_answer_among_public_ones_refuses_the_name() {
        let answers = [
            SocketAddr::new(v4(151, 101, 0, 223), 443),
            SocketAddr::new(v4(10, 0, 0, 1), 443),
        ];
        assert_eq!(
            choose_address(&answers, is_public_address),
            Err(Denial::NonPublicAddress)
        );
        assert_eq!(
            choose_address(&[], is_public_address),
            Err(Denial::Unresolved)
        );
    }

    #[test]
    fn the_tally_counts_distinct_hosts_case_blind() {
        let mut tally = Tally::default();
        tally.record("pypi.org", true);
        tally.record("PyPI.org", true);
        tally.record("example.org", false);
        tally.record("example.org", false);
        tally.record("10.0.0.1", false);
        assert_eq!(
            tally.counts(),
            EgressCounts {
                allowed_hosts: 1,
                denied_hosts: 2
            }
        );
        assert_eq!(
            serde_json::to_string(&tally.counts()).unwrap(),
            r#"{"allowed_hosts":1,"denied_hosts":2}"#
        );
    }

    #[test]
    fn an_untiered_call_cannot_start_a_proxy() {
        let err = EgressProxy::start_with(
            &CallEgress::off(),
            &std::env::temp_dir().join("verevon-egress-test-off"),
            system_resolver(),
            is_public_address,
        )
        .expect_err("no proxy for an untiered call");
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }

    #[test]
    fn a_lease_tier_reads_off_unless_it_names_a_grantable_tier() {
        use mp_contracts::model_plane::v1::SandboxEgressTier;
        assert_eq!(
            lease_tier(SandboxEgressTier::Registries as i32),
            NetworkTier::Registries
        );
        assert_eq!(
            lease_tier(SandboxEgressTier::Allowlist as i32),
            NetworkTier::Allowlist
        );
        for raw in [
            SandboxEgressTier::Unspecified as i32,
            SandboxEgressTier::Off as i32,
            SandboxEgressTier::All as i32,
            99,
            -1,
        ] {
            assert_eq!(lease_tier(raw), NetworkTier::Off, "{raw}");
        }
    }

    /// D-E3: ZDR, waived, plan-mode, tainted and read-only runs are held at
    /// off; any one of them is enough.
    #[test]
    fn a_run_that_must_not_leak_is_held_at_off() {
        assert_eq!(run_ceiling(RunEgressFacts::default()), NetworkTier::All);
        let each: [fn(&mut RunEgressFacts); 5] = [
            |facts| facts.zdr = true,
            |facts| facts.waived = true,
            |facts| facts.plan_mode = true,
            |facts| facts.tainted = true,
            |facts| facts.read_only_rung = true,
        ];
        for (index, set) in each.iter().enumerate() {
            let mut facts = RunEgressFacts::default();
            set(&mut facts);
            assert_eq!(run_ceiling(facts), NetworkTier::Off, "fact {index}");
            // Whatever the switch and the lease allow, the call is off.
            assert!(!CallEgress::resolve(
                NetworkTier::Allowlist,
                NetworkTier::Allowlist,
                run_ceiling(facts),
                &["data.example.org".to_owned()],
            )
            .is_tiered());
        }
    }

    fn registry() -> &'static RunEgressRegistry {
        Box::leak(Box::new(RunEgressRegistry::default()))
    }

    #[test]
    fn an_unregistered_run_is_off_whatever_its_lease_says() {
        let runs = registry();
        runs.record_lease(
            "org-a",
            "run-1",
            NetworkTier::Allowlist,
            &["data.example.org".to_owned()],
        );
        assert_eq!(
            runs.call_egress_with(NetworkTier::Allowlist, "org-a", "run-1"),
            CallEgress::off()
        );
    }

    #[test]
    fn a_registered_run_takes_the_narrowest_of_switch_lease_and_ceiling() {
        let runs = registry();
        let _guard = runs.register("org-a", "run-1");
        let list = vec!["data.example.org".to_owned()];
        // Nothing set yet: a fresh entry is off on both halves.
        runs.record_lease("org-a", "run-1", NetworkTier::Allowlist, &list);
        assert!(!runs
            .call_egress_with(NetworkTier::Allowlist, "org-a", "run-1")
            .is_tiered());
        runs.set_ceiling("org-a", "run-1", NetworkTier::All);
        let call = runs.call_egress_with(NetworkTier::Allowlist, "org-a", "run-1");
        assert_eq!(call.tier, NetworkTier::Allowlist);
        assert_eq!(call.allowlist, list);
        assert_eq!(
            runs.call_egress_with(NetworkTier::Registries, "org-a", "run-1")
                .tier,
            NetworkTier::Registries
        );
        assert!(!runs
            .call_egress_with(NetworkTier::Off, "org-a", "run-1")
            .is_tiered());
        // Another org's run of the same id sees nothing of it.
        assert!(!runs
            .call_egress_with(NetworkTier::Allowlist, "org-b", "run-1")
            .is_tiered());
    }

    #[test]
    fn a_run_whose_ceiling_drops_is_off_and_its_entry_goes_with_the_guard() {
        let runs = registry();
        let guard = runs.register("org-a", "run-1");
        runs.set_ceiling("org-a", "run-1", NetworkTier::All);
        runs.record_lease("org-a", "run-1", NetworkTier::Registries, &[]);
        assert!(runs
            .call_egress_with(NetworkTier::Registries, "org-a", "run-1")
            .is_tiered());
        // Tainted mid-run: the next call's ceiling is off, the lease unchanged.
        runs.set_ceiling(
            "org-a",
            "run-1",
            run_ceiling(RunEgressFacts {
                tainted: true,
                ..RunEgressFacts::default()
            }),
        );
        assert!(!runs
            .call_egress_with(NetworkTier::Registries, "org-a", "run-1")
            .is_tiered());
        drop(guard);
        runs.set_ceiling("org-a", "run-1", NetworkTier::All);
        runs.record_lease("org-a", "run-1", NetworkTier::Registries, &[]);
        assert_eq!(
            runs.call_egress_with(NetworkTier::Registries, "org-a", "run-1"),
            CallEgress::off()
        );
    }

    #[test]
    fn the_switch_reads_off_unless_its_exact_word_is_set() {
        // The process-wide value is whatever the test environment holds; the
        // parse rule itself is pinned in mp-contracts. Here: the value is one
        // of the three a switch can produce.
        assert!(matches!(
            switch_tier(),
            NetworkTier::Off | NetworkTier::Registries | NetworkTier::Allowlist
        ));
    }

    /// End to end through a real socket: a resolver that maps every name to a
    /// private address is refused with 403, a refused host is counted, and the
    /// directory is gone after `finish`.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_proxy_refuses_a_listed_name_that_resolves_privately() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let root = std::env::temp_dir().join(format!("verevon-egress-test-{}", std::process::id()));
        let private: Resolver = Arc::new(|_host: String, port: u16| {
            Box::pin(async move { Ok(vec![SocketAddr::new(v4(10, 0, 0, 1), port)]) })
        });
        let proxy = EgressProxy::start_with(&registries(), &root, private, is_public_address)
            .expect("the proxy starts");
        assert!(proxy.is_live());

        let mut client = tokio::net::UnixStream::connect(proxy.socket_path())
            .await
            .expect("connect");
        client
            .write_all(b"CONNECT pypi.org:443 HTTP/1.1\r\nHost: pypi.org:443\r\n\r\n")
            .await
            .unwrap();
        let mut reply = Vec::new();
        client.read_to_end(&mut reply).await.unwrap();
        assert!(
            reply.starts_with(b"HTTP/1.1 403"),
            "{}",
            String::from_utf8_lossy(&reply)
        );

        let mut unlisted = tokio::net::UnixStream::connect(proxy.socket_path())
            .await
            .expect("connect");
        unlisted
            .write_all(b"CONNECT example.org:443 HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let mut reply = Vec::new();
        unlisted.read_to_end(&mut reply).await.unwrap();
        assert!(reply.starts_with(b"HTTP/1.1 403"));

        let dir = proxy.socket_path().parent().unwrap().to_path_buf();
        let counts = proxy.finish();
        assert_eq!(
            counts,
            EgressCounts {
                allowed_hosts: 0,
                denied_hosts: 2
            }
        );
        assert!(!dir.exists(), "the call's socket directory is removed");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The admitted path: a listed name whose answer the (test-only) guard
    /// accepts is tunnelled to it, bytes flow both ways, and it counts as allowed.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_proxy_tunnels_a_listed_host_it_checked() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut conn, _) = upstream.accept().await.unwrap();
            let mut buf = [0u8; 4];
            conn.read_exact(&mut buf).await.unwrap();
            conn.write_all(b"pong").await.unwrap();
        });

        let root =
            std::env::temp_dir().join(format!("verevon-egress-test-ok-{}", std::process::id()));
        let resolver: Resolver = Arc::new(move |_host: String, _port: u16| {
            Box::pin(async move { Ok(vec![upstream_addr]) })
        });
        let proxy = EgressProxy::start_with(&registries(), &root, resolver, |_| true)
            .expect("the proxy starts");
        let mut client = tokio::net::UnixStream::connect(proxy.socket_path())
            .await
            .unwrap();
        client
            .write_all(b"CONNECT files.pythonhosted.org:443 HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let mut established = vec![0u8; ESTABLISHED.len()];
        client.read_exact(&mut established).await.unwrap();
        assert_eq!(established, ESTABLISHED);
        client.write_all(b"ping").await.unwrap();
        let mut pong = [0u8; 4];
        client.read_exact(&mut pong).await.unwrap();
        assert_eq!(&pong, b"pong");
        assert_eq!(
            proxy.finish(),
            EgressCounts {
                allowed_hosts: 1,
                denied_hosts: 0
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
