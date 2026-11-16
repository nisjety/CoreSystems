//! Personal memory for one shared-room turn in Session Core (build plan WS17-S8; design WS17 D5).
//!
//! A current editor, manager or owner of a shared Space may use their own personal memory for one turn of
//! that room's answer. The gateway asks Control for a per-turn `model.memory.personal_context` decision, and
//! model-gateway calls `SearchMemory` a second time for that turn, carrying the decision in gRPC metadata
//! (`x-space-personal-context-decision` and `x-space-personal-context-ref`). This module verifies the
//! decision and answers with the owner's own rows only: `origin_kind = 'personal'`, `scope = 'user'`, the
//! caller's own user id. It never returns org rows, another user's rows or the room's rows (the ordinary
//! room recall answers those). A decision that does not verify answers with no rows, which is the
//! fail-closed answer.
//!
//! The caller, the org, the thread's ownership and the owner's memory controls are checked in `memory_grpc.rs`
//! before this module runs.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier as _};
use mp_contracts::model_plane::v1::{MemoryEntry, SearchMemoryResponse};
use sha2::{Digest as _, Sha256};
use sqlx::PgPool;
use tonic::{metadata::MetadataMap, Status};

use crate::dreaming::{MemoryProvenance, MemorySearchRow};
use crate::grpc::{
    configured_control_space_decision_keys, CONTROL_SPACE_DECISION_AUDIENCE,
    CONTROL_SPACE_DECISION_VERSION, MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES,
};

/// The metadata keys a model-gateway attaches to a personal-only `SearchMemory` call. Metadata rather than a
/// request field, so no other caller's request literal changes.
pub(crate) const PERSONAL_CONTEXT_TOKEN_METADATA: &str = "x-space-personal-context-decision";
pub(crate) const PERSONAL_CONTEXT_REF_METADATA: &str = "x-space-personal-context-ref";

/// The Control action, schema and sole permission of a per-turn personal-context decision.
pub(crate) const CONTROL_MEMORY_PERSONAL_CONTEXT_ACTION: &str = "model.memory.personal_context";
pub(crate) const CONTROL_MEMORY_PERSONAL_CONTEXT_SCHEMA: &str = "sha256:memory-personal-context-v1";
pub(crate) const MEMORY_PERSONAL_CONTEXT_PERMISSION: &str = "memory:personal_context";

/// The candidate pool the personal query ranks before the caller's `limit` is applied, the same flat bound the
/// other personal readers use (a small indexed read).
const PERSONAL_CANDIDATE_POOL: i64 = 200;

/// A personal-context grant as read from a call's metadata. It is unverified until
/// [`verify_personal_context_decision`] has accepted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersonalContextGrant {
    pub(crate) token: String,
    pub(crate) decision_ref: String,
}

