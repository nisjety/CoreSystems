//! PKCE-backed [`TokenProvider`]: mints access tokens for the sync backend from
//! the refresh tokens captured by the onboarding flow.
//!
//! Access tokens are cached per tenant and connection version; acquisition is
//! serialized by a single async mutex so concurrent sync units never trigger
//! overlapping refreshes — Xero rotates the refresh token on every use, so an
//! overlapping refresh would invalidate it. Rotated refresh tokens are
//! persisted back to the [`ConnectionStore`].

use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use xero_common::Error;
use xero_store::ConnectionStore;

use crate::{authorization_code, TokenData, TokenProvider};

pub struct PkceTokenProvider {
    http: reqwest::Client,
    client_id: String,
    client_secret: Option<String>,
    store: Arc<dyn ConnectionStore>,
    /// tenant_id → access token from the current onboarding connection.
    cache: Mutex<HashMap<String, CachedToken>>,
}

struct CachedToken {
    connected_at: DateTime<Utc>,
    token: TokenData,
}

impl PkceTokenProvider {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: Option<String>,
        store: Arc<dyn ConnectionStore>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("xero_service_v2/0.1")
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client for PkceTokenProvider");
        Self {
            http,
            client_id: client_id.into(),
            client_secret,
            store,
            cache: Mutex::new(HashMap::new()),
        }
    }

    async fn connection_version(&self, tenant_id: &str) -> xero_common::Result<DateTime<Utc>> {
        self.store
            .list()
            .await
            .map_err(|e| Error::Auth(format!("store connection list failed: {e}")))?
            .into_iter()
            .find(|conn| conn.tenant_id == tenant_id)
            .map(|conn| conn.connected_at)
            .ok_or_else(|| Error::Auth(format!("no PKCE connection stored for tenant {tenant_id}")))
    }
}

#[async_trait::async_trait]
impl TokenProvider for PkceTokenProvider {
    async fn get_valid_token(&self, tenant_id: &str) -> xero_common::Result<TokenData> {
        // Hold the async mutex across the refresh so at most one refresh is ever
        // in flight — prevents refresh-token rotation races between concurrent
        // sync units. Refreshes are rare (then cached), so global serialization
        // is a non-issue.
        let mut cache = self.cache.lock().await;
        let connected_at = self.connection_version(tenant_id).await?;

        if let Some(entry) = cache.get(tenant_id) {
            if entry.connected_at == connected_at && !entry.token.needs_refresh(120) {
                return Ok(entry.token.clone());
            }
        }
        cache.remove(tenant_id);

        let lease = self
            .store
            .acquire_refresh_lock()
            .await
            .map_err(|e| Error::Auth(format!("store refresh lease failed: {e}")))?;
        let refresh_result = tokio::time::timeout(Duration::from_secs(120), async {
            // Onboarding may have replaced the connection while we waited for
            // the refresh lease; this is the version of the token we now read.
            let connected_at = self.connection_version(tenant_id).await?;
            let refresh_token = self
                .store
                .get_refresh_token(tenant_id)
                .await
                .map_err(|e| Error::Auth(format!("store read failed: {e}")))?
                .ok_or_else(|| {
                    Error::Auth(format!(
                        "no PKCE refresh token stored for tenant {tenant_id} (onboard it first)"
                    ))
                })?;
            let resp = authorization_code::refresh(
                &self.http,
                &self.client_id,
                self.client_secret.as_deref(),
                &refresh_token,
            )
            .await?;
            if resp.refresh_token.is_empty() {
                return Err(Error::Auth(
                    "Xero refresh response omitted refresh_token".into(),
                ));
            }
            self.store
                .update_refresh_token(&lease, tenant_id, &refresh_token, &resp.refresh_token)
                .await
                .map_err(|e| Error::Auth(format!("store rotate failed: {e}")))?;
            Ok((resp, connected_at))
        })
        .await
        .unwrap_or_else(|_| Err(Error::Auth("PKCE refresh exceeded 120 seconds".into())));
        self.store
            .release_refresh_lock(lease)
            .await
            .map_err(|e| Error::Auth(format!("store refresh lease release failed: {e}")))?;
        let (resp, connected_at) = refresh_result?;

        let expires_at = resp.expires_at();
        let scopes = resp.scopes();
        let token = TokenData {
            access_token: resp.access_token,
            refresh_token: resp.refresh_token,
            expires_at,
            scopes,
            tenant_id: tenant_id.to_owned(),
        };
        cache.insert(
            tenant_id.to_owned(),
            CachedToken {
                connected_at,
                token: token.clone(),
            },
        );
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use xero_store::{FileConnectionStore, StoredConnection};

    fn connection(connected_at: chrono::DateTime<Utc>, refresh_token: &str) -> StoredConnection {
        StoredConnection {
            tenant_id: "tenant-1".into(),
            tenant_name: "Test organisation".into(),
            tenant_type: "ORGANISATION".into(),
            connected_at,
            scopes: vec!["offline_access".into()],
            refresh_token: refresh_token.into(),
        }
    }

    fn access_token(value: &str, refresh_token: &str) -> TokenData {
        TokenData {
            access_token: value.into(),
            refresh_token: refresh_token.into(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            scopes: vec!["offline_access".into()],
            tenant_id: "tenant-1".into(),
        }
    }

    #[tokio::test]
    async fn reconnect_to_same_tenant_never_uses_previous_users_cached_access_token() {
        let path = std::env::temp_dir().join(format!(
            "xero-auth-reconnect-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let store = Arc::new(FileConnectionStore::load(path.join("connections.json")).unwrap());
        let first_version = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let second_version = Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap();
        store
            .upsert(connection(first_version, "first-user-refresh"))
            .await
            .unwrap();

        let mut provider = PkceTokenProvider::new("test-client", None, store.clone());
        provider.http = reqwest::Client::builder()
            .no_proxy()
            .resolve("identity.xero.com", "127.0.0.1:1".parse().unwrap())
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        provider.cache.lock().await.insert(
            "tenant-1".into(),
            CachedToken {
                connected_at: first_version,
                token: access_token("first-user-access", "first-user-refresh"),
            },
        );
        assert_eq!(
            provider
                .get_valid_token("tenant-1")
                .await
                .unwrap()
                .access_token,
            "first-user-access"
        );

        store
            .upsert(connection(second_version, "second-user-refresh"))
            .await
            .unwrap();
        let result = provider.get_valid_token("tenant-1").await;
        assert!(
            result.is_err(),
            "old cached access token survived reconnect"
        );
        assert_eq!(
            store
                .get_refresh_token("tenant-1")
                .await
                .unwrap()
                .as_deref(),
            Some("second-user-refresh")
        );
        let lease = tokio::time::timeout(Duration::from_secs(1), store.acquire_refresh_lock())
            .await
            .expect("failed refresh retained the lease")
            .unwrap();
        store.release_refresh_lock(lease).await.unwrap();

        provider.cache.lock().await.insert(
            "tenant-1".into(),
            CachedToken {
                connected_at: second_version,
                token: access_token("second-user-access", "second-user-refresh"),
            },
        );
        assert_eq!(
            provider
                .get_valid_token("tenant-1")
                .await
                .unwrap()
                .access_token,
            "second-user-access"
        );

        std::fs::remove_dir_all(path).unwrap();
    }
}
