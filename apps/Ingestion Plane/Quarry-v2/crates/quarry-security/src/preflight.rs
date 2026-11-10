//! Default SecurityEngine impl: heuristic + discovered-URL same-origin scoping.
//! Blocklist/allowlist backends are pluggable via trait; default is in-memory
//! with optional file-backed persistence (atomic JSON snapshot).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use url::Url;

use crate::{heur, Decision, SecurityEngine, Verdict};

/// On-disk snapshot of the host blocklist/allowlist.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ListSnapshot {
    #[serde(default)]
    pub blocklist: Vec<String>,
    #[serde(default)]
    pub allowlist: Vec<String>,
}

#[derive(Default, Clone)]
pub struct DefaultEngine {
    host_blocklist: Arc<RwLock<HashSet<String>>>,
    host_allowlist: Arc<RwLock<HashSet<String>>>,
    allow_private_hosts: bool,
    /// When set, mutations auto-persist a snapshot to this path.
    persist_path: Option<PathBuf>,
    /// Dot-suffix matching for the blocklist (D7 rule). Off unless set.
    match_subdomains: bool,
}

impl DefaultEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test-only opt-in: skip the heuristic private/loopback host block.
    /// Explicit blocklist/allowlist still apply.
    pub fn with_allow_private_hosts(mut self, v: bool) -> Self {
        self.allow_private_hosts = v;
        self
    }

    /// Configure file-backed persistence. Loads existing snapshot if present;
    /// subsequent `block_host`/`allow_host` calls auto-save atomically.
    pub async fn with_persistence(mut self, path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        if path.exists() {
            let snap = Self::load_snapshot(&path).await?;
            let mut bl = self.host_blocklist.write().await;
            for h in snap.blocklist {
                bl.insert(h);
            }
            drop(bl);
            let mut al = self.host_allowlist.write().await;
            for h in snap.allowlist {
                al.insert(h);
            }
        }
        self.persist_path = Some(path);
        Ok(self)
    }

    /// Load a snapshot from disk without mutating state.
    pub async fn load_snapshot(path: &Path) -> std::io::Result<ListSnapshot> {
        let bytes = tokio::fs::read(path).await?;
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Persist current state to disk via atomic temp-file rename.
    pub async fn save_snapshot(&self, path: &Path) -> std::io::Result<()> {
        let snap = ListSnapshot {
            blocklist: self.host_blocklist.read().await.iter().cloned().collect(),
            allowlist: self.host_allowlist.read().await.iter().cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&snap)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
        }
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, &bytes).await?;
        tokio::fs::rename(&tmp, path).await?;
        Ok(())
    }

    async fn maybe_persist(&self) {
        if let Some(p) = &self.persist_path {
            if let Err(e) = self.save_snapshot(p).await {
                tracing::warn!(error_class = ?e.kind(), path = %p.display(), "blocklist persist failed");
            }
        }
    }

    pub async fn block_host(&self, host: impl Into<String>) {
        self.host_blocklist.write().await.insert(host.into());
        self.maybe_persist().await;
    }

    pub async fn allow_host(&self, host: impl Into<String>) {
        self.host_allowlist.write().await.insert(host.into());
        self.maybe_persist().await;
    }

    /// Set whether a blocklist entry also blocks its subdomains (dot-suffix
    /// match). Off by default; the feed sync can name hosts whose subdomains
    /// are unrelated (for example a shared hosting domain), so this is an
    /// explicit operator opt-in.
    pub fn with_subdomain_matching(mut self, on: bool) -> Self {
        self.match_subdomains = on;
        self
    }

    /// Entry counts of the two host lists. Counts only; no host name leaves.
    pub async fn list_counts(&self) -> ListCounts {
        ListCounts {
            blocklist: self.host_blocklist.read().await.len(),
            allowlist: self.host_allowlist.read().await.len(),
        }
    }

    /// Replace both host lists from a snapshot file (hot reload). The file is
    /// parsed before any lock is taken; on error the previous lists stay in
    /// force. Both write guards are held across the swap so a preflight never
    /// sees a new blocklist beside an old allowlist. preflight takes one list
    /// at a time, so this lock order cannot deadlock.
    pub async fn reload_snapshot(&self, path: &Path) -> std::io::Result<ListCounts> {
        let snap = Self::load_snapshot(path).await?;
        let block: HashSet<String> = snap.blocklist.into_iter().collect();
        let allow: HashSet<String> = snap.allowlist.into_iter().collect();
        let counts = ListCounts {
            blocklist: block.len(),
            allowlist: allow.len(),
        };
        let mut bl = self.host_blocklist.write().await;
        let mut al = self.host_allowlist.write().await;
        *bl = block;
        *al = allow;
        Ok(counts)
    }

    /// True when the URL host is on the blocklist, exactly or (when subdomain
    /// matching is on) through a listed parent domain: `evil.example` also
    /// blocks `a.b.evil.example`.
    async fn blocklist_hit(&self, u: &Url) -> bool {
        let Some(h) = u.host_str() else {
            return false;
        };
        let list = self.host_blocklist.read().await;
        host_on_list(&list, h, self.match_subdomains)
    }
}

