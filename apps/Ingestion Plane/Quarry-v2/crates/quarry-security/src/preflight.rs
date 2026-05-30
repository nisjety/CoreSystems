//! Default SecurityEngine impl: heuristic + discovered-URL same-origin scoping.
//! Blocklist/allowlist backends are pluggable via trait; default is in-memory
//! with optional file-backed persistence (atomic JSON snapshot).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

#[derive(Default)]
pub struct DefaultEngine {
    host_blocklist: Arc<RwLock<HashSet<String>>>,
    host_allowlist: Arc<RwLock<HashSet<String>>>,
    allow_private_hosts: bool,
    /// When set, mutations auto-persist a snapshot to this path.
    persist_path: Option<PathBuf>,
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
                tracing::warn!(error = %e, path = %p.display(), "blocklist persist failed");
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

    async fn host_listed(&self, u: &Url, list: &Arc<RwLock<HashSet<String>>>) -> bool {
        let Some(h) = u.host_str() else {
            return false;
        };
        list.read().await.contains(h)
    }
}

#[async_trait]
impl SecurityEngine for DefaultEngine {
    async fn preflight(&self, u: &Url) -> Verdict {
        if self.host_listed(u, &self.host_blocklist).await {
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
}
