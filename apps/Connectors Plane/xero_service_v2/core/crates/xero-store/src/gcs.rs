//! Shared PKCE store backed by one generation-guarded GCS object.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gcloud_storage::{
    client::{Client, ClientConfig},
    http::{
        objects::{
            download::Range,
            get::GetObjectRequest,
            upload::{Media, UploadObjectRequest, UploadType},
        },
        Error as GcsError,
    },
};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use crate::{ConnectionStore, RefreshLock, RefreshLockKind, StoredConnection};

const CAS_ATTEMPTS: usize = 12;
const LEASE_WAIT: Duration = Duration::from_secs(90);
const LEASE_POLL: Duration = Duration::from_millis(400);
const LEASE_TTL_SECONDS: i64 = 300;

pub struct GcsConnectionStore {
    client: Client,
    bucket: String,
    object: String,
    kms_key_name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct LeaseRecord {
    owner: Uuid,
    expires_at: DateTime<Utc>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct StoreDocument {
    connections: Vec<StoredConnection>,
    refresh_lease: Option<LeaseRecord>,
}

fn is_status(error: &GcsError, status: u16) -> bool {
    matches!(error, GcsError::Response(response) if response.code == status)
}

fn replace_connections(
    current: &[StoredConnection],
    additions: &[StoredConnection],
) -> Result<Vec<StoredConnection>> {
    anyhow::ensure!(
        additions
            .iter()
            .all(|conn| !conn.tenant_id.is_empty() && !conn.refresh_token.is_empty()),
        "connected tenant and refresh token must not be empty"
    );
    if let Some(first) = additions.first() {
        anyhow::ensure!(
            additions
                .iter()
                .all(|conn| conn.refresh_token == first.refresh_token),
            "one onboarding batch must contain one refresh-token grant"
        );
    }
    let mut by_tenant: BTreeMap<String, StoredConnection> = current
        .iter()
        .map(|conn| (conn.tenant_id.clone(), conn.clone()))
        .collect();
    for conn in additions {
        by_tenant.insert(conn.tenant_id.clone(), conn.clone());
    }
    Ok(by_tenant.into_values().collect())
}

fn rotate_connections(
    current: &[StoredConnection],
    tenant_id: &str,
    previous: &str,
    replacement: &str,
) -> Result<Vec<StoredConnection>> {
    anyhow::ensure!(
        !previous.is_empty() && !replacement.is_empty(),
        "refresh token must not be empty"
    );
    let target = current
        .iter()
        .find(|conn| conn.tenant_id == tenant_id)
        .ok_or_else(|| anyhow::anyhow!("tenant not connected"))?;
    anyhow::ensure!(target.refresh_token == previous, "stale refresh token");
    Ok(current
        .iter()
        .map(|conn| {
            if conn.refresh_token == previous {
                StoredConnection {
                    refresh_token: replacement.to_owned(),
                    ..conn.clone()
                }
            } else {
                conn.clone()
            }
        })
        .collect())
}

fn rotate_with_lease(
    current: &StoreDocument,
    owner: Uuid,
    tenant_id: &str,
    previous: &str,
    replacement: &str,
) -> Result<StoreDocument> {
    let lease = current
        .refresh_lease
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("refresh lease is absent"))?;
    anyhow::ensure!(
        lease.owner == owner && lease.expires_at > Utc::now(),
        "refresh lease expired or changed owner"
    );
    Ok(StoreDocument {
        connections: rotate_connections(&current.connections, tenant_id, previous, replacement)?,
        ..current.clone()
    })
}

impl GcsConnectionStore {
    pub async fn new(bucket: String, object: String, kms_key_name: Option<String>) -> Result<Self> {
        anyhow::ensure!(!bucket.is_empty(), "token-store bucket must be set");
        anyhow::ensure!(
            !object.is_empty() && !object.starts_with('/') && !object.contains(".."),
            "invalid token-store object name"
        );
        let config = ClientConfig::default()
            .with_auth()
            .await
            .context("authenticate GCS connection store")?;
        Ok(Self {
            client: Client::new(config),
            bucket,
            object,
            kms_key_name,
        })
    }

