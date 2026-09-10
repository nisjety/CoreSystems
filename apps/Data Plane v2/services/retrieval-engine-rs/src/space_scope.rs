//! Verified Space scope for the first Data Plane retrieval vertical.
//!
//! A [`VerifiedSpaceAuthority`] can only be constructed by
//! [`verify_retrieval_space_decision`] below, which the HTTP `retrieve` handler
//! and the gRPC retrieval methods call at their service boundary. It is
//! deliberately not deserializable from HTTP/gRPC input. The resolver then
//! maps the canonical Space reference to one Data-owned resource target.
//! Existing document ownership/grant visibility remains an additional
//! post-filter; a Space never widens a document ACL.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::BTreeMap;

use crate::pipeline::types::RetrievalFiltersInput;

/// Upper bound on a Space's document set for the first vertical.
///
/// The set becomes a `document_id` any-of filter, so it must be bounded. It is
/// a REFUSAL bound, never a truncation: silently keeping the newest N would
/// narrow a room's retrieval to a subset the reader cannot see or predict,
/// which is the same silent-narrowing failure this module exists to prevent.
pub const MAX_SPACE_DOCUMENTS: i64 = 2_000;

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
    /// Documents imported into this Space under verified Control authority
    /// (`documents.space_ref`). This is the only Space predicate the document
    /// vertical can actually enforce: `workspace_id` / `collection_id` are
    /// payload keys that ONLY the wiki consumer writes, so a document-chunk
    /// search filtered on them matches nothing at all.
    ///
    /// Needed only where the predicate cannot be expressed in SQL — the vector
    /// search. A Postgres read filters on `space_ref` directly and is exact.
    pub document_ids: Vec<String>,
    /// The Space holds more documents than [`MAX_SPACE_DOCUMENTS`], so
    /// `document_ids` is a prefix rather than the set.
    ///
    /// Carried rather than raised at resolution time because it only matters to
    /// the vector search: a truncated id list there would narrow a room's
    /// retrieval to an unpredictable subset, while a Postgres read of the same
    /// Space is unaffected and must keep working.
    pub document_ids_truncated: bool,
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

    /// Constrain a document-vertical search to this Space's own documents.
    ///
    /// `apply_to_filters` above pins the workspace/collection the binding
    /// names, which is what the wiki arm needs. It does nothing for document
    /// chunks: their Qdrant payload carries `document_id`, `org_id`, `title`,
    /// `source`, `type` and `chunk_tokens` — no workspace and no collection.
    /// A Space-scoped dense search that relied on `apply_to_filters` alone was
    /// therefore filtering on a key no point has, and answered every query
    /// with zero candidates while returning 200.
    ///
    /// A Space with no documents refuses rather than clearing the filter: an
    /// empty `document_ids` list means "unconstrained", so falling through
    /// would search the whole org under Space authority.
    pub fn apply_document_scope_to_filters(
        &self,
        filters: &mut RetrievalFiltersInput,
    ) -> anyhow::Result<()> {
        self.validate()?;
        if self.document_ids_truncated {
            anyhow::bail!(
                "Space-scoped vector search needs an indexed Space predicate to span this many documents"
            );
        }
        if self.document_ids.is_empty() {
            anyhow::bail!("verified Space has no documents imported under Space authority");
        }
        if filters.document_ids.is_empty() {
            filters.document_ids = self.document_ids.clone();
            return Ok(());
        }
        let allowed: std::collections::HashSet<&str> =
            self.document_ids.iter().map(String::as_str).collect();
        let intersection: Vec<String> = filters
            .document_ids
            .iter()
            .map(|value| value.trim())
            .filter(|value| allowed.contains(value))
            .map(str::to_owned)
            .collect();
        if intersection.is_empty() {
            anyhow::bail!("requested documents are outside the verified Space");
        }
        filters.document_ids = intersection;
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
    let (document_ids, document_ids_truncated) =
        resolve_space_document_ids(pool, &authority.org_id, &authority.space_ref).await?;
    let scope = ResolvedSpaceRetrievalScope {
        authority,
        collection_id: row.collection_id,
        document_ids,
        document_ids_truncated,
        owner_resource_ref: row.owner_resource_ref,
        workspace_id: row.workspace_id,
    };
    scope.validate()?;
    Ok(scope)
}

