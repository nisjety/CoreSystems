//! Verified Space scope for the first Data Plane retrieval vertical.
//!
//! A [`VerifiedSpaceAuthority`] can only be constructed by a future Control
//! decision verifier at the service boundary. It is deliberately not
//! deserializable from HTTP/gRPC input. The resolver then maps the canonical
//! Space reference to one Data-owned resource target. Existing document
//! ownership/grant visibility remains an additional post-filter; a Space never
//! widens a document ACL.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::BTreeMap;

use crate::pipeline::types::RetrievalFiltersInput;

const DECISION_VERSION: &str = "v2";
const RETRIEVAL_AUDIENCE: &str = "data-plane-retrieval";
const MAX_DECISION_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
struct RetrievalDecisionClaims {
    decision_ref: String,
    org_id: String,
    space_ref: String,
    subject_id: String,
    service_audience: String,
    recipient_audience_ref: String,
    privacy_policy_ref: String,
    resource_authorization_ref: String,
    authority_revision: i64,
    permissions: Vec<String>,
    purpose: String,
    lawful_basis: String,
    privacy_class: String,
    retention_class: String,
    residency: String,
    deletion_scope: String,
    nonce: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

/// Verify a Control-issued retrieval decision at the Data boundary. Callers
/// cannot construct [`VerifiedSpaceAuthority`] from transport data; missing,
/// expired, mismatched, or policy-incomplete decisions are rejected before a
/// raw collection/workspace filter reaches the retrieval pipeline.
pub fn verify_retrieval_space_decision(
    token: &str,
    keys: &BTreeMap<String, VerifyingKey>,
    org_id: &str,
    subject_id: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<VerifiedSpaceAuthority> {
    if token.len() > MAX_DECISION_BYTES {
        anyhow::bail!("Space decision is too large");
    }
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 4 || parts[0] != DECISION_VERSION {
        anyhow::bail!("invalid Space decision envelope");
    }
    let key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|value| String::from_utf8(value).ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("untrusted Space decision key"))?;
    let key = keys
        .get(&key_id)
        .ok_or_else(|| anyhow::anyhow!("untrusted Space decision key"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| anyhow::anyhow!("invalid Space decision payload"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(parts[3])
        .map_err(|_| anyhow::anyhow!("invalid Space decision signature"))?;
    let signature = Signature::from_slice(&signature)
        .map_err(|_| anyhow::anyhow!("invalid Space decision signature"))?;
    key.verify(
        format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
        &signature,
    )
    .map_err(|_| anyhow::anyhow!("invalid Space decision signature"))?;
    let claims: RetrievalDecisionClaims = serde_json::from_slice(&payload)
        .map_err(|_| anyhow::anyhow!("invalid Space decision claims"))?;
    let privacy_complete = [
        &claims.purpose,
        &claims.lawful_basis,
        &claims.privacy_class,
        &claims.retention_class,
        &claims.residency,
        &claims.deletion_scope,
        &claims.nonce,
    ]
    .iter()
    .all(|value| !value.trim().is_empty());
    if claims.org_id != org_id
        || claims.subject_id != subject_id
        || claims.service_audience != RETRIEVAL_AUDIENCE
        || !claims
            .permissions
            .iter()
            .any(|permission| permission == "retrieval:read")
        || claims.authority_revision <= 0
        || claims.issued_at > now + chrono::Duration::minutes(1)
        || claims.expires_at <= now
        || !privacy_complete
    {
        anyhow::bail!("Space decision does not authorize this retrieval");
    }
    let authority = VerifiedSpaceAuthority {
        authority_revision: claims.authority_revision,
        decision_ref: claims.decision_ref,
        org_id: claims.org_id,
        privacy_policy_ref: claims.privacy_policy_ref,
        recipient_audience_ref: claims.recipient_audience_ref,
        resource_authorization_ref: claims.resource_authorization_ref,
        space_ref: claims.space_ref,
    };
    authority.validate()?;
    Ok(authority)
}

/// Deployment-owned Control key material. The key set is intentionally shared
/// in shape with Model Plane so rotation can overlap current/previous keys;
/// Data still enforces its own audience and permission after verification.
pub fn configured_retrieval_decision_keys() -> anyhow::Result<BTreeMap<String, VerifyingKey>> {
    let raw = std::env::var("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON").ok();
    let encoded = match raw {
        Some(raw) => serde_json::from_str::<BTreeMap<String, String>>(&raw)?,
        None => BTreeMap::from([(
            std::env::var("CONTROL_SPACE_DECISION_KEY_ID")?,
            std::env::var("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64")?,
        )]),
    };
    if encoded.is_empty() {
        anyhow::bail!("Control Space decision public key set is empty");
    }
    encoded
        .into_iter()
        .map(|(id, encoded)| {
            if id.trim().is_empty()
                || encoded.trim().is_empty()
                || encoded.chars().any(char::is_whitespace)
            {
                anyhow::bail!("invalid Control Space decision key configuration");
            }
            let raw = URL_SAFE_NO_PAD.decode(encoded.trim())?;
            let raw: [u8; 32] = raw
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid Control Space decision public key length"))?;
            Ok((id, VerifyingKey::from_bytes(&raw)?))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedSpaceAuthority {
    pub authority_revision: i64,
    pub decision_ref: String,
    pub org_id: String,
    pub privacy_policy_ref: String,
    pub recipient_audience_ref: String,
    pub resource_authorization_ref: String,
    pub space_ref: String,
}

impl VerifiedSpaceAuthority {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.authority_revision <= 0 {
            anyhow::bail!("Space authority revision must be positive");
        }
        for (name, value) in [
            ("decision_ref", &self.decision_ref),
            ("org_id", &self.org_id),
            ("privacy_policy_ref", &self.privacy_policy_ref),
            ("recipient_audience_ref", &self.recipient_audience_ref),
            (
                "resource_authorization_ref",
                &self.resource_authorization_ref,
            ),
            ("space_ref", &self.space_ref),
        ] {
            if value.trim().is_empty() {
                anyhow::bail!("Space authority {name} is required");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSpaceRetrievalScope {
    pub authority: VerifiedSpaceAuthority,
    pub collection_id: Option<String>,
    pub owner_resource_ref: String,
    pub workspace_id: Option<String>,
}

impl ResolvedSpaceRetrievalScope {
    pub fn validate(&self) -> anyhow::Result<()> {
        self.authority.validate()?;
        if self.owner_resource_ref.trim().is_empty() {
            anyhow::bail!("Space retrieval owner resource reference is required");
        }
        if self
            .workspace_id
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
            && self
                .collection_id
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            anyhow::bail!("Space retrieval mapping requires a workspace or collection");
        }
        Ok(())
    }

    /// Intersect, never replace, caller filters with the authority-resolved
    /// target. This makes a forged or stale filter fail rather than broadening
    /// to a similarly named workspace.
    pub fn apply_to_filters(&self, filters: &mut RetrievalFiltersInput) -> anyhow::Result<()> {
        self.validate()?;
        intersect_exact_filter(
            &mut filters.workspaces,
            self.workspace_id.as_deref(),
            "workspace",
        )?;
        intersect_exact_filter(
            &mut filters.collections,
            self.collection_id.as_deref(),
            "collection",
        )?;
        Ok(())
    }
}

fn intersect_exact_filter(
    supplied: &mut Vec<String>,
    required: Option<&str>,
    name: &str,
) -> anyhow::Result<()> {
    let Some(required) = required.map(str::trim).filter(|value| !value.is_empty()) else {
        if supplied.is_empty() {
            return Ok(());
        }
        anyhow::bail!("Space retrieval binding has no {name} target");
    };
    if supplied.is_empty() {
        supplied.push(required.to_owned());
        return Ok(());
    }
    if supplied.len() != 1 || supplied[0].trim() != required {
        anyhow::bail!("requested {name} is outside the verified Space retrieval binding");
    }
    Ok(())
}

/// Resolve the only active Data binding for a verified Control decision.
///
/// The query is tenant-scoped and requires the current resource authorization
/// reference to match. A revoked/missing/ambiguous binding fails closed before
/// retrieval; the caller must not fall back to raw workspace filters.
#[allow(dead_code)] // wired when the Control decision verifier lands at the boundary
pub async fn resolve_space_retrieval_scope(
    pool: &PgPool,
    authority: VerifiedSpaceAuthority,
) -> anyhow::Result<ResolvedSpaceRetrievalScope> {
    authority.validate()?;
    let mut tx = pg_org_scope::begin_org_scoped(pool, &authority.org_id).await?;
    let rows = sqlx::query_as::<_, SpaceBindingRow>(
        r#"
        SELECT workspace_id, collection_id, owner_resource_ref
        FROM space_retrieval_bindings
        WHERE org_id = $1
          AND space_ref = $2
          AND resource_authorization_ref = $3
          AND binding_state = 'active'
        "#,
    )
    .bind(&authority.org_id)
    .bind(&authority.space_ref)
    .bind(&authority.resource_authorization_ref)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    if rows.len() != 1 {
        anyhow::bail!("verified Space has no unique active Data retrieval binding");
    }
    let row = rows.into_iter().next().expect("checked exactly one row");
    let scope = ResolvedSpaceRetrievalScope {
        authority,
        collection_id: row.collection_id,
        owner_resource_ref: row.owner_resource_ref,
        workspace_id: row.workspace_id,
    };
    scope.validate()?;
    Ok(scope)
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)] // constructed by the deferred boundary resolver above
struct SpaceBindingRow {
    workspace_id: Option<String>,
    collection_id: Option<String>,
    owner_resource_ref: String,
}

#[cfg(test)]
mod tests {
    use super::{
        verify_retrieval_space_decision, ResolvedSpaceRetrievalScope, VerifiedSpaceAuthority,
        DECISION_VERSION, RETRIEVAL_AUDIENCE,
    };
    use crate::pipeline::types::RetrievalFiltersInput;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use chrono::Utc;
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::collections::BTreeMap;

    fn signed_retrieval_decision(key: &SigningKey, key_id: &str) -> String {
        let now = Utc::now();
        let payload = serde_json::json!({
            "decision_ref":"decision-1", "org_id":"org-1", "space_ref":"space-1", "subject_id":"user-1",
            "service_audience": RETRIEVAL_AUDIENCE, "recipient_audience_ref":"audience-1", "privacy_policy_ref":"privacy-1",
            "resource_authorization_ref":"resource-1", "authority_revision": 1, "permissions":["retrieval:read"],
            "purpose":"assistant_collaboration", "lawful_basis":"contract", "privacy_class":"internal",
            "retention_class":"standard", "residency":"swedencentral", "deletion_scope":"space", "nonce":"nonce-1",
            "issued_at": now.to_rfc3339(), "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339()
        });
        let key_part = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let payload_part = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let input = format!("{DECISION_VERSION}.{key_part}.{payload_part}");
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(key.sign(input.as_bytes()).to_bytes())
        )
    }

    fn scope() -> ResolvedSpaceRetrievalScope {
        ResolvedSpaceRetrievalScope {
            authority: VerifiedSpaceAuthority {
                authority_revision: 4,
                decision_ref: "decision_1".into(),
                org_id: "org_1".into(),
                privacy_policy_ref: "privacy_3".into(),
                recipient_audience_ref: "audience_2".into(),
                resource_authorization_ref: "resource_auth_8".into(),
                space_ref: "space_1".into(),
            },
            collection_id: Some("collection_1".into()),
            owner_resource_ref: "document_1".into(),
            workspace_id: Some("workspace_1".into()),
        }
    }

    #[test]
    fn scope_injects_its_exact_data_targets() {
        let mut filters = RetrievalFiltersInput::default();
        scope()
            .apply_to_filters(&mut filters)
            .expect("scope is valid");
        assert_eq!(filters.workspaces, ["workspace_1"]);
        assert_eq!(filters.collections, ["collection_1"]);
    }

    #[test]
    fn scope_never_widens_a_forged_workspace_or_collection_filter() {
        let mut filters = RetrievalFiltersInput {
            workspaces: vec!["workspace_other".into()],
            collections: vec!["collection_1".into()],
            ..Default::default()
        };
        let error = scope()
            .apply_to_filters(&mut filters)
            .expect_err("different workspace must fail");
        assert!(error.to_string().contains("outside the verified Space"));
    }

    #[test]
    fn scope_rejects_missing_privacy_or_resource_authority() {
        let mut invalid = scope();
        invalid.authority.privacy_policy_ref.clear();
        assert!(invalid.validate().is_err());
        invalid = scope();
        invalid.authority.resource_authorization_ref.clear();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn retrieval_decision_is_a_signed_target_bound_authority_not_a_client_filter() {
        let signing = SigningKey::from_bytes(&[21; 32]);
        let token = signed_retrieval_decision(&signing, "control-current");
        let keys = BTreeMap::from([("control-current".to_owned(), signing.verifying_key())]);
        let authority =
            verify_retrieval_space_decision(&token, &keys, "org-1", "user-1", Utc::now())
                .expect("matching signed retrieval decision");
        assert_eq!(authority.space_ref, "space-1");
        assert!(
            verify_retrieval_space_decision(&token, &keys, "org-1", "other-user", Utc::now())
                .is_err()
        );
    }
}