/// Whether `host` is on `list`, exactly or (when `match_subdomains` is on)
/// through a listed parent domain. One matcher for the scrape preflight and the
/// browser reputation hook (E3.8.7), so the two paths can never disagree.
fn host_on_list(list: &HashSet<String>, host: &str, match_subdomains: bool) -> bool {
    if list.contains(host) {
        return true;
    }
    if !match_subdomains {
        return false;
    }
    let mut rest = host;
    while let Some(idx) = rest.find('.') {
        rest = &rest[idx + 1..];
        if list.contains(rest) {
            return true;
        }
    }
    false
}

/// Entry counts of the two host lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListCounts {
    pub blocklist: usize,
    pub allowlist: usize,
}

/// `QUARRY_EDGE__REPUTATION_MODE` (E3.8.7, D2-100): the deployment's browser URL
/// reputation mode, `off` (the default), `warn` or `block`. It is a kill switch
/// as well as a default: with it off no org setting turns the hook on.
pub const REPUTATION_MODE_ENV: &str = "QUARRY_EDGE__REPUTATION_MODE";

/// What the browser egress boundary does with a host the reputation lists name
/// (E3.8.7). `Off` never consults the lists, so a policy built with it behaves
/// exactly as one built before the hook existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReputationMode {
    #[default]
    Off,
    /// Allow the request and record a `reputation_warned` receipt.
    Warn,
    /// Refuse the request and record a `reputation_blocked` receipt.
    Block,
}

impl ReputationMode {
    /// Parse a mode value. Anything but `warn` or `block` (case and surrounding
    /// space ignored) is `Off`: an unreadable value never turns blocking on.
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
            Some("warn") => Self::Warn,
            Some("block") => Self::Block,
            _ => Self::Off,
        }
    }

    /// The deployment mode from [`REPUTATION_MODE_ENV`].
    pub fn from_env() -> Self {
        Self::parse(std::env::var(REPUTATION_MODE_ENV).ok().as_deref())
    }

    /// The wire name: `off`, `warn` or `block`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Warn => "warn",
            Self::Block => "block",
        }
    }

    /// The mode one org runs under. `self` is the deployment mode: when it is
    /// `Off` the answer is `Off` whatever the org says (the flag is dark); when
    /// it is on, the org's own setting wins, and an org without one gets the
    /// deployment mode.
    pub fn for_org(self, org: Option<ReputationMode>) -> Self {
        if self == Self::Off {
            return Self::Off;
        }
        org.unwrap_or(self)
    }
}

/// What a reputation source knows about one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostStanding {
    /// Not on any list.
    Unlisted,
    /// On a block list.
    Listed,
    /// The source could not answer without waiting (for example a list swap in
    /// progress). The browser boundary treats it as `Unlisted`.
    Unknown,
}