/// The documents `documents-api` recorded against this Space, newest first.
///
/// Reads `documents.space_ref`, which is written only from a verified Control
/// Space import decision. It is deliberately not a name match, a metadata
/// lookup, or a workspace guess — the binding table's own contract is that
/// those are never a mapping.
///
/// Ownership/grant visibility is NOT applied here: this answers "what is in the
/// room", and the per-viewer post-filter that answers "what may this reader
/// see" still runs downstream. A Space never widens a document ACL.
/// Returns the ids and whether the Space holds more than the bound. Being over
/// the bound is not an error here: only the vector search cannot work with a
/// prefix, and it says so at the point where the prefix would be used.
pub async fn resolve_space_document_ids(
    pool: &PgPool,
    org_id: &str,
    space_ref: &str,
) -> anyhow::Result<(Vec<String>, bool)> {
    if org_id.trim().is_empty() || space_ref.trim().is_empty() {
        anyhow::bail!("Space document scope requires an org and a Space reference");
    }
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT document_id
        FROM documents
        WHERE org_id = $1
          AND space_ref = $2
          AND deleted_at IS NULL
        ORDER BY updated_at DESC
        LIMIT $3
        "#,
    )
    .bind(org_id)
    .bind(space_ref)
    .bind(MAX_SPACE_DOCUMENTS + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let truncated = rows.len() as i64 > MAX_SPACE_DOCUMENTS;
    let mut rows = rows;
    rows.truncate(MAX_SPACE_DOCUMENTS as usize);
    Ok((rows, truncated))
}

#[derive(sqlx::FromRow)]
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
            document_ids: vec!["document_1".into(), "document_2".into()],
            document_ids_truncated: false,
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

    // The defect this method was added for: the workspace/collection targets
    // are wiki-only payload keys, so a document search pinned to them returned
    // nothing while reporting success.
    #[test]
    fn document_scope_pins_the_spaces_own_documents() {
        let mut filters = RetrievalFiltersInput::default();
        scope()
            .apply_document_scope_to_filters(&mut filters)
            .expect("scope is valid");
        assert_eq!(filters.document_ids, ["document_1", "document_2"]);
    }

    #[test]
    fn document_scope_narrows_a_caller_list_and_refuses_one_outside_the_space() {
        let mut filters = RetrievalFiltersInput {
            document_ids: vec!["document_2".into(), "document_elsewhere".into()],
            ..Default::default()
        };
        scope()
            .apply_document_scope_to_filters(&mut filters)
            .expect("one requested document is in the Space");
        assert_eq!(filters.document_ids, ["document_2"]);

        let mut outside = RetrievalFiltersInput {
            document_ids: vec!["document_elsewhere".into()],
            ..Default::default()
        };
        let error = scope()
            .apply_document_scope_to_filters(&mut outside)
            .expect_err("a document outside the Space must fail");
        assert!(error.to_string().contains("outside the verified Space"));
    }

    // An empty document_ids list means "unconstrained" downstream, so an empty
    // Space must refuse rather than quietly search the whole org.
    #[test]
    fn an_empty_space_refuses_instead_of_clearing_the_filter() {
        let mut empty = scope();
        empty.document_ids.clear();
        let mut filters = RetrievalFiltersInput {
            document_ids: vec!["document_1".into()],
            ..Default::default()
        };
        let error = empty
            .apply_document_scope_to_filters(&mut filters)
            .expect_err("an empty Space must not clear the filter");
        assert!(error.to_string().contains("no documents"));
        assert_eq!(filters.document_ids, ["document_1"]);
    }

    // A prefix would narrow the room's retrieval to a subset no reader can see
    // or predict, so the vector search refuses. The same Space still lists and
    // still reads through Postgres, where the predicate is exact.
    #[test]
    fn a_truncated_document_set_refuses_the_vector_search() {
        let mut capped = scope();
        capped.document_ids_truncated = true;
        let mut filters = RetrievalFiltersInput::default();
        let error = capped
            .apply_document_scope_to_filters(&mut filters)
            .expect_err("a prefix must not become a filter");
        assert!(error.to_string().contains("indexed Space predicate"));
        assert!(filters.document_ids.is_empty());
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
