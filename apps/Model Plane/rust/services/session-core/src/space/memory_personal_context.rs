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
    // D18: the instant the member's history window starts at. Absent on a decision for a member without a window.
    // Not part of the payload digest; the memory rows it narrows are the room-derived ones (`memory_scope`).
    #[serde(default)]
    history_floor_at: Option<DateTime<Utc>>,
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
///
/// On success it returns the member's D18 timestamp floor (`history_floor_at`), `None` for a member without a window.
pub(crate) fn verify_personal_context_decision(
    token: &str,
    decision_ref: &str,
    org_id: &str,
    user_id: &str,
    space_id: &str,
    thread_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, Status> {
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
    Ok(claims.history_floor_at)
}

/// The owner's personal rows for one turn: `scope = 'user'`, no session, owned by the verified caller, the
/// personal origin, accepted and current, and the legacy room-origin exclusion (C7). Ranked like the other
/// personal readers: an exact match on the turn's query gets the head start, then confidence and recency.
/// D18: under the member's timestamp floor a room-derived row is admitted only as `history_floor_admits_sql`
/// says; the member's own personal memory, which is not room-derived, is never narrowed by a room window.
async fn personal_rows(
    pool: &PgPool,
    org_id: &str,
    owner_user_id: &str,
    thread_id: &str,
    query: &str,
    limit: u32,
    floor_at: Option<DateTime<Utc>>,
) -> Result<Vec<MemorySearchRow>, Status> {
    let limit = usize::try_from(limit.clamp(1, 50)).unwrap_or(10);
    let legacy_exclusion = crate::space::memory_scope::legacy_room_origin_exclusion("$4", "$5");
    let floor_admits = crate::space::memory_scope::history_floor_admits_sql("$6");
    let sql = format!(
        "SELECT id, kind, key, content, confidence, updated_at, source_links \
         FROM agent_memory \
         WHERE org_id = $1 \
           AND scope = 'user' AND session_id IS NULL AND owner = $2 \
           AND origin_kind = 'personal' \
           AND review_state = 'accepted' AND is_latest = true \
           AND (expires_at IS NULL OR expires_at > now()) \
           AND {legacy_exclusion} \
           AND {floor_admits} \
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
        .bind(floor_at)
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
    let Ok(floor_at) = verify_personal_context_decision(
        &grant.token,
        &grant.decision_ref,
        org_id,
        owner_user_id,
        &space_id,
        thread_id,
        Utc::now(),
    ) else {
        metrics::counter!("mp_session_memory_personal_context_total", "op" => "refused").increment(1);
        return Ok(empty_answer());
    };
    let rows = personal_rows(pool, org_id, owner_user_id, thread_id, query, limit, floor_at).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::space_w1_tests::{signing_key, StreamKeysEnv, KEY_ID};
    use chrono::TimeZone as _;
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde_json::{json, Value};
    use tonic::metadata::MetadataValue;

    const ORG: &str = "org-1";
    const USER: &str = "user-1";
    const SPACE: &str = "space-room";
    const THREAD: &str = "thread-1";
    const DECISION_REF: &str = "personal-ctx-1";
    /// The digest `personal_context_payload_digest` must give for `base_claims` and [`THREAD`], computed
    /// by an independent implementation of the byte layout in Control's `memoryPersonalContextPayloadDigest`
    /// (apps/Control Plane/user-core/internal/spaces/memory_personal_context_decision.go).
    const GOLDEN_DIGEST: &str = "sha256:b77c5c7add45335a9095972d9fa6692ecdc80994187948dc6847758a8f84947e";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).single().expect("test time")
    }

    /// The claims of a valid decision issued at [`now`]; the digest is bound by [`token_for`].
    fn base_claims() -> Value {
        json!({
            "decision_ref": DECISION_REF,
            "org_id": ORG,
            "space_ref": SPACE,
            "subject_id": USER,
            "service_audience": CONTROL_SPACE_DECISION_AUDIENCE,
            "action_id": CONTROL_MEMORY_PERSONAL_CONTEXT_ACTION,
            "action_schema_hash": CONTROL_MEMORY_PERSONAL_CONTEXT_SCHEMA,
            "payload_digest": "",
            "idempotency_key": "turn-1",
            "recipient_audience_ref": "audience:space-room:4",
            "recipient_audience_revision": 4,
            "recipient_audience_hash": "sha256:test-audience",
            "privacy_policy_ref": "privacy:org-1:5",
            "resource_authorization_ref": "control:space-room:memory-personal-context:7",
            "purpose": "assistant_collaboration",
            "lawful_basis": "contract",
            "privacy_class": "internal",
            "retention_class": "standard",
            "residency": "swedencentral",
            "deletion_scope": "space",
            "zero_data_retention": false,
            "nonce": "nonce-1",
            "authority_revision": 7,
            "permissions": [MEMORY_PERSONAL_CONTEXT_PERMISSION],
            "issued_at": now().to_rfc3339(),
            "expires_at": (now() + chrono::Duration::minutes(2)).to_rfc3339(),
        })
    }

    fn sign_with(key: &SigningKey, key_id: &str, claims: &Value) -> String {
        let key_part = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims"));
        let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{key_part}.{payload}");
        format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(key.sign(signing_input.as_bytes()).to_bytes())
        )
    }

    /// Signs `claims` under the test key after binding the digest of `thread` into them.
    fn token_for(mut claims: Value, thread: &str) -> String {
        let parsed: PersonalContextClaims = serde_json::from_value(claims.clone()).expect("claims fixture");
        claims["payload_digest"] = json!(personal_context_payload_digest(&parsed, thread));
        sign_with(&signing_key(), KEY_ID, &claims)
    }

    fn valid_token() -> String {
        token_for(base_claims(), THREAD)
    }

    fn verify(token: &str) -> Result<Option<DateTime<Utc>>, Status> {
        verify_personal_context_decision(token, DECISION_REF, ORG, USER, SPACE, THREAD, now())
    }

    fn refused<T: std::fmt::Debug>(result: Result<T, Status>, why: &str) {
        let status = result.expect_err(why);
        assert_eq!(status.code(), tonic::Code::PermissionDenied, "{why}: {status:?}");
    }

    // ---- the contract constants -------------------------------------------------------------

    #[test]
    fn the_wire_names_are_the_ones_control_and_the_gateway_use() {
        assert_eq!(PERSONAL_CONTEXT_TOKEN_METADATA, "x-space-personal-context-decision");
        assert_eq!(PERSONAL_CONTEXT_REF_METADATA, "x-space-personal-context-ref");
        assert_eq!(CONTROL_MEMORY_PERSONAL_CONTEXT_ACTION, "model.memory.personal_context");
        assert_eq!(CONTROL_MEMORY_PERSONAL_CONTEXT_SCHEMA, "sha256:memory-personal-context-v1");
        assert_eq!(MEMORY_PERSONAL_CONTEXT_PERMISSION, "memory:personal_context");
    }

    // ---- grant_from_metadata ----------------------------------------------------------------

    #[test]
    fn a_grant_needs_both_metadata_keys() {
        let mut metadata = MetadataMap::new();
        assert_eq!(grant_from_metadata(&metadata), None, "no keys, no grant");

        metadata.insert(PERSONAL_CONTEXT_TOKEN_METADATA, MetadataValue::from_static("tok"));
        assert_eq!(grant_from_metadata(&metadata), None, "a token without a ref is no grant");

        let mut only_ref = MetadataMap::new();
        only_ref.insert(PERSONAL_CONTEXT_REF_METADATA, MetadataValue::from_static("ref-1"));
        assert_eq!(grant_from_metadata(&only_ref), None, "a ref without a token is no grant");

        metadata.insert(PERSONAL_CONTEXT_REF_METADATA, MetadataValue::from_static("ref-1"));
        assert_eq!(
            grant_from_metadata(&metadata),
            Some(PersonalContextGrant {
                token: "tok".to_owned(),
                decision_ref: "ref-1".to_owned(),
            })
        );
    }

    #[test]
    fn a_blank_metadata_value_is_no_grant_and_values_are_trimmed() {
        let mut blank = MetadataMap::new();
        blank.insert(PERSONAL_CONTEXT_TOKEN_METADATA, MetadataValue::from_static("tok"));
        blank.insert(PERSONAL_CONTEXT_REF_METADATA, MetadataValue::from_static("   "));
        assert_eq!(grant_from_metadata(&blank), None, "a whitespace-only ref is no ref");

        let mut padded = MetadataMap::new();
        padded.insert(PERSONAL_CONTEXT_TOKEN_METADATA, MetadataValue::from_static("  tok  "));
        padded.insert(PERSONAL_CONTEXT_REF_METADATA, MetadataValue::from_static(" ref-1 "));
        assert_eq!(
            grant_from_metadata(&padded),
            Some(PersonalContextGrant {
                token: "tok".to_owned(),
                decision_ref: "ref-1".to_owned(),
            })
        );
    }

    // ---- the payload digest -----------------------------------------------------------------

    fn digest_of(claims: &Value, thread: &str) -> String {
        let parsed: PersonalContextClaims = serde_json::from_value(claims.clone()).expect("claims fixture");
        personal_context_payload_digest(&parsed, thread)
    }

    #[test]
    fn the_payload_digest_equals_the_independent_golden() {
        assert_eq!(digest_of(&base_claims(), THREAD), GOLDEN_DIGEST);
        // The thread is bound: the same decision for another thread is another digest.
        assert_eq!(
            digest_of(&base_claims(), "thread-2"),
            "sha256:c38d42f022b203d9163329064212aa1897717ba77ccefe72ea63009f2a8c9559"
        );
        // The recipient-audience revision is bound as a big-endian u64.
        let mut later = base_claims();
        later["recipient_audience_revision"] = json!(5);
        assert_eq!(
            digest_of(&later, THREAD),
            "sha256:ad8b69daf337b1baf63b2bec997efa69b80b831dcd601b38b18d190b3e0d6cd8"
        );
        // Each value is framed by its length in bytes, not characters.
        assert_eq!(
            digest_of(&base_claims(), "th\u{e9}ad-1"),
            "sha256:aed7f056e829642f3ecd96a6c11445eac7ca09de7d8c023e01632f592eaf5d4a"
        );
    }

    #[test]
    fn every_bound_field_changes_the_digest_and_unbound_fields_do_not() {
        let base = digest_of(&base_claims(), THREAD);
        for field in [
            "org_id",
            "subject_id",
            "space_ref",
            "decision_ref",
            "recipient_audience_ref",
            "recipient_audience_hash",
            "privacy_policy_ref",
            "resource_authorization_ref",
            "idempotency_key",
        ] {
            let mut changed = base_claims();
            changed[field] = json!("changed");
            assert_ne!(digest_of(&changed, THREAD), base, "{field} must be bound into the digest");
        }
        for field in ["authority_revision", "recipient_audience_revision"] {
            let mut changed = base_claims();
            changed[field] = json!(99);
            assert_ne!(digest_of(&changed, THREAD), base, "{field} must be bound into the digest");
        }
        for (field, value) in [
            ("nonce", json!("another-nonce")),
            ("purpose", json!("another-purpose")),
            ("permissions", json!(["memory:other"])),
            ("zero_data_retention", json!(true)),
            ("expires_at", json!("2030-01-01T00:00:00Z")),
        ] {
            let mut changed = base_claims();
            changed[field] = value;
            assert_eq!(digest_of(&changed, THREAD), base, "{field} is not part of the digest");
        }
    }

    // ---- verify_personal_context_decision ---------------------------------------------------

    #[test]
    fn a_decision_for_this_owner_org_space_and_thread_verifies() {
        let _keys = StreamKeysEnv::publish();
        verify(&valid_token()).expect("a valid decision verifies");
    }

    #[test]
    fn a_decision_carries_the_members_timestamp_floor_and_a_member_without_a_window_has_none() {
        let _keys = StreamKeysEnv::publish();
        // D18: the member's window instant rides on the decision; it narrows the owner's room-derived rows.
        assert_eq!(verify(&valid_token()).expect("no window verifies"), None, "no window, no floor");
        let floor = Utc.with_ymd_and_hms(2026, 10, 15, 9, 30, 0).single().expect("floor time");
        let windowed = token_for(
            {
                let mut claims = base_claims();
                claims["history_floor_at"] = json!(floor.to_rfc3339());
                claims
            },
            THREAD,
        );
        assert_eq!(verify(&windowed).expect("a windowed decision verifies"), Some(floor));
    }

    #[test]
    fn a_malformed_timestamp_floor_refuses_the_decision_rather_than_showing_full_history() {
        let _keys = StreamKeysEnv::publish();
        // The claim must parse or the whole decision is refused: a floor that cannot be read is never read as none.
        let mut claims = base_claims();
        claims["history_floor_at"] = json!("the first Tuesday");
        let digest = digest_of(&base_claims(), THREAD);
        claims["payload_digest"] = json!(digest);
        refused(verify(&sign_with(&signing_key(), KEY_ID, &claims)), "a floor that is not a timestamp");
    }

    #[test]
    fn a_decision_that_names_another_action_schema_audience_or_permission_is_refused() {
        let _keys = StreamKeysEnv::publish();
        // (case, edit of the claims before the digest is bound and the token signed)
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
            ("another action", Box::new(|c: &mut Value| c["action_id"] = json!("model.memory.declassify"))),
            ("a read action", Box::new(|c: &mut Value| c["action_id"] = json!("model.thread.read"))),
            ("another schema", Box::new(|c: &mut Value| c["action_schema_hash"] = json!("sha256:memory-declassify-v1"))),
            ("another service audience", Box::new(|c: &mut Value| c["service_audience"] = json!("model-plane-space-stream"))),
            ("a read permission", Box::new(|c: &mut Value| c["permissions"] = json!(["thread:read"]))),
            ("a declassify permission", Box::new(|c: &mut Value| c["permissions"] = json!(["memory:declassify"]))),
            (
                "the permission with another one beside it",
                Box::new(|c: &mut Value| c["permissions"] = json!([MEMORY_PERSONAL_CONTEXT_PERMISSION, "thread:read"])),
            ),
            ("no permission at all", Box::new(|c: &mut Value| c["permissions"] = json!([]))),
        ];
        for (why, edit) in cases {
            let mut claims = base_claims();
            edit(&mut claims);
            refused(verify(&token_for(claims, THREAD)), why);
        }
    }

    #[test]
    fn a_decision_signed_for_another_org_subject_space_or_ref_is_refused() {
        let _keys = StreamKeysEnv::publish();
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
            ("another org in the claims", Box::new(|c: &mut Value| c["org_id"] = json!("org-2"))),
            ("another subject in the claims", Box::new(|c: &mut Value| c["subject_id"] = json!("user-2"))),
            ("another Space in the claims", Box::new(|c: &mut Value| c["space_ref"] = json!("space-other"))),
            ("another decision ref in the claims", Box::new(|c: &mut Value| c["decision_ref"] = json!("other-ref"))),
        ];
        for (why, edit) in cases {
            let mut claims = base_claims();
            edit(&mut claims);
            refused(verify(&token_for(claims, THREAD)), why);
        }
    }

    #[test]
    fn a_valid_decision_asked_for_another_thread_space_owner_org_or_ref_is_refused() {
        let _keys = StreamKeysEnv::publish();
        let token = valid_token();
        let at = |org: &str, user: &str, space: &str, thread: &str, decision_ref: &str| {
            verify_personal_context_decision(&token, decision_ref, org, user, space, thread, now())
        };
        at(ORG, USER, SPACE, THREAD, DECISION_REF).expect("the request it was issued for");
        refused(at(ORG, USER, SPACE, "thread-2", DECISION_REF), "another thread (the digest binds it)");
        refused(at(ORG, USER, "space-other", THREAD, DECISION_REF), "another Space");
        refused(at(ORG, "user-2", SPACE, THREAD, DECISION_REF), "another subject");
        refused(at("org-2", USER, SPACE, THREAD, DECISION_REF), "another org");
        refused(at(ORG, USER, SPACE, THREAD, "other-ref"), "another decision ref");
        // A decision whose digest was bound to a different thread does not verify for this one.
        refused(verify(&token_for(base_claims(), "thread-2")), "a digest bound to another thread");
    }

    #[test]
    fn a_zero_retention_decision_is_refused() {
        let _keys = StreamKeysEnv::publish();
        let mut claims = base_claims();
        claims["zero_data_retention"] = json!(true);
        refused(verify(&token_for(claims, THREAD)), "such a decision is never issued");
    }

    #[test]
    fn an_incomplete_decision_is_refused() {
        let _keys = StreamKeysEnv::publish();
        for field in ["purpose", "lawful_basis", "privacy_class", "retention_class", "residency", "deletion_scope", "nonce", "recipient_audience_ref", "recipient_audience_hash"] {
            let mut claims = base_claims();
            claims[field] = json!("  ");
            refused(verify(&token_for(claims, THREAD)), field);
        }
        let mut no_revision = base_claims();
        no_revision["recipient_audience_revision"] = json!(0);
        refused(verify(&token_for(no_revision, THREAD)), "a zero audience revision");
    }

    #[test]
    fn the_window_is_two_minutes_with_one_minute_of_clock_skew() {
        let _keys = StreamKeysEnv::publish();
        let with = |issued: DateTime<Utc>, expires: DateTime<Utc>| {
            let mut claims = base_claims();
            claims["issued_at"] = json!(issued.to_rfc3339());
            claims["expires_at"] = json!(expires.to_rfc3339());
            token_for(claims, THREAD)
        };
        let minute = chrono::Duration::minutes(1);
        let second = chrono::Duration::seconds(1);
        // Expired, and expiring this very instant.
        refused(verify(&with(now() - minute * 3, now() - second)), "expired a second ago");
        refused(verify(&with(now() - minute * 3, now())), "expires exactly now");
        verify(&with(now() - minute, now() + second)).expect("one second left");
        // Issued ahead of this clock: a minute of skew is fine, more is not.
        verify(&with(now() + minute, now() + minute * 3)).expect("issued exactly one minute ahead");
        refused(verify(&with(now() + minute + second, now() + minute * 3)), "issued more than a minute ahead");
    }

    #[test]
    fn a_token_that_is_not_a_well_formed_signed_decision_is_refused() {
        let _keys = StreamKeysEnv::publish();
        let valid = valid_token();
        let parts: Vec<&str> = valid.split('.').collect();
        assert_eq!(parts.len(), 4);

        refused(verify(""), "an empty token");
        refused(verify("   "), "a blank token");
        refused(verify(&valid.replacen("v2.", "v1.", 1)), "another envelope version");
        refused(verify(&format!("{}.{}.{}", parts[0], parts[1], parts[2])), "three parts");
        refused(verify(&format!("{valid}.extra")), "five parts");
        refused(verify(&format!("{}.{}.!!!.{}", parts[0], parts[1], parts[3])), "a payload that is not base64");
        refused(verify(&format!("{}.{}.{}.!!!", parts[0], parts[1], parts[2])), "a signature that is not base64");
        refused(verify(&format!("{}.!!!.{}.{}", parts[0], parts[2], parts[3])), "a key id that is not base64");

        // A payload that was changed after signing no longer verifies.
        let mut forged_claims = base_claims();
        forged_claims["subject_id"] = json!("user-2");
        let forged_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&forged_claims).expect("claims"));
        refused(verify(&format!("{}.{}.{}.{}", parts[0], parts[1], forged_payload, parts[3])), "a tampered payload");

        // Signed by a key the service does not hold, under the id it does.
        let stranger = SigningKey::from_bytes(&[7_u8; 32]);
        let mut claims = base_claims();
        let parsed: PersonalContextClaims = serde_json::from_value(claims.clone()).expect("claims fixture");
        claims["payload_digest"] = json!(personal_context_payload_digest(&parsed, THREAD));
        refused(verify(&sign_with(&stranger, KEY_ID, &claims)), "a signature by another key");
        // Signed under an id the service does not trust.
        refused(verify(&sign_with(&signing_key(), "unknown-key-id", &claims)), "an unknown key id");
    }

    #[test]
    fn blank_inputs_and_an_oversize_token_are_refused_before_any_key_is_read() {
        let valid = valid_token();
        let at = |token: &str, decision_ref: &str, user: &str, space: &str, thread: &str| {
            verify_personal_context_decision(token, decision_ref, ORG, user, space, thread, now())
        };
        refused(at("", DECISION_REF, USER, SPACE, THREAD), "no token");
        refused(at(&valid, "", USER, SPACE, THREAD), "no decision ref");
        refused(at(&valid, "  ", USER, SPACE, THREAD), "a blank decision ref");
        refused(at(&valid, DECISION_REF, "", SPACE, THREAD), "no user");
        refused(at(&valid, DECISION_REF, USER, "", THREAD), "no Space");
        refused(at(&valid, DECISION_REF, USER, SPACE, ""), "no thread");
        let oversize = "a".repeat(MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES + 1);
        let status = at(&oversize, DECISION_REF, USER, SPACE, THREAD).expect_err("an oversize token");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    // ---- the answer (needs Postgres) --------------------------------------------------------

    const NEEDS_DB: &str = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)";

    /// A decision valid at the wall clock now (the answer verifies at `Utc::now()`), bound to `thread`.
    fn live_token(org: &str, user: &str, space: &str, thread: &str, edit: impl FnOnce(&mut Value)) -> String {
        let now = Utc::now();
        let mut claims = base_claims();
        claims["org_id"] = json!(org);
        claims["subject_id"] = json!(user);
        claims["space_ref"] = json!(space);
        claims["issued_at"] = json!(now.to_rfc3339());
        claims["expires_at"] = json!((now + chrono::Duration::minutes(2)).to_rfc3339());
        edit(&mut claims);
        token_for(claims, thread)
    }

    fn grant(token: String) -> PersonalContextGrant {
        PersonalContextGrant {
            token,
            decision_ref: DECISION_REF.to_owned(),
        }
    }

    /// One `agent_memory` row to seed. Every field a recall filter looks at is named.
    struct Seed<'a> {
        scope: &'a str,
        owner: &'a str,
        session_id: Option<&'a str>,
        content: &'a str,
        source_links: Vec<String>,
        review_state: &'a str,
        origin_kind: &'a str,
        scope_id: Option<&'a str>,
        space_ref: Option<&'a str>,
        audience_hash: Option<&'a str>,
        audience_revision: Option<i64>,
        is_latest: bool,
        expired: bool,
    }

    impl<'a> Seed<'a> {
        /// An owner's accepted, current, personal user row.
        fn user(owner: &'a str, content: &'a str) -> Self {
            Self {
                scope: "user",
                owner,
                session_id: None,
                content,
                source_links: Vec::new(),
                review_state: "accepted",
                origin_kind: "personal",
                scope_id: None,
                space_ref: None,
                audience_hash: None,
                audience_revision: None,
                is_latest: true,
                expired: false,
            }
        }
    }

    async fn seed_memory(pool: &PgPool, org: &str, seed: Seed<'_>) {
        let id = mp_ids::new_ulid();
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner, source_links, \
             review_state, origin_kind, scope_id, space_ref, audience_hash, audience_revision, is_latest, expires_at) \
             VALUES ($1, $2, $3, $4, $1, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
             CASE WHEN $15 THEN now() - interval '1 day' ELSE NULL END)",
        )
        .bind(&id)
        .bind(org)
        .bind(seed.session_id)
        .bind(seed.scope)
        .bind(seed.content)
        .bind(seed.owner)
        .bind(&seed.source_links)
        .bind(seed.review_state)
        .bind(seed.origin_kind)
        .bind(seed.scope_id)
        .bind(seed.space_ref)
        .bind(seed.audience_hash)
        .bind(seed.audience_revision)
        .bind(seed.is_latest)
        .bind(seed.expired)
        .execute(pool)
        .await
        .expect("seed agent_memory");
    }

    async fn contents(pool: &PgPool, grant: &PersonalContextGrant, org: &str, owner: &str, thread: &str) -> Vec<String> {
        let answer = personal_only_answer(pool, grant, org, owner, thread, "", 50)
            .await
            .expect("the answer is never an error for a refusal");
        assert!(!answer.degraded && answer.withheld == 0, "{answer:?}");
        let mut contents: Vec<String> = answer.entries.iter().map(|entry| entry.content.clone()).collect();
        contents.sort();
        contents
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn personal_only_answer_returns_only_the_owners_origin_personal_user_rows() {
        let Some(pool) = crate::space::test_support::connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let suffix = mp_ids::new_ulid();
        let org = format!("org-pc-{suffix}");
        let space = format!("space-pc-{suffix}");
        let owner = "owner-1";
        let room = format!("thread-room-{suffix}");
        crate::space::test_support::seed_space_thread(&pool, &org, &space, &room, owner).await;

        let linked = vec![format!("thread:{room}"), "message:m1".to_owned()];
        seed_memory(&pool, &org, Seed::user(owner, "own personal fact")).await;
        seed_memory(&pool, &org, Seed::user(owner, "own second fact")).await;
        // Everything below must stay out of the answer.
        seed_memory(&pool, &org, Seed { scope: "org", owner: "", ..Seed::user(owner, "org policy wording") }).await;
        seed_memory(&pool, &org, Seed::user("user-2", "another member's personal fact")).await;
        seed_memory(
            &pool,
            &org,
            Seed {
                scope: "space",
                owner: "",
                scope_id: Some(&space),
                origin_kind: "room",
                space_ref: Some(&space),
                audience_hash: Some("sha256:audience-1"),
                audience_revision: Some(1),
                ..Seed::user(owner, "what the room said")
            },
        )
        .await;
        seed_memory(&pool, &org, Seed { origin_kind: "room_unverified", ..Seed::user(owner, "withheld legacy row") }).await;
        seed_memory(&pool, &org, Seed { source_links: linked, ..Seed::user(owner, "learned in the room before containment") }).await;
        seed_memory(&pool, &org, Seed { scope: "thread", session_id: Some(&room), owner: "", ..Seed::user(owner, "this thread's own note") }).await;
        seed_memory(&pool, &org, Seed { review_state: "pending", ..Seed::user(owner, "not accepted yet") }).await;
        seed_memory(&pool, &org, Seed { expired: true, ..Seed::user(owner, "expired fact") }).await;
        seed_memory(&pool, &org, Seed { is_latest: false, ..Seed::user(owner, "an older version") }).await;

        let token = live_token(&org, owner, &space, &room, |_| {});
        let answer = personal_only_answer(&pool, &grant(token), &org, owner, &room, "", 50)
            .await
            .expect("a verified grant is answered");
        let mut found: Vec<&str> = answer.entries.iter().map(|entry| entry.content.as_str()).collect();
        found.sort_unstable();
        assert_eq!(found, ["own personal fact", "own second fact"], "{NEEDS_DB}");
        for entry in &answer.entries {
            assert_eq!(entry.user_id, owner);
            assert_eq!(entry.topic, "USER");
            assert_eq!(entry.withheld_reason, "");
        }

        // A decision bound to another thread, another owner or zero retention answers with no rows.
        let other_thread = grant(live_token(&org, owner, &space, "thread-elsewhere", |_| {}));
        assert!(contents(&pool, &other_thread, &org, owner, &room).await.is_empty(), "a decision for another thread");
        let other_owner = grant(live_token(&org, "owner-2", &space, &room, |_| {}));
        assert!(contents(&pool, &other_owner, &org, owner, &room).await.is_empty(), "a decision for another owner");
        let zdr = grant(live_token(&org, owner, &space, &room, |claims| claims["zero_data_retention"] = json!(true)));
        assert!(contents(&pool, &zdr, &org, owner, &room).await.is_empty(), "a zero-retention decision");
        let garbage = grant("not-a-token".to_owned());
        assert!(contents(&pool, &garbage, &org, owner, &room).await.is_empty(), "a token that does not parse");

        crate::space::test_support::clean_space(&pool, &org, &space).await;
        sqlx::query("DELETE FROM agent_memory WHERE org_id = $1")
            .bind(&org)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn personal_only_answer_refuses_a_thread_that_is_not_a_shared_room() {
        let Some(pool) = crate::space::test_support::connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let suffix = mp_ids::new_ulid();
        let org = format!("org-pc-{suffix}");
        let space = format!("space-pc-{suffix}");
        let owner = "owner-1";
        let personal = format!("thread-personal-{suffix}");
        let plain = format!("thread-plain-{suffix}");
        crate::space::test_support::seed_space_thread(&pool, &org, &space, &personal, owner).await;
        sqlx::query("UPDATE threads SET space_kind = 'personal' WHERE id = $1")
            .bind(&personal)
            .execute(&pool)
            .await
            .expect("stamp a personal Space thread");
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
            .bind(&plain)
            .bind(&org)
            .bind(owner)
            .execute(&pool)
            .await
            .expect("seed an ordinary chat thread");
        seed_memory(&pool, &org, Seed::user(owner, "own personal fact")).await;

        for (why, thread) in [
            ("a personal Space thread", personal.as_str()),
            ("an ordinary chat thread", plain.as_str()),
            ("a thread that does not exist", "thread-missing"),
        ] {
            // The decision is perfectly valid for that thread id; the thread is not a shared room.
            let granted = grant(live_token(&org, owner, &space, thread, |_| {}));
            assert!(contents(&pool, &granted, &org, owner, thread).await.is_empty(), "{why}, {NEEDS_DB}");
        }

        crate::space::test_support::clean_space(&pool, &org, &space).await;
        for sql in ["DELETE FROM agent_memory WHERE org_id = $1", "DELETE FROM threads WHERE org_id = $1"] {
            sqlx::query(sql).bind(&org).execute(&pool).await.ok();
        }
    }
}