    async fn read_object(&self, object: &str) -> Result<Option<(i64, Vec<u8>)>> {
        for _ in 0..3 {
            let req = GetObjectRequest {
                bucket: self.bucket.clone(),
                object: object.to_owned(),
                ..Default::default()
            };
            let meta = match self.client.get_object(&req).await {
                Ok(meta) => meta,
                Err(error) if is_status(&error, 404) => return Ok(None),
                Err(error) => return Err(error).context("read token-store object metadata"),
            };
            let pinned = GetObjectRequest {
                generation: Some(meta.generation),
                ..req
            };
            match self
                .client
                .download_object(&pinned, &Range::default())
                .await
            {
                Ok(bytes) => return Ok(Some((meta.generation, bytes))),
                Err(error) if is_status(&error, 404) => continue,
                Err(error) => return Err(error).context("download token-store object"),
            }
        }
        anyhow::bail!("token-store object changed during every read attempt")
    }

    async fn write_object(
        &self,
        object: &str,
        previous_generation: i64,
        body: Vec<u8>,
    ) -> std::result::Result<i64, GcsError> {
        let mut media = Media::new(object.to_owned());
        media.content_type = "application/json".into();
        let req = UploadObjectRequest {
            bucket: self.bucket.clone(),
            if_generation_match: Some(previous_generation),
            kms_key_name: self.kms_key_name.clone(),
            ..Default::default()
        };
        self.client
            .upload_object(&req, body, &UploadType::Simple(media))
            .await
            .map(|meta| meta.generation)
    }

    async fn read_document(&self) -> Result<(i64, StoreDocument)> {
        let Some((generation, bytes)) = self.read_object(&self.object).await? else {
            return Ok((0, StoreDocument::default()));
        };
        let document: StoreDocument = if bytes.first() == Some(&b'[') {
            StoreDocument {
                connections: serde_json::from_slice(&bytes)
                    .context("parse legacy token-store connections")?,
                refresh_lease: None,
            }
        } else {
            serde_json::from_slice(&bytes).context("parse token-store document")?
        };
        let unique: std::collections::HashSet<&str> = document
            .connections
            .iter()
            .map(|conn| conn.tenant_id.as_str())
            .collect();
        anyhow::ensure!(
            unique.len() == document.connections.len(),
            "duplicate tenant in token store"
        );
        Ok((generation, document))
    }

    async fn update<F>(&self, apply: F) -> Result<()>
    where
        F: Fn(&StoreDocument) -> Result<StoreDocument> + Send + Sync,
    {
        for attempt in 0..CAS_ATTEMPTS {
            let (generation, current) = self.read_document().await?;
            let next = apply(&current)?;
            let body = serde_json::to_vec(&next).context("serialize token-store document")?;
            match self.write_object(&self.object, generation, body).await {
                Ok(_) => return Ok(()),
                Err(error) if is_status(&error, 412) => {
                    sleep(Duration::from_millis(20 * (attempt as u64 + 1))).await;
                }
                Err(error) => return Err(error).context("write token-store connections"),
            }
        }
        anyhow::bail!("token-store compare-and-swap conflict limit exceeded")
    }
}

#[async_trait]
impl ConnectionStore for GcsConnectionStore {
    async fn upsert_many(&self, connections: Vec<StoredConnection>) -> Result<()> {
        let lock = self.acquire_refresh_lock().await?;
        let result = self
            .update(|current| {
                let RefreshLockKind::Gcs(owner) = lock.0 else {
                    anyhow::bail!("lock backend mismatch");
                };
                anyhow::ensure!(
                    current
                        .refresh_lease
                        .as_ref()
                        .is_some_and(|lease| lease.owner == owner && lease.expires_at > Utc::now()),
                    "onboarding lease expired or changed owner"
                );
                Ok(StoreDocument {
                    connections: replace_connections(&current.connections, &connections)?,
                    ..current.clone()
                })
            })
            .await;
        self.release_refresh_lock(lock).await?;
        result
    }

    async fn list(&self) -> Result<Vec<StoredConnection>> {
        let (_, document) = self.read_document().await?;
        Ok(document.connections)
    }

    async fn get_refresh_token(&self, tenant_id: &str) -> Result<Option<String>> {
        let (_, document) = self.read_document().await?;
        Ok(document
            .connections
            .iter()
            .find(|conn| conn.tenant_id == tenant_id)
            .map(|conn| conn.refresh_token.clone()))
    }

