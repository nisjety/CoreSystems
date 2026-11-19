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
    // WS17-S9: the recipient audience commitment and revision Control signed with
    // the decision. Required (no serde default): a decision without them is refused.
    recipient_audience_hash: String,
    recipient_audience_revision: i64,
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
    // D18: the member's history window. Both are additive, signed with the decision and in no digest.
    // Absent on a decision whose member has no window. A value that does not parse refuses the whole
    // decision (see `HistoryFloor::from_claims`), so a malformed floor is never read as no floor.
    #[serde(default)]
    history_floor_at: Option<DateTime<Utc>>,
    #[serde(default)]
    history_floor_revision: i64,
}

/// D18: one member's history window, as the Control-signed retrieval decision carries it.
///
/// Control signs the window when the member has one. `history_floor_at` is an instant: a per-member
/// since_date window, or a member's join. `history_floor_revision` is the first recipient-audience
/// revision a since_join window starts at (WS04-S11). The window is a presentation narrowing of a read
/// that is already authorised, so it can only HIDE: it never widens a document ACL, and a Space still
/// never widens one either. It is honoured with no rollout switch, because a hide must never depend on one.
///
/// A Data Plane row has a source INSTANT (`documents.created_at`: when the file entered the room;
/// `wiki_pages.created_at`) and no audience revision, so the engine can place a row against an instant
/// and against nothing else:
///
/// * `Since(instant)` is exact. A row is shown only when its source instant is at or after it, and a hit
///   with no reliable source instant is never shown (fail closed). This is also what a per-member
///   since_join window applies: Control signs the member's join instant next to the revision.
/// * a revision with no instant (Control signs that for a WS04-S11 since_join ROOM, behind its
///   `SPACE_HISTORY_WINDOW_CLAIM`) leaves no way to place any row against the window. `Closed` shows
///   NOTHING rather than the whole room: a retrieval that cannot prove a hit is inside the window hides it.
///
/// An entity is placed by the instant it ENTERED the room, never by a later edit: a document or page
/// created before the floor stays hidden after an update, because the update does not remove the
/// text the member must not see (the same rule the canvas window uses).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryFloor {
    /// No window: every row the read was already allowed to show.
    #[default]
    Open,
    /// The window starts at this instant, at microsecond precision (the precision of `timestamptz`).
    Since(DateTime<Utc>),
    /// A window the engine cannot place any row against: nothing is shown.
    Closed,
}

impl HistoryFloor {
    /// The window a verified decision's two floor claims describe. A negative revision, or an instant at
    /// or before the Unix epoch (the zero time of a client that never set it), is not a window Control
    /// signs, so the decision is refused: a malformed floor must never read as no floor.
    pub fn from_claims(at: Option<DateTime<Utc>>, revision: i64) -> anyhow::Result<Self> {
        if revision < 0 {
            anyhow::bail!("Space decision history floor revision is not valid");
        }
        match at {
            Some(at) if at.timestamp() <= 0 => {
                anyhow::bail!("Space decision history floor instant is not valid")
            }
            // `timestamptz` keeps microseconds, so the floor is cut to them here: the SQL predicate and
            // the Rust check then agree on the instant, and one window has one cache key.
            Some(at) => DateTime::<Utc>::from_timestamp_micros(at.timestamp_micros())
                .map(Self::Since)
                .ok_or_else(|| anyhow::anyhow!("Space decision history floor instant is not valid")),
            None if revision > 0 => Ok(Self::Closed),
            None => Ok(Self::Open),
        }
    }

    /// Whether the window shows nothing at all.
    #[must_use]
    pub const fn is_closed(self) -> bool {
        matches!(self, Self::Closed)
    }

    /// The instant a SQL predicate binds (`created_at >= $n`). `None` is no timestamp bound: callers check
    /// [`Self::is_closed`] first, because a closed window has no instant and must not read as an open one.
    #[must_use]
    pub const fn since(self) -> Option<DateTime<Utc>> {
        match self {
            Self::Since(at) => Some(at),
            Self::Open | Self::Closed => None,
        }
    }