/// A host reputation source the browser egress boundary consults for every
/// http(s) request it decides: navigations, redirect hops, clicks that
/// navigate, frames and sub-resources (E3.8.7). It is synchronous and must
/// never block: the boundary calls it inside the CDP Fetch listener.
pub trait HostReputation: Send + Sync {
    /// The standing of `host`, lower-case with no trailing dot.
    fn standing(&self, host: &str) -> HostStanding;

    /// The stable name of this source, for logs and receipts. No host or URL.
    fn source(&self) -> &'static str {
        "security_snapshot"
    }
}

impl HostReputation for DefaultEngine {
    /// The same blocklist (and subdomain rule) the scrape preflight uses, read
    /// without waiting. A reload holds the write lock only for the swap; a
    /// lookup that lands inside it answers `Unknown`.
    fn standing(&self, host: &str) -> HostStanding {
        match self.host_blocklist.try_read() {
            Ok(list) if host_on_list(&list, host, self.match_subdomains) => HostStanding::Listed,
            Ok(_) => HostStanding::Unlisted,
            Err(_) => HostStanding::Unknown,
        }
    }
}

/// Watch the snapshot at `path` by modification time and hot-reload the
/// engine's lists when it changes. The file is read once at boot before this
/// is called, so the first mtime is taken at spawn and only a later change
/// triggers a reload. A failed reload keeps the previous lists in force,
/// reports the error kind to `on_reload`, and waits for the next change.
/// The interval is clamped to at least one second.
pub fn spawn_snapshot_watch<F>(
    engine: &DefaultEngine,
    path: impl Into<PathBuf>,
    interval: Duration,
    on_reload: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn(Result<ListCounts, std::io::ErrorKind>) + Send + 'static,
{
    let path: PathBuf = path.into();
    let engine = engine.clone();
    let interval = interval.max(Duration::from_secs(1));
    tokio::spawn(async move {
        let mut last = file_mtime(&path).await;
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick completes at once; the first real check is one interval out.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let now = file_mtime(&path).await;
            if now.is_none() || now == last {
                continue;
            }
            last = now;
            match engine.reload_snapshot(&path).await {
                Ok(counts) => {
                    tracing::info!(
                        blocklist = counts.blocklist,
                        allowlist = counts.allowlist,
                        "reputation snapshot reloaded"
                    );
                    on_reload(Ok(counts));
                }
                Err(error) => {
                    tracing::warn!(
                        error_class = ?error.kind(),
                        "reputation snapshot reload failed; previous lists stay in force"
                    );
                    on_reload(Err(error.kind()));
                }
            }
        }
    })
}

async fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    tokio::fs::metadata(path).await.ok()?.modified().ok()
}

#[async_trait]
impl SecurityEngine for DefaultEngine {
    async fn preflight(&self, u: &Url) -> Verdict {
        if self.blocklist_hit(u).await {
            return Verdict::block(format!("host blocklisted: {}", u.host_str().unwrap_or("")));
        }
        if !heur::scheme_allowed(u) {
            return Verdict::block(format!("disallowed scheme: {}", u.scheme()));
        }
        if !self.allow_private_hosts {
            if let Some(r) = heur::blocks_private_host(u) {
                return Verdict::block(r);
            }
        }
        if u.password().is_some() || !u.username().is_empty() {
            return Verdict::block("userinfo in URL");
        }
        // Allowlist mode: if allowlist non-empty and host not on it, escalate.
        let allow = self.host_allowlist.read().await;
        if !allow.is_empty() {
            if let Some(h) = u.host_str() {
                if !allow.contains(h) {
                    return Verdict {
                        decision: Decision::Escalate,
                        reasons: vec![format!("host not on allowlist: {h}")],
                        risk_score: 50,
                    };
                }
            }
        }
        Verdict::allow()
    }

    async fn check_discovered(&self, parent: &Url, child: &Url) -> Verdict {
        // Parent re-preflight first (cheap; covers schema drift).
        let base = self.preflight(child).await;
        if base.decision == Decision::Block {
            return base;
        }

        // Out-of-scope = escalate, not block, so crawl policy can decide.
        if parent.host_str() != child.host_str() {
            return Verdict {
                decision: Decision::Escalate,
                reasons: vec![format!(
                    "cross-origin: {} → {}",
                    parent.host_str().unwrap_or(""),
                    child.host_str().unwrap_or(""),
                )],
                risk_score: 30,
            };
        }
        base
    }