    async fn acquire_refresh_lock(&self) -> Result<RefreshLock> {
        let deadline = Instant::now() + LEASE_WAIT;
        loop {
            let (generation, current) = self.read_document().await?;
            if current
                .refresh_lease
                .as_ref()
                .is_none_or(|lease| lease.expires_at <= Utc::now())
            {
                let owner = Uuid::new_v4();
                let next = StoreDocument {
                    refresh_lease: Some(LeaseRecord {
                        owner,
                        expires_at: Utc::now() + chrono::Duration::seconds(LEASE_TTL_SECONDS),
                    }),
                    ..current
                };
                match self
                    .write_object(&self.object, generation, serde_json::to_vec(&next)?)
                    .await
                {
                    Ok(_) => return Ok(RefreshLock(RefreshLockKind::Gcs(owner))),
                    Err(error) if is_status(&error, 412) => {}
                    Err(error) => return Err(error).context("acquire token-store refresh lease"),
                }
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "timed out waiting for refresh lease"
            );
            sleep(LEASE_POLL).await;
        }
    }

    async fn release_refresh_lock(&self, lock: RefreshLock) -> Result<()> {
        let RefreshLockKind::Gcs(owner) = lock.0 else {
            anyhow::bail!("lock backend mismatch");
        };
        self.update(|current| {
            anyhow::ensure!(
                current
                    .refresh_lease
                    .as_ref()
                    .is_some_and(|lease| lease.owner == owner),
                "refresh lease changed owner"
            );
            Ok(StoreDocument {
                refresh_lease: None,
                ..current.clone()
            })
        })
        .await
    }

