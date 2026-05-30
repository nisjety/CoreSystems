//! S3-backed `ProfileStore` so session snapshots survive restarts.
//!
//! Tenant-scoped layout (P0 / cluster #auth+tenancy):
//! - `profiles/{org_id}/{profile_id}.json` — snapshot blob.
//! - `.profile-index/{org_id}/{profile_id}` — flat marker for `list()`.
//!
//! Every method takes an `org_id` argument that the edge handler sets from
//! the verified JWT claim. List prefixes are bucketed by org so one
//! tenant's enumeration can never see another's IDs even if the S3 IAM
//! policy is overly broad.

use async_trait::async_trait;

use quarry_browser::session::{ProfileStore, SessionSnapshot};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::ProfileKind;
use quarry_core::QuarryResult;

const PROFILES_PREFIX: &str = "profiles";
const INDEX_PREFIX: &str = ".profile-index";

pub struct S3ProfileStore {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl S3ProfileStore {
    pub async fn new(bucket: impl Into<String>) -> QuarryResult<Self> {
        let cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_s3::Client::new(&cfg);
        Ok(Self {
            client,
            bucket: bucket.into(),
        })
    }

    pub fn from_parts(client: aws_sdk_s3::Client, bucket: impl Into<String>) -> Self {
        Self {
            client,
            bucket: bucket.into(),
        }
    }

    fn snapshot_key(org_id: &str, id: &ProfileKind) -> String {
        format!("{PROFILES_PREFIX}/{org_id}/{id}.json")
    }

    fn index_key(org_id: &str, id: &ProfileKind) -> String {
        format!("{INDEX_PREFIX}/{org_id}/{id}")
    }

    fn index_prefix(org_id: &str) -> String {
        format!("{INDEX_PREFIX}/{org_id}/")
    }
}

#[async_trait]
impl ProfileStore for S3ProfileStore {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
    ) -> QuarryResult<()> {
        let body = serde_json::to_vec(snapshot).map_err(|e| {
            QuarryError::new(ErrorCode::Internal, format!("snapshot encode failed: {e}"))
        })?;
        let snap_key = Self::snapshot_key(org_id, profile_id);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&snap_key)
            .body(aws_sdk_s3::primitives::ByteStream::from(body))
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("s3 put profile: {e}")))?;

        let idx_key = Self::index_key(org_id, profile_id);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&idx_key)
            .body(aws_sdk_s3::primitives::ByteStream::from(Vec::new()))
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::Internal, format!("s3 put profile index: {e}"))
            })?;

        Ok(())
    }

    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>> {
        let key = Self::snapshot_key(org_id, profile_id);
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await;

        match resp {
            Ok(out) => {
                let body = out.body.collect().await.map_err(|e| {
                    QuarryError::new(ErrorCode::Internal, format!("s3 read body: {e}"))
                })?;
                let bytes = body.into_bytes();
                let snapshot: SessionSnapshot = serde_json::from_slice(&bytes).map_err(|e| {
                    QuarryError::new(ErrorCode::Internal, format!("snapshot decode: {e}"))
                })?;
                Ok(Some(snapshot))
            }
            Err(err) => {
                let s = err.to_string();
                if s.contains("NoSuchKey") || s.contains("NotFound") || s.contains("404") {
                    Ok(None)
                } else {
                    Err(QuarryError::new(
                        ErrorCode::Internal,
                        format!("s3 get profile: {err}"),
                    ))
                }
            }
        }
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        let snap_key = Self::snapshot_key(org_id, profile_id);
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&snap_key)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::Internal, format!("s3 delete profile: {e}"))
            })?;
        let idx_key = Self::index_key(org_id, profile_id);
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&idx_key)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::Internal, format!("s3 delete profile idx: {e}"))
            })?;
        Ok(())
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileKind>> {
        let mut continuation: Option<String> = None;
        let mut out = Vec::new();
        let prefix = Self::index_prefix(org_id);
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix);
            if let Some(token) = continuation.as_deref() {
                req = req.continuation_token(token);
            }
            let resp = req.send().await.map_err(|e| {
                QuarryError::new(ErrorCode::Internal, format!("s3 list profiles: {e}"))
            })?;
            for obj in resp.contents() {
                if let Some(key) = obj.key() {
                    if let Some(id_str) = key.strip_prefix(&prefix) {
                        if let Ok(id) = id_str.parse::<ProfileKind>() {
                            out.push(id);
                        }
                    }
                }
            }
            if resp.is_truncated().unwrap_or(false) {
                continuation = resp.next_continuation_token().map(str::to_string);
                if continuation.is_none() {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_key_includes_org_prefix() {
        let id: ProfileKind = quarry_core::ids::Id::new();
        let key = S3ProfileStore::snapshot_key("org_alpha", &id);
        assert!(key.starts_with("profiles/org_alpha/"));
        assert!(key.ends_with(".json"));
    }

    #[test]
    fn index_key_includes_org_prefix() {
        let id: ProfileKind = quarry_core::ids::Id::new();
        let key = S3ProfileStore::index_key("org_alpha", &id);
        assert!(key.starts_with(".profile-index/org_alpha/"));
    }

    #[test]
    fn index_prefix_isolates_orgs() {
        // Two orgs MUST produce non-overlapping prefixes — otherwise
        // a list() call could leak entries.
        let a = S3ProfileStore::index_prefix("org_alpha");
        let b = S3ProfileStore::index_prefix("org_beta");
        assert_ne!(a, b);
        assert!(!a.starts_with(&b));
        assert!(!b.starts_with(&a));
    }
}