    fn allow_private_hosts(&self) -> bool {
        self.allow_private_hosts
    }

    /// This engine's blocklist as a browser reputation source (E3.8.7). The clone
    /// shares the lists with `self`, so a hot reload shows through it.
    fn host_reputation(&self) -> Option<Arc<dyn HostReputation>> {
        Some(Arc::new(self.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn persist_and_reload_blocklist() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("quarry_sec_{nanos}.json"));
        let _ = std::fs::remove_file(&path);

        let eng = DefaultEngine::new()
            .with_persistence(path.clone())
            .await
            .unwrap();
        eng.block_host("evil.example").await;
        eng.allow_host("good.example").await;

        let eng2 = DefaultEngine::new()
            .with_persistence(path.clone())
            .await
            .unwrap();
        let v = eng2
            .preflight(&Url::parse("https://evil.example/").unwrap())
            .await;
        assert_eq!(v.decision, Decision::Block);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reputation_mode_parses_off_unless_warn_or_block() {
        assert_eq!(ReputationMode::parse(None), ReputationMode::Off);
        assert_eq!(ReputationMode::parse(Some("")), ReputationMode::Off);
        assert_eq!(ReputationMode::parse(Some("off")), ReputationMode::Off);
        assert_eq!(ReputationMode::parse(Some("yes")), ReputationMode::Off);
        assert_eq!(ReputationMode::parse(Some(" Warn ")), ReputationMode::Warn);
        assert_eq!(ReputationMode::parse(Some("BLOCK")), ReputationMode::Block);
        assert_eq!(ReputationMode::default(), ReputationMode::Off);
        for mode in [ReputationMode::Off, ReputationMode::Warn, ReputationMode::Block] {
            assert_eq!(ReputationMode::parse(Some(mode.as_str())), mode);
        }
    }

    #[test]
    fn an_org_mode_applies_only_when_the_deployment_flag_is_on() {
        let off = ReputationMode::Off;
        assert_eq!(off.for_org(Some(ReputationMode::Block)), ReputationMode::Off);
        assert_eq!(off.for_org(None), ReputationMode::Off);
        let warn = ReputationMode::Warn;
        assert_eq!(warn.for_org(None), ReputationMode::Warn);
        assert_eq!(warn.for_org(Some(ReputationMode::Block)), ReputationMode::Block);
        assert_eq!(warn.for_org(Some(ReputationMode::Off)), ReputationMode::Off);
        let block = ReputationMode::Block;
        assert_eq!(block.for_org(Some(ReputationMode::Warn)), ReputationMode::Warn);
    }

    #[tokio::test]
    async fn the_engine_answers_host_standing_from_its_blocklist() {
        let exact = DefaultEngine::new();
        exact.block_host("evil.example").await;
        assert_eq!(exact.standing("evil.example"), HostStanding::Listed);
        assert_eq!(exact.standing("a.evil.example"), HostStanding::Unlisted);
        assert_eq!(exact.standing("good.example"), HostStanding::Unlisted);
        assert_eq!(HostReputation::source(&exact), "security_snapshot");

        let suffix = DefaultEngine::new().with_subdomain_matching(true);
        suffix.block_host("evil.example").await;
        assert_eq!(suffix.standing("a.b.evil.example"), HostStanding::Listed);
        assert_eq!(suffix.standing("notevil.example"), HostStanding::Unlisted);
    }

    #[tokio::test]
    async fn a_lookup_during_a_list_swap_answers_unknown_without_waiting() {
        let engine = DefaultEngine::new();
        engine.block_host("evil.example").await;
        let held = engine.host_blocklist.write().await;
        assert_eq!(engine.standing("evil.example"), HostStanding::Unknown);
        drop(held);
        assert_eq!(engine.standing("evil.example"), HostStanding::Listed);
    }
}