    async fn update_refresh_token(
        &self,
        lock: &RefreshLock,
        tenant_id: &str,
        previous_refresh_token: &str,
        refresh_token: &str,
    ) -> Result<()> {
        let RefreshLockKind::Gcs(owner) = &lock.0 else {
            anyhow::bail!("lock backend mismatch");
        };
        self.update(|current| {
            rotate_with_lease(
                current,
                *owner,
                tenant_id,
                previous_refresh_token,
                refresh_token,
            )
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{to_bytes, Body},
        extract::State,
        http::{Request, StatusCode},
        response::{IntoResponse, Response},
        routing::any,
        Json, Router,
    };
    use chrono::Utc;
    use tokio::sync::Mutex;

    use super::*;

    fn rec(tenant: &str, token: &str) -> StoredConnection {
        StoredConnection {
            tenant_id: tenant.into(),
            tenant_name: tenant.into(),
            tenant_type: "ORGANISATION".into(),
            connected_at: Utc::now(),
            scopes: vec!["offline_access".into()],
            refresh_token: token.into(),
        }
    }

    #[test]
    fn onboarding_publishes_entire_grant_without_losing_unrelated_tenants() {
        let next = replace_connections(
            &[rec("old", "other")],
            &[rec("a", "shared"), rec("b", "shared")],
        )
        .unwrap();
        assert_eq!(next.len(), 3);
        assert_eq!(next[0].tenant_id, "a");
        assert_eq!(next[2].tenant_id, "old");
        assert_eq!(next[0].refresh_token, next[1].refresh_token);
    }

    #[test]
    fn rotate_replaces_every_tenant_on_grant_and_rejects_stale_exchange() {
        let current = vec![rec("a", "shared"), rec("b", "shared"), rec("c", "other")];
        let next = rotate_connections(&current, "a", "shared", "new").unwrap();
        assert_eq!(next[0].refresh_token, "new");
        assert_eq!(next[1].refresh_token, "new");
        assert_eq!(next[2].refresh_token, "other");
        assert!(rotate_connections(&next, "a", "shared", "stale").is_err());
        assert!(rotate_connections(&next, "missing", "new", "later").is_err());
    }

    #[test]
    fn expired_or_stolen_lease_cannot_commit_rotated_token() {
        let owner = Uuid::new_v4();
        let doc = StoreDocument {
            connections: vec![rec("a", "old")],
            refresh_lease: Some(LeaseRecord {
                owner,
                expires_at: Utc::now() + chrono::Duration::minutes(1),
            }),
        };
        assert!(rotate_with_lease(&doc, Uuid::new_v4(), "a", "old", "new").is_err());
        let rotated = rotate_with_lease(&doc, owner, "a", "old", "new").unwrap();
        assert_eq!(rotated.connections[0].refresh_token, "new");
        let expired = StoreDocument {
            refresh_lease: Some(LeaseRecord {
                owner,
                expires_at: Utc::now() - chrono::Duration::seconds(1),
            }),
            ..doc
        };
        assert!(rotate_with_lease(&expired, owner, "a", "old", "new").is_err());
    }

    #[derive(Default)]
    struct MockObject {
        generation: i64,
        body: Vec<u8>,
    }

    fn mock_error(status: StatusCode) -> Response {
        (
            status,
            Json(serde_json::json!({"error": {
                "code": status.as_u16(), "message": "precondition or object missing", "errors": []
            }})),
        )
            .into_response()
    }

    async fn mock_gcs(
        State(state): State<Arc<Mutex<MockObject>>>,
        request: Request<Body>,
    ) -> Response {
        let path = request.uri().path().to_owned();
        let query = request.uri().query().unwrap_or_default().to_owned();
        let generation_arg = |name: &str| -> Option<i64> {
            query
                .split('&')
                .find_map(|part| part.strip_prefix(&format!("{name}=")))
                .and_then(|value| value.parse().ok())
        };
        let mut current = state.lock().await;
        if path.starts_with("/upload/storage/v1/") {
            if generation_arg("ifGenerationMatch") != Some(current.generation) {
                return mock_error(StatusCode::PRECONDITION_FAILED);
            }
            let Ok(body) = to_bytes(request.into_body(), 1024 * 1024).await else {
                return mock_error(StatusCode::BAD_REQUEST);
            };
            current.generation += 1;
            current.body = body.to_vec();
        } else if path.starts_with("/storage/v1/") {
            if current.generation == 0
                || generation_arg("generation")
                    .is_some_and(|generation| generation != current.generation)
            {
                return mock_error(StatusCode::NOT_FOUND);
            }
            if query.split('&').any(|part| part == "alt=media") {
                return (StatusCode::OK, current.body.clone()).into_response();
            }
        } else {
            return mock_error(StatusCode::NOT_FOUND);
        }
        Json(serde_json::json!({
            "selfLink": "", "mediaLink": "", "metageneration": "1",
            "name": "connections.json", "id": "test/connections.json",
            "bucket": "test", "generation": current.generation.to_string()
        }))
        .into_response()
    }

    #[tokio::test]
    async fn two_store_instances_share_a_fenced_refresh_lease() {
        let state = Arc::new(Mutex::new(MockObject::default()));
        let app = Router::new()
            .fallback(any(mock_gcs))
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let create = || GcsConnectionStore {
            client: Client::new(ClientConfig {
                storage_endpoint: endpoint.clone(),
                ..ClientConfig::default().anonymous()
            }),
            bucket: "test".into(),
            object: "connections.json".into(),
            kms_key_name: None,
        };
        let first = Arc::new(create());
        let second = Arc::new(create());
        first
            .upsert_many(vec![rec("a", "shared"), rec("b", "shared")])
            .await
            .unwrap();
        let lease = first.acquire_refresh_lock().await.unwrap();
        let contender = second.acquire_refresh_lock();
        assert!(tokio::time::timeout(Duration::from_millis(100), contender)
            .await
            .is_err());
        let onboarding_store = Arc::clone(&second);
        let onboarding = tokio::spawn(async move {
            onboarding_store
                .upsert_many(vec![rec("a", "new-grant")])
                .await
        });
        tokio::task::yield_now().await;
        assert!(
            !onboarding.is_finished(),
            "onboarding must wait for refresh"
        );
        first
            .update_refresh_token(&lease, "a", "shared", "rotated")
            .await
            .unwrap();
        first.release_refresh_lock(lease).await.unwrap();
        onboarding.await.unwrap().unwrap();
        assert_eq!(
            second.get_refresh_token("a").await.unwrap().as_deref(),
            Some("new-grant")
        );
        assert_eq!(
            second.get_refresh_token("b").await.unwrap().as_deref(),
            Some("rotated")
        );
        let next_lease = second.acquire_refresh_lock().await.unwrap();
        assert!(second
            .update_refresh_token(&next_lease, "b", "shared", "stale")
            .await
            .is_err());
        second.release_refresh_lock(next_lease).await.unwrap();
        server.abort();
    }
}
