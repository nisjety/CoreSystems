//! Persistent CDP/Browserless session lifecycle.
//!
//! Quarry's default browser drivers fire one-shot POSTs per page — fine for
//! cold paths but expensive when an agent needs many actions on the same
//! domain (cookie persistence, JS heap reuse, lower per-page latency). This
//! module manages keep-alive sessions:
//!
//! - Browserless: `POST /sessions` returns `{connectionId, wsEndpoint, ttl}`
//!   and the connection is reused across pages until `DELETE
//!   /sessions/{id}` or the TTL expires.
//! - Direct CDP: persistent WS connection to a Chromiumoxide instance, with
//!   an idle timer that reaps the connection if no requests arrive.
//!
//! The implementation here is transport-agnostic; concrete drivers compose it
//! by storing the [`PersistentSession`] in their session inner state.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentSession {
    pub connection_id: String,
    pub ws_endpoint: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub ttl_s: u64,
}

impl PersistentSession {
    pub fn new(
        connection_id: impl Into<String>,
        ws_endpoint: impl Into<String>,
        ttl_s: u64,
    ) -> Self {
        let now = Utc::now();
        let expires_at = if ttl_s > 0 {
            now.checked_add_signed(chrono::Duration::seconds(ttl_s as i64))
        } else {
            None
        };
        Self {
            connection_id: connection_id.into(),
            ws_endpoint: ws_endpoint.into(),
            created_at: now,
            expires_at,
            ttl_s,
        }
    }

    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => exp < Utc::now(),
            None => false,
        }
    }

    pub fn time_remaining(&self) -> Option<Duration> {
        let exp = self.expires_at?;
        let secs = (exp - Utc::now()).num_seconds();
        if secs <= 0 {
            Some(Duration::from_secs(0))
        } else {
            Some(Duration::from_secs(secs as u64))
        }
    }
}

/// Per-process registry of live persistent sessions, keyed by `lease_id`.
///
/// The registry implements an idle reaper: any session that hasn't been
/// touched within `idle_timeout` is dropped (a background task handles
/// cleanup; consumers see `Err(SessionNotFound)` if they look up a reaped
/// session).
pub struct PersistentSessionRegistry {
    sessions: Arc<Mutex<HashMap<String, RegistryEntry>>>,
    idle_timeout: Duration,
}

struct RegistryEntry {
    session: PersistentSession,
    last_touched: Instant,
}

impl PersistentSessionRegistry {
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            idle_timeout,
        }
    }

    pub async fn insert(&self, lease_id: String, session: PersistentSession) {
        let mut guard = self.sessions.lock().await;
        guard.insert(
            lease_id,
            RegistryEntry {
                session,
                last_touched: Instant::now(),
            },
        );
    }

    pub async fn get(&self, lease_id: &str) -> QuarryResult<PersistentSession> {
        let mut guard = self.sessions.lock().await;
        let entry = guard.get_mut(lease_id).ok_or_else(|| {
            QuarryError::new(ErrorCode::NotFound, format!("session {lease_id} not found"))
        })?;
        if entry.session.is_expired() {
            guard.remove(lease_id);
            return Err(QuarryError::new(
                ErrorCode::Timeout,
                format!("session {lease_id} expired"),
            ));
        }
        entry.last_touched = Instant::now();
        Ok(entry.session.clone())
    }

    pub async fn remove(&self, lease_id: &str) -> Option<PersistentSession> {
        let mut guard = self.sessions.lock().await;
        guard.remove(lease_id).map(|e| e.session)
    }

    pub async fn len(&self) -> usize {
        self.sessions.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Reap idle sessions. Call from a periodic background task.
    pub async fn reap_idle(&self) -> Vec<String> {
        let mut guard = self.sessions.lock().await;
        let now = Instant::now();
        let timeout = self.idle_timeout;
        let to_remove: Vec<String> = guard
            .iter()
            .filter_map(|(k, v)| {
                let idle = now.duration_since(v.last_touched);
                if idle > timeout || v.session.is_expired() {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();
        for k in &to_remove {
            guard.remove(k);
        }
        to_remove
    }
}

impl Default for PersistentSessionRegistry {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_with_ttl_has_expiry() {
        let s = PersistentSession::new("conn_1", "ws://x", 60);
        assert!(!s.is_expired());
        assert!(s.expires_at.is_some());
        let remaining = s.time_remaining().unwrap();
        assert!(remaining.as_secs() > 50 && remaining.as_secs() <= 60);
    }

    #[test]
    fn session_without_ttl_never_expires() {
        let s = PersistentSession::new("conn_1", "ws://x", 0);
        assert!(!s.is_expired());
        assert!(s.expires_at.is_none());
        assert!(s.time_remaining().is_none());
    }

    #[tokio::test]
    async fn registry_insert_and_get() {
        let r = PersistentSessionRegistry::default();
        let s = PersistentSession::new("c1", "ws://x", 60);
        r.insert("lease_a".into(), s.clone()).await;

        let fetched = r.get("lease_a").await.unwrap();
        assert_eq!(fetched.connection_id, "c1");
    }

    #[tokio::test]
    async fn registry_get_unknown_returns_not_found() {
        let r = PersistentSessionRegistry::default();
        let err = r.get("unknown").await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn registry_get_expired_returns_timeout_and_removes() {
        let r = PersistentSessionRegistry::default();
        let mut s = PersistentSession::new("c1", "ws://x", 0);
        s.expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
        r.insert("lease_a".into(), s).await;

        let err = r.get("lease_a").await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Timeout);
        assert_eq!(r.len().await, 0);
    }

    #[tokio::test]
    async fn registry_reaps_idle_sessions() {
        let r = PersistentSessionRegistry::new(Duration::from_millis(10));
        let s = PersistentSession::new("c1", "ws://x", 60);
        r.insert("lease_a".into(), s).await;

        tokio::time::sleep(Duration::from_millis(30)).await;
        let reaped = r.reap_idle().await;
        assert_eq!(reaped, vec!["lease_a".to_string()]);
        assert_eq!(r.len().await, 0);
    }

    #[tokio::test]
    async fn registry_remove_works() {
        let r = PersistentSessionRegistry::default();
        let s = PersistentSession::new("c1", "ws://x", 60);
        r.insert("lease_a".into(), s).await;
        let removed = r.remove("lease_a").await;
        assert!(removed.is_some());
        assert_eq!(r.len().await, 0);
    }
}