/// Reads the grant a personal-only call carries. A call with only one of the two keys carries no grant; the
/// verifier would refuse a half grant anyway.
pub(crate) fn grant_from_metadata(metadata: &MetadataMap) -> Option<PersonalContextGrant> {
    let read = |key: &str| {
        metadata
            .get(key)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    Some(PersonalContextGrant {
        token: read(PERSONAL_CONTEXT_TOKEN_METADATA)?,
        decision_ref: read(PERSONAL_CONTEXT_REF_METADATA)?,
    })
}

/// The claims of a Control personal-context decision this module verifies. A subset of the Control envelope:
/// the signature covers the whole token, and every field below is either checked or bound into the digest.
#[derive(Debug, serde::Deserialize)]
struct PersonalContextClaims {
    decision_ref: String,
    org_id: String,
    space_ref: String,
    subject_id: String,
    service_audience: String,
    action_id: String,
    action_schema_hash: String,
    payload_digest: String,
    idempotency_key: String,
    recipient_audience_ref: String,
    recipient_audience_revision: u64,
    recipient_audience_hash: String,
    privacy_policy_ref: String,
    resource_authorization_ref: String,
    purpose: String,
    lawful_basis: String,
    privacy_class: String,
    retention_class: String,
    residency: String,
    deletion_scope: String,
    zero_data_retention: bool,
    nonce: String,
    authority_revision: u64,
    permissions: Vec<String>,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

/// Byte for byte the Go `memoryPersonalContextPayloadDigest`: the prefix, then each (name, value) as name, a NUL, a
/// u64 big-endian length and the value, then the authority and recipient-audience revisions. The thread id is the
/// one the request names, so a decision issued for another thread does not recompute to the signed digest.
fn personal_context_payload_digest(claims: &PersonalContextClaims, thread_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.memory.personal_context\0v1\0");
    for (name, value) in [
        ("org_id", claims.org_id.as_str()),
        ("user_id", claims.subject_id.as_str()),
        ("space_id", claims.space_ref.as_str()),
        ("space_decision_ref", claims.decision_ref.as_str()),
        ("recipient_audience_ref", claims.recipient_audience_ref.as_str()),
        ("recipient_audience_hash", claims.recipient_audience_hash.as_str()),
        ("privacy_policy_ref", claims.privacy_policy_ref.as_str()),
        ("resource_authorization_ref", claims.resource_authorization_ref.as_str()),
        ("thread_id", thread_id),
        ("action_schema_hash", CONTROL_MEMORY_PERSONAL_CONTEXT_SCHEMA),
        ("idempotency_key", claims.idempotency_key.as_str()),
    ] {
        digest.update(name.as_bytes());
        digest.update([0u8]);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.update(b"authority_revision\0");
    digest.update(claims.authority_revision.to_be_bytes());
    digest.update(b"recipient_audience_revision\0");
    digest.update(claims.recipient_audience_revision.to_be_bytes());
    format!("sha256:{:x}", digest.finalize())
}

/// Verifies one Control `model.memory.personal_context` decision for one turn of one thread. It is bound to the
/// caller (the thread's owner), the org, the thread's Space and the thread. The checks mirror
/// `verify_memory_declassify_decision_with_key`: version, key id, Ed25519 signature, service audience, action,
/// schema, the one permission, the digest, privacy completeness and the two-minute window (one minute of clock
/// skew allowed). A zero-retention claim is refused: such a decision is never issued.
pub(crate) fn verify_personal_context_decision(
    token: &str,
    decision_ref: &str,
    org_id: &str,
    user_id: &str,
    space_id: &str,
    thread_id: &str,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    const DENIED: &str = "Control personal context decision does not authorize this request";
    if token.trim().is_empty()
        || decision_ref.trim().is_empty()
        || space_id.trim().is_empty()
        || thread_id.trim().is_empty()
        || user_id.trim().is_empty()
    {
        return Err(Status::permission_denied(DENIED));
    }
    if token.len() > MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES {
        return Err(Status::invalid_argument(
            "Control personal context decision is too large",
        ));
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 4 || parts[0] != CONTROL_SPACE_DECISION_VERSION {
        return Err(Status::permission_denied(DENIED));
    }
    let keys = configured_control_space_decision_keys()?;
    let key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let key = crate::decision_keys::lookup_key(&keys, &key_id)?;
    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| Status::permission_denied(DENIED))?;
    let signature = URL_SAFE_NO_PAD
        .decode(parts[3])
        .ok()
        .and_then(|raw| Signature::from_slice(&raw).ok())
        .ok_or_else(|| Status::permission_denied(DENIED))?;
    key.verify(
        format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
        &signature,
    )
    .map_err(|_| Status::permission_denied(DENIED))?;
    let claims: PersonalContextClaims =
        serde_json::from_slice(&payload).map_err(|_| Status::permission_denied(DENIED))?;

    // Exactly the one permission. A declassify, retract, read, append or any other Control token carries
    // another action and fails here.
    let matches_request = claims.decision_ref == decision_ref
        && claims.org_id == org_id
        && claims.subject_id == user_id
        && claims.space_ref == space_id
        && claims.service_audience == CONTROL_SPACE_DECISION_AUDIENCE
        && claims.action_id == CONTROL_MEMORY_PERSONAL_CONTEXT_ACTION
        && claims.action_schema_hash == CONTROL_MEMORY_PERSONAL_CONTEXT_SCHEMA
        && claims.payload_digest == personal_context_payload_digest(&claims, thread_id)
        && claims.permissions.len() == 1
        && claims.permissions[0] == MEMORY_PERSONAL_CONTEXT_PERMISSION;
    if !matches_request {
        return Err(Status::permission_denied(DENIED));
    }
    let privacy_complete = [
        claims.purpose.as_str(),
        claims.lawful_basis.as_str(),
        claims.privacy_class.as_str(),
        claims.retention_class.as_str(),
        claims.residency.as_str(),
        claims.deletion_scope.as_str(),
    ]
    .iter()
    .all(|value| !value.trim().is_empty());
    if !privacy_complete
        || claims.zero_data_retention
        || claims.nonce.trim().is_empty()
        || claims.recipient_audience_ref.trim().is_empty()
        || claims.recipient_audience_hash.trim().is_empty()
        || claims.recipient_audience_revision == 0
        || claims.issued_at > now + chrono::Duration::minutes(1)
        || claims.expires_at <= now
    {
        return Err(Status::permission_denied(
            "Control personal context decision is expired or incomplete",
        ));
    }
    Ok(())
}

/// The owner's personal rows for one turn: `scope = 'user'`, no session, owned by the verified caller, the
/// personal origin, accepted and current, and the legacy room-origin exclusion (C7). Ranked like the other
/// personal readers: an exact match on the turn's query gets the head start, then confidence and recency.
async fn personal_rows(
    pool: &PgPool,
    org_id: &str,
    owner_user_id: &str,
    thread_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<MemorySearchRow>, Status> {
    let limit = usize::try_from(limit.clamp(1, 50)).unwrap_or(10);
    let legacy_exclusion = crate::space::memory_scope::legacy_room_origin_exclusion("$4", "$5");
    let sql = format!(
        "SELECT id, kind, key, content, confidence, updated_at, source_links \
         FROM agent_memory \
         WHERE org_id = $1 \
           AND scope = 'user' AND session_id IS NULL AND owner = $2 \
           AND origin_kind = 'personal' \
           AND review_state = 'accepted' AND is_latest = true \
           AND (expires_at IS NULL OR expires_at > now()) \
           AND {legacy_exclusion} \
         ORDER BY confidence DESC, updated_at DESC \
         LIMIT $3"
    );
    let rows: Vec<(
        String,
        String,
        String,
        String,
        f64,
        chrono::DateTime<Utc>,
        Vec<String>,
    )> = sqlx::query_as(&sql)
        .bind(org_id)
        .bind(owner_user_id)
        .bind(PERSONAL_CANDIDATE_POOL)
        .bind(crate::space::memory_scope::containment_on())
        .bind(thread_id)
        .fetch_all(pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let needle = query.trim().to_ascii_lowercase();
    let mut scored: Vec<MemorySearchRow> = rows
        .into_iter()
        .map(|(id, _kind, key, content, confidence, updated_at, source_links)| {
            let exact_match = !needle.is_empty()
                && (content.to_ascii_lowercase().contains(&needle)
                    || key.to_ascii_lowercase().contains(&needle));
            let exact_bonus = if needle.is_empty() || exact_match {
                0.15
            } else {
                0.0
            };
            MemorySearchRow {
                id,
                thread_id: thread_id.to_owned(),
                topic: "USER".to_owned(),
                provenance: MemoryProvenance::classify(&source_links),
                origin_space_ref: String::new(),
                withheld_reason: String::new(),
                content,
                score: crate::dreaming::memory_score(confidence, exact_bonus),
                updated_at,
                exact_match,
            }
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    scored.truncate(limit);
    Ok(scored)
}

fn empty_answer() -> SearchMemoryResponse {
    SearchMemoryResponse {
        entries: Vec::new(),
        degraded: false,
        degradation_reason: String::new(),
        skip_reason: String::new(),
        withheld: 0,
    }
}

/// The personal-only answer to a `SearchMemory` call that carries a personal-context grant (WS17-S8). The owner
/// is the verified caller (`search_memory` has run `authorize_thread` and the owner's memory gate). The thread
/// must be a shared-room thread, and the decision must verify for this owner, org, Space and thread. Any other
/// case answers with no rows and counts the refusal. The entries carry ids, topics and content only as the
/// ordinary recall does; nothing here logs content.
pub(crate) async fn personal_only_answer(
    pool: &PgPool,
    grant: &PersonalContextGrant,
    org_id: &str,
    owner_user_id: &str,
    thread_id: &str,
    query: &str,
    limit: u32,
) -> Result<SearchMemoryResponse, Status> {
    let thread: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT space_id, space_kind FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
    let Some((space_id, space_kind)) = thread else {
        return Ok(empty_answer());
    };
    let contained = crate::space::memory_scope::thread_is_contained(
        space_id.as_deref(),
        space_kind.as_deref(),
    );
    let space_id = match (contained, space_id) {
        (true, Some(space_id)) => space_id,
        _ => {
            metrics::counter!("mp_session_memory_personal_context_total", "op" => "not_shared_thread")
                .increment(1);
            return Ok(empty_answer());
        }
    };
    if verify_personal_context_decision(
        &grant.token,
        &grant.decision_ref,
        org_id,
        owner_user_id,
        &space_id,
        thread_id,
        Utc::now(),
    )
    .is_err()
    {
        metrics::counter!("mp_session_memory_personal_context_total", "op" => "refused").increment(1);
        return Ok(empty_answer());
    }
    let rows = personal_rows(pool, org_id, owner_user_id, thread_id, query, limit).await?;
    let entries: Vec<MemoryEntry> = rows
        .into_iter()
        .map(|row| crate::memory_grpc::pg_row_to_entry(row, owner_user_id))
        .collect();
    metrics::counter!("mp_session_memory_personal_context_total", "op" => "used").increment(1);
    tracing::info!(
        event = "MEMORY_PERSONAL_CONTEXT_USED",
        thread_id = %thread_id,
        count = entries.len(),
        "personal memory used for one shared-room turn"
    );
    Ok(SearchMemoryResponse {
        entries,
        degraded: false,
        degradation_reason: String::new(),
        skip_reason: String::new(),
        withheld: 0,
    })
}