    /// Whether a row with this source instant may be shown. This is the Rust copy of the SQL predicate, for
    /// the stage that checks the rows the retrieval arms returned: a predicate that drifted in SQL still
    /// cannot show a row before the floor. `None` is a row with no reliable instant.
    #[must_use]
    pub fn admits(self, source_at: Option<DateTime<Utc>>) -> bool {
        match self {
            Self::Open => true,
            Self::Closed => false,
            Self::Since(floor) => source_at.is_some_and(|at| at >= floor),
        }
    }

    /// The part of a retrieval-cache key that names this window, or `None` for no window so that a room
    /// with no window keeps the key it always had. Two members of one room under different windows, or one
    /// member before and after the window moved, never share an answer.
    fn cache_part(self) -> Option<String> {
        match self {
            Self::Open => None,
            Self::Closed => Some("floor=closed".to_owned()),
            Self::Since(at) => Some(format!(
                "floor={}",
                at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
            )),
        }
    }
}

/// The window of an optional Space scope: `Open` for the unscoped, legacy org read.
#[must_use]
pub fn history_floor_of(scope: Option<&ResolvedSpaceRetrievalScope>) -> HistoryFloor {
    scope.map_or(HistoryFloor::Open, |scope| scope.authority.history_floor)
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
    // D18: read after the decision is known to be signed and addressed to this call, so a floor is only
    // ever the one Control signed. A floor that does not parse as a window refuses the decision.
    let history_floor =
        HistoryFloor::from_claims(claims.history_floor_at, claims.history_floor_revision)?;
    let authority = VerifiedSpaceAuthority {
        authority_revision: claims.authority_revision,
        decision_ref: claims.decision_ref,
        history_floor,
        org_id: claims.org_id,
        privacy_policy_ref: claims.privacy_policy_ref,
        recipient_audience_hash: claims.recipient_audience_hash,
        recipient_audience_ref: claims.recipient_audience_ref,
        recipient_audience_revision: claims.recipient_audience_revision,
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
    /// D18: the member's history window Control signed with the decision. Every read this authority guards
    /// applies it; `Open` (the default) means the member has no window.
    pub history_floor: HistoryFloor,
    pub org_id: String,
    pub privacy_policy_ref: String,
    /// WS17-S9: the recipient audience commitment Control signed (content-free hash).
    pub recipient_audience_hash: String,
    pub recipient_audience_ref: String,
    /// WS17-S9: the recipient audience revision Control signed; must be positive.
    pub recipient_audience_revision: i64,
    pub resource_authorization_ref: String,
    pub space_ref: String,
}

impl VerifiedSpaceAuthority {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.authority_revision <= 0 {
            anyhow::bail!("Space authority revision must be positive");
        }
        if self.recipient_audience_revision <= 0 {
            anyhow::bail!("Space authority recipient audience revision must be positive");
        }
        for (name, value) in [
            ("decision_ref", &self.decision_ref),
            ("org_id", &self.org_id),
            ("privacy_policy_ref", &self.privacy_policy_ref),
            ("recipient_audience_hash", &self.recipient_audience_hash),
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

    /// The retrieval-cache partition of this authority (WS17-S9). A Space-scoped
    /// answer is reused only for the same Space under the same recipient audience
    /// hash and revision, never for another room or an older audience. A member's
    /// history window (D18) is part of the partition too: the window is applied
    /// before the cache read's filters, and it can move without the audience moving,
    /// so an answer shown under one window is never served under another. A member
    /// with no window keeps the partition they always had.
    pub fn cache_partition(&self) -> String {
        let base = format!(
            "space={};audience={};audience_revision={}",
            self.space_ref, self.recipient_audience_hash, self.recipient_audience_revision
        );
        match self.history_floor.cache_part() {
            Some(floor) => format!("{base};{floor}"),
            None => base,
        }
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
    // D18: the member's window narrows the room's document set here, once, so every arm that pins itself
    // to `document_ids` (the dense vertical, the by-id lookups' scope) starts from rows inside the window.
    let (document_ids, document_ids_truncated) = resolve_space_document_ids(
        pool,
        &authority.org_id,
        &authority.space_ref,
        authority.history_floor,
    )
    .await?;
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
///
/// D18: `floor` is the member's history window. Only documents that entered the room at or after it are
/// listed (`documents.created_at`; a later update never moves a document back into the window), so the
/// bound above counts the documents the member may see. A closed window lists none and reads nothing. The
/// caller then meets the same "no documents imported under Space authority" refusal as for an empty
/// room, which tells a windowed member nothing about what the window hides.
pub async fn resolve_space_document_ids(
    pool: &PgPool,
    org_id: &str,
    space_ref: &str,
    floor: HistoryFloor,
) -> anyhow::Result<(Vec<String>, bool)> {
    if org_id.trim().is_empty() || space_ref.trim().is_empty() {
        anyhow::bail!("Space document scope requires an org and a Space reference");
    }
    if floor.is_closed() {
        return Ok((Vec::new(), false));
    }
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = sqlx::query_scalar::<_, String>(SPACE_DOCUMENT_IDS_SQL)
        .bind(org_id)
        .bind(space_ref)
        .bind(MAX_SPACE_DOCUMENTS + 1)
        .bind(floor.since())
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let truncated = rows.len() as i64 > MAX_SPACE_DOCUMENTS;
    let mut rows = rows;
    rows.truncate(MAX_SPACE_DOCUMENTS as usize);
    Ok((rows, truncated))
}

/// The room's live documents, newest first. `$4` is the member's history floor (D18): NULL for no window,
/// otherwise the instant a document must have entered the room at or after. `created_at` is that instant:
/// `space_ref` is written once, on create, from a verified Control import decision.
const SPACE_DOCUMENT_IDS_SQL: &str = r#"
        SELECT document_id
        FROM documents
        WHERE org_id = $1
          AND space_ref = $2
          AND deleted_at IS NULL
          AND ($4::timestamptz IS NULL OR created_at >= $4::timestamptz)
        ORDER BY updated_at DESC
        LIMIT $3
        "#;

#[derive(sqlx::FromRow)]
struct SpaceBindingRow {
    workspace_id: Option<String>,
    collection_id: Option<String>,
    owner_resource_ref: String,
}

#[cfg(test)]
mod tests {
    use super::{
        history_floor_of, verify_retrieval_space_decision, HistoryFloor, ResolvedSpaceRetrievalScope,
        VerifiedSpaceAuthority, DECISION_VERSION, RETRIEVAL_AUDIENCE, SPACE_DOCUMENT_IDS_SQL,
    };
    use crate::pipeline::types::RetrievalFiltersInput;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use chrono::{DateTime, TimeZone, Utc};
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::collections::BTreeMap;

    /// A complete Control decision for `org-1` / `user-1`. The recipient audience hash and revision
    /// are signed claims (WS17-S9); a decision without them is refused.
    fn retrieval_decision_payload() -> serde_json::Value {
        let now = Utc::now();
        serde_json::json!({
            "decision_ref":"decision-1", "org_id":"org-1", "space_ref":"space-1", "subject_id":"user-1",
            "service_audience": RETRIEVAL_AUDIENCE, "recipient_audience_ref":"audience-1",
            "recipient_audience_hash":"audience-hash-1", "recipient_audience_revision": 1,
            "privacy_policy_ref":"privacy-1",
            "resource_authorization_ref":"resource-1", "authority_revision": 1, "permissions":["retrieval:read"],
            "purpose":"assistant_collaboration", "lawful_basis":"contract", "privacy_class":"internal",
            "retention_class":"standard", "residency":"swedencentral", "deletion_scope":"space", "nonce":"nonce-1",
            "issued_at": now.to_rfc3339(), "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339()
        })
    }

    fn sign_decision(key: &SigningKey, key_id: &str, payload: &serde_json::Value) -> String {
        let key_part = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let payload_part = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap());
        let input = format!("{DECISION_VERSION}.{key_part}.{payload_part}");
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(key.sign(input.as_bytes()).to_bytes())
        )
    }

    fn signed_retrieval_decision(key: &SigningKey, key_id: &str) -> String {
        sign_decision(key, key_id, &retrieval_decision_payload())
    }

    fn scope() -> ResolvedSpaceRetrievalScope {
        ResolvedSpaceRetrievalScope {
            authority: VerifiedSpaceAuthority {
                authority_revision: 4,
                decision_ref: "decision_1".into(),
                history_floor: HistoryFloor::default(),
                org_id: "org_1".into(),
                privacy_policy_ref: "privacy_3".into(),
                recipient_audience_hash: "audience_hash_2".into(),
                recipient_audience_ref: "audience_2".into(),
                recipient_audience_revision: 3,
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

    // WS17-S9: the recipient audience hash and revision are required, signed claims. A decision that
    // omits either one, or whose audience is blank or has a revision that is not positive, is refused.
    #[test]
    fn a_retrieval_decision_without_the_recipient_audience_claims_is_refused() {
        let signing = SigningKey::from_bytes(&[22; 32]);
        let keys = BTreeMap::from([("control-current".to_owned(), signing.verifying_key())]);
        let verify = |payload: &serde_json::Value| {
            let token = sign_decision(&signing, "control-current", payload);
            verify_retrieval_space_decision(&token, &keys, "org-1", "user-1", Utc::now())
        };
        assert!(
            verify(&retrieval_decision_payload()).is_ok(),
            "the complete decision verifies"
        );

        for claim in ["recipient_audience_hash", "recipient_audience_revision"] {
            let mut payload = retrieval_decision_payload();
            payload
                .as_object_mut()
                .expect("object payload")
                .remove(claim);
            // Required by the claims struct (no serde default), so the parse is what refuses it.
            let error = verify(&payload).expect_err("a decision without the claim must be refused");
            assert!(
                error.to_string().contains("invalid Space decision claims"),
                "{claim}: {error}"
            );
        }

        let mut zero_revision = retrieval_decision_payload();
        zero_revision["recipient_audience_revision"] = serde_json::json!(0);
        let error = verify(&zero_revision).expect_err("a zero audience revision must be refused");
        assert!(
            error
                .to_string()
                .contains("recipient audience revision must be positive"),
            "{error}"
        );

        let mut blank_hash = retrieval_decision_payload();
        blank_hash["recipient_audience_hash"] = serde_json::json!("   ");
        let error = verify(&blank_hash).expect_err("a blank audience hash must be refused");
        assert!(
            error.to_string().contains("recipient_audience_hash"),
            "{error}"
        );
    }

    // WS17-S9: the retrieval-cache partition of a Space-scoped answer names its room, its recipient
    // audience hash and its audience revision, so none of the three can share a cache entry.
    #[test]
    fn a_space_answers_cache_partition_names_its_room_and_its_audience() {
        let base = scope().authority;
        assert_eq!(base.cache_partition(), scope().authority.cache_partition());

        let mut other_room = base.clone();
        other_room.space_ref = "space_2".into();
        let mut other_audience = base.clone();
        other_audience.recipient_audience_hash = "audience_hash_9".into();
        let mut newer_audience = base.clone();
        newer_audience.recipient_audience_revision = 4;
        for other in [other_room, other_audience, newer_audience] {
            assert_ne!(other.cache_partition(), base.cache_partition());
        }
    }

    // ── D18: the member's history window ──────────────────────────────────────────────────────────

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .expect("test instant")
            .with_timezone(&Utc)
    }

    /// Verifies the standard decision with `claims` merged over it, signed by the right key.
    fn verify_with(claims: serde_json::Value) -> anyhow::Result<VerifiedSpaceAuthority> {
        let signing = SigningKey::from_bytes(&[31; 32]);
        let keys = BTreeMap::from([("control-current".to_owned(), signing.verifying_key())]);
        let mut payload = retrieval_decision_payload();
        for (name, value) in claims.as_object().expect("claims to merge") {
            payload[name.as_str()] = value.clone();
        }
        let token = sign_decision(&signing, "control-current", &payload);
        verify_retrieval_space_decision(&token, &keys, "org-1", "user-1", Utc::now())
    }

    #[test]
    fn a_decision_with_no_floor_claims_has_an_open_window() {
        let authority = verify_with(serde_json::json!({})).expect("a decision with no window");
        assert_eq!(authority.history_floor, HistoryFloor::Open);
        assert_eq!(authority.history_floor.since(), None);
        assert!(!authority.history_floor.is_closed());
        // An explicit null is the same as absent: Control omits the claim when the member has no window.
        let null = verify_with(serde_json::json!({ "history_floor_at": null, "history_floor_revision": 0 }))
            .expect("null claims");
        assert_eq!(null.history_floor, HistoryFloor::Open);
    }

    #[test]
    fn a_signed_floor_instant_is_the_window_cut_to_the_precision_of_timestamptz() {
        let authority = verify_with(serde_json::json!({ "history_floor_at": "2026-11-18T09:30:15.123456789Z" }))
            .expect("a since_date window");
        assert_eq!(authority.history_floor, HistoryFloor::Since(at("2026-11-18T09:30:15.123456Z")));
        assert_eq!(authority.history_floor.since(), Some(at("2026-11-18T09:30:15.123456Z")));
        // An offset is read as the instant it names.
        let offset = verify_with(serde_json::json!({ "history_floor_at": "2026-11-18T10:30:15+01:00" }))
            .expect("an offset instant");
        assert_eq!(offset.history_floor, HistoryFloor::Since(at("2026-11-18T09:30:15Z")));
    }

    #[test]
    fn an_instant_wins_over_a_revision_and_a_revision_alone_closes_the_window() {
        // A per-member since_join window: Control signs the join instant next to the revision. The
        // instant places a row; a document has no audience revision to compare.
        let member = verify_with(serde_json::json!({
            "history_floor_at": "2026-11-18T09:30:00Z", "history_floor_revision": 5
        }))
        .expect("a since_join member window");
        assert_eq!(member.history_floor, HistoryFloor::Since(at("2026-11-18T09:30:00Z")));

        // A WS04-S11 since_join room signs the revision alone. Nothing in Data can be placed against a
        // revision, so the window shows nothing (fail closed) instead of the whole room.
        let room = verify_with(serde_json::json!({ "history_floor_revision": 5 })).expect("a since_join room");
        assert_eq!(room.history_floor, HistoryFloor::Closed);
        assert!(room.history_floor.is_closed());
        assert_eq!(room.history_floor.since(), None, "a closed window has no instant to bind");
    }

    // A floor narrows a read, so one that cannot be read as a window refuses the decision: it must never
    // fall back to showing the whole room.
    #[test]
    fn a_malformed_floor_refuses_the_decision() {
        for (claims, why) in [
            (serde_json::json!({ "history_floor_at": "last tuesday" }), "text that is no instant"),
            (serde_json::json!({ "history_floor_at": 1_700_000_000 }), "a bare number"),
            (serde_json::json!({ "history_floor_at": true }), "a boolean"),
            (serde_json::json!({ "history_floor_at": {} }), "an object"),
            (serde_json::json!({ "history_floor_revision": "5" }), "a revision that is text"),
            (serde_json::json!({ "history_floor_revision": 1.5 }), "a revision that is a fraction"),
        ] {
            let error = verify_with(claims).expect_err(why);
            assert!(error.to_string().contains("invalid Space decision claims"), "{why}: {error}");
        }
        for (claims, why) in [
            (serde_json::json!({ "history_floor_at": "0001-01-01T00:00:00Z" }), "the zero time"),
            (serde_json::json!({ "history_floor_at": "1970-01-01T00:00:00Z" }), "the Unix epoch"),
            (serde_json::json!({ "history_floor_at": "1969-12-31T23:59:59Z" }), "before the epoch"),
            (serde_json::json!({ "history_floor_revision": -1 }), "a negative revision"),
        ] {
            let error = verify_with(claims).expect_err(why);
            assert!(error.to_string().contains("history floor"), "{why}: {error}");
        }
    }

    // The floor is signed with the decision: a copy with the floor removed, or moved, no longer verifies.
    #[test]
    fn a_floor_cannot_be_removed_or_moved_after_signing() {
        let signing = SigningKey::from_bytes(&[32; 32]);
        let keys = BTreeMap::from([("control-current".to_owned(), signing.verifying_key())]);
        let mut payload = retrieval_decision_payload();
        payload["history_floor_at"] = serde_json::json!("2026-11-18T09:30:00Z");
        let token = sign_decision(&signing, "control-current", &payload);
        assert!(verify_retrieval_space_decision(&token, &keys, "org-1", "user-1", Utc::now()).is_ok());

        let parts: Vec<&str> = token.split('.').collect();
        for tampered_floor in [None, Some("2026-11-18T10:30:00Z")] {
            let mut forged = payload.clone();
            match tampered_floor {
                Some(floor) => forged["history_floor_at"] = serde_json::json!(floor),
                None => {
                    forged.as_object_mut().expect("object").remove("history_floor_at");
                }
            }
            let forged_token = format!(
                "{}.{}.{}.{}",
                parts[0],
                parts[1],
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&forged).expect("payload")),
                parts[3]
            );
            let error = verify_retrieval_space_decision(&forged_token, &keys, "org-1", "user-1", Utc::now())
                .expect_err("an edited payload under the old signature");
            assert!(error.to_string().contains("invalid Space decision signature"), "{error}");
        }
    }

    #[test]
    fn a_window_admits_a_row_only_when_its_source_instant_is_inside_it() {
        let floor = at("2026-11-18T09:30:00Z");
        let before = floor - chrono::Duration::microseconds(1);
        let after = floor + chrono::Duration::microseconds(1);
        // No window shows every row, including one with no instant.
        assert!(HistoryFloor::Open.admits(Some(before)));
        assert!(HistoryFloor::Open.admits(None));
        // A window with an instant shows a row at or after it, never one before it, and never a row whose
        // instant is unknown (fail closed).
        let window = HistoryFloor::Since(floor);
        assert!(!window.admits(Some(before)), "a microsecond before the floor is hidden");
        assert!(window.admits(Some(floor)), "the floor instant itself is shown");
        assert!(window.admits(Some(after)));
        assert!(!window.admits(None), "a hit with no reliable instant is hidden");
        // A closed window shows nothing, whatever the instant.
        assert!(!HistoryFloor::Closed.admits(Some(after)));
        assert!(!HistoryFloor::Closed.admits(None));
        // The default is the open window.
        assert_eq!(HistoryFloor::default(), HistoryFloor::Open);
    }

    #[test]
    fn the_window_of_a_scope_is_open_without_a_scope() {
        assert_eq!(history_floor_of(None), HistoryFloor::Open);
        let mut windowed = scope();
        windowed.authority.history_floor = HistoryFloor::Since(Utc.with_ymd_and_hms(2026, 11, 18, 9, 30, 0).unwrap());
        assert_eq!(history_floor_of(Some(&windowed)), windowed.authority.history_floor);
        assert_eq!(history_floor_of(Some(&scope())), HistoryFloor::Open);
    }

    // The retrieval cache is partitioned by the window: it is applied after the cache read, and it can
    // move while the audience does not. A room with no window keeps the key it always had.
    #[test]
    fn the_cache_partition_names_the_members_window() {
        let open = scope().authority;
        assert_eq!(
            open.cache_partition(),
            "space=space_1;audience=audience_hash_2;audience_revision=3",
            "a member with no window keeps the partition they always had"
        );
        let window = |floor| {
            let mut authority = scope().authority;
            authority.history_floor = floor;
            authority.cache_partition()
        };
        let since_nine = window(HistoryFloor::Since(at("2026-11-18T09:00:00Z")));
        let since_ten = window(HistoryFloor::Since(at("2026-11-18T10:00:00Z")));
        let closed = window(HistoryFloor::Closed);
        for (name, partition) in [("since nine", &since_nine), ("since ten", &since_ten), ("closed", &closed)] {
            assert_ne!(partition, &open.cache_partition(), "{name} shares the open partition");
        }
        assert_ne!(since_nine, since_ten, "two instants share a partition");
        assert_ne!(since_nine, closed);
        assert_ne!(since_ten, closed);
        assert_eq!(since_nine, window(HistoryFloor::Since(at("2026-11-18T09:00:00Z"))), "one window, one key");
        assert!(since_nine.starts_with(&open.cache_partition()), "the window extends the room and audience part");
    }

    // The room's document set is read with the window as a SQL predicate, so the bound counts only what
    // the member may see and a document that entered the room before the floor is never listed.
    #[test]
    fn the_room_document_set_is_read_inside_the_window() {
        let sql = SPACE_DOCUMENT_IDS_SQL.split_whitespace().collect::<Vec<_>>().join(" ");
        for needle in [
            "WHERE org_id = $1 AND space_ref = $2 AND deleted_at IS NULL",
            "AND ($4::timestamptz IS NULL OR created_at >= $4::timestamptz)",
            "ORDER BY updated_at DESC LIMIT $3",
        ] {
            assert!(sql.contains(needle), "the room document SQL lost `{needle}`: {sql}");
        }
    }
}
