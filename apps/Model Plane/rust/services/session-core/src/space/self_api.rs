//! `SpaceRunActions`: the run-bound owner path of the Space agent self-API (build plan WS18-S3).
//!
//! A room agent acts on its own run's thread through a row of the golden registry
//! (`apps/Model Plane/api/contracts/space-self-api-registry.json`). The model proposes; this service
//! executes, and it checks everything first. `ApplyRunThreadAction` runs these checks in order, and
//! each one fails closed:
//!
//! 1. `authorize_operation(caller, "session:run-thread-action")`: a zero-retention credential is
//!    refused with the typed `zdr_durable_mutation_forbidden`.
//! 2. The switch `SPACE_SELF_API_SESSION_CORE`, read once at boot. Off answers `feature_disabled`.
//! 3. The request shape: bounded ids, exactly the argument message the action takes, and the
//!    arguments within the golden schema. The payload digest is computed here from the typed
//!    arguments, with arrays flattened (`fields.len`, `fields.<i>.label`, `fields.<i>.value`).
//! 4. The action is a row of the golden registry that names this service as its audience.
//! 5. In one transaction: the run joined to its thread (the join `resolve_run_action_authority`
//!    uses), owned by the caller. The caller must be a user principal; a service or an api key is
//!    refused. The run is active, a root run, persistent, and not a `service:` subject.
//! 6. The Control `run-action-v1` decision verifies: envelope, key, Ed25519 signature, and the 29
//!    claims of Control's `RunActionDecision` (`deny_unknown_fields`). Every claim equals the
//!    request, the registry row or the run row, and the digest the claims carry equals the one
//!    recomputed in step 3. The decision lasts at most two minutes. A zero-retention claim is refused.
//! 7. No `space_membership_revocations` row exists for the caller in this Space.
//! 8. The thread row is locked (`FOR UPDATE`). A receipt with the same key replays when the digest
//!    matches and conflicts when it does not. The run's receipts for this action are capped at the
//!    registry row's `max_per_run`.
//! 9. The effect, the receipt and the events commit together. Rename and pin write the presentation
//!    columns and set their `*_source` to `agent`. An agent never overwrites a value a person set.
//!    `space.thread.post_card` is refused as not built here; WS18-S7 writes cards.
//!
//! `ProbeRunThreadActions` answers execution-core only (`service:execution-core` with the scope
//! `session:run-thread-action:probe`). It returns three booleans and no content.
//!
//! Receipts and events carry ids, kinds and digests only, never titles, bodies or arguments. The
//! switch is a rollout control, never an authorization input.

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::time::Instant;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::space_run_actions_server::SpaceRunActions;
use mp_contracts::space_self_api as sa;
use mp_contracts::zdr_refusal::Refusal;
use mp_events::idempotency::derive_idempotency_hash;
use mp_ids::new_ulid;
use sqlx::PgPool;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::decision_keys::lookup_key;
use crate::grpc::{
    configured_control_space_decision_keys, normalize_thread_presentation_text, record_metrics,
    THREAD_PRESENTATION_UPDATED_TYPE_URL, THREAD_TITLE_MAX_CHARS,
};
use crate::space::mentions::coded_status;

/// The scope a user principal's call carries for this operation (`authorize_operation`).
const APPLY_SCOPE: &str = "session:run-thread-action";
/// The probe scope, and the one service identity that may hold it.
const PROBE_SCOPE: &str = "session:run-thread-action:probe";
const PROBE_CALLER: &str = "service:execution-core";
/// The envelope version Control signs (`runActionDecisionVersion`).
const DECISION_VERSION: &str = "run-action-v1";
/// The audience of every row this service serves (the golden `service_audience`).
const SESSION_CORE_AUDIENCE: &str = "model-plane-session-core";
/// The one permission a Space row's decision carries (the golden `permission`).
const SPACE_ACTION_PERMISSION: &str = "space-action:execute";
/// Control's `personalDecisionLifetime` (two minutes), and the clock skew a decision may show.
const DECISION_MAX_LIFE_MINUTES: i64 = 2;
const DECISION_SKEW_MINUTES: i64 = 1;
/// Longest id, idempotency key and decision envelope accepted on the request.
const MAX_ID_BYTES: usize = 128;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 200;
const MAX_DECISION_TOKEN_BYTES: usize = 16 * 1024;

/// The registry actions this service serves.
pub(crate) const RENAME_ACTION: &str = "space.thread.rename";
pub(crate) const PIN_ACTION: &str = "space.thread.pin";
pub(crate) const POST_CARD_ACTION: &str = "space.thread.post_card";

/// The argument names of each action, in the golden `argument_order`. A golden that disagrees
/// refuses to serve, so the flattening below can never drift from the registry.
const RENAME_ARGUMENTS: &[&str] = &["title"];
const PIN_ARGUMENTS: &[&str] = &["pinned"];
const POST_CARD_ARGUMENTS: &[&str] = &["title", "body", "tone", "fields"];
const CARD_TONES: [&str; 4] = ["info", "progress", "done", "blocked"];

/// The `*_source` word an agent action writes. A human write writes `owner`.
const AGENT_SOURCE: &str = "agent";
/// The receipt event (ids and kinds only) and its type URL.
const RUN_THREAD_ACTION_EVENT: &str = "RUN_THREAD_ACTION_APPLIED";
const RUN_THREAD_ACTION_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunThreadActionApplied";

/// Typed refusal words, carried on the `x-mp-error-code` trailer.
const FEATURE_DISABLED: &str = "feature_disabled";
const INVALID_ARGUMENTS: &str = "invalid_arguments";
const NOT_OFFERED: &str = "not_offered";
const NOT_AUTHORIZED: &str = "not_authorized";
const RUN_NOT_ACTIVE: &str = "run_not_active";
const TITLE_OWNED_BY_HUMAN: &str = "title_owned_by_human";
const LIMIT_REACHED: &str = "limit_reached";
const IDEMPOTENCY_KEY_REUSED: &str = "idempotency_key_reused";

/// One run joined to its thread, read under the same join as `resolve_run_action_authority`. The
/// caller's owner id is in the predicate, so a run the caller does not own is simply not found.
const RUN_JOIN_SQL: &str = "SELECT r.id AS run_id, r.org_id AS org_id, r.user_id AS subject_id,
        r.thread_id AS thread_id, r.status AS run_status, r.retention_class AS retention_class,
        r.parent_run_id AS parent_run_id, r.space_id AS space_id,
        r.recipient_audience_ref AS recipient_audience_ref,
        r.recipient_audience_revision AS recipient_audience_revision,
        r.recipient_audience_hash AS recipient_audience_hash,
        r.privacy_policy_ref AS privacy_policy_ref,
        r.resource_authorization_ref AS resource_authorization_ref,
        r.authority_revision AS authority_revision
 FROM runs r
 JOIN threads t ON t.id = r.thread_id AND t.org_id = r.org_id AND t.user_id = r.user_id
 WHERE r.id = $1 AND r.org_id = $2 AND r.user_id = $3
   AND r.space_id IS NOT DISTINCT FROM t.space_id
   AND r.recipient_audience_ref IS NOT DISTINCT FROM t.recipient_audience_ref
   AND r.recipient_audience_revision IS NOT DISTINCT FROM t.recipient_audience_revision
   AND r.recipient_audience_hash IS NOT DISTINCT FROM t.recipient_audience_hash
   AND r.privacy_policy_ref IS NOT DISTINCT FROM t.privacy_policy_ref
   AND r.resource_authorization_ref IS NOT DISTINCT FROM t.resource_authorization_ref
   AND r.authority_revision IS NOT DISTINCT FROM t.authority_revision";

/// One run and its thread, as the checks need them.
#[derive(sqlx::FromRow)]
struct RunRow {
    run_id: String,
    org_id: String,
    subject_id: String,
    thread_id: String,
    run_status: String,
    retention_class: String,
    parent_run_id: Option<String>,
    space_id: Option<String>,
    recipient_audience_ref: Option<String>,
    recipient_audience_revision: Option<i64>,
    recipient_audience_hash: Option<String>,
    privacy_policy_ref: Option<String>,
    resource_authorization_ref: Option<String>,
    authority_revision: Option<i64>,
}

/// The thread row, locked for the effect.
#[derive(sqlx::FromRow)]
struct ThreadRow {
    thread_id: String,
    presentation_title: Option<String>,
    pinned_at: Option<DateTime<Utc>>,
    title_source: String,
    pinned_source: String,
}

/// An earlier application of the same idempotency key.
#[derive(sqlx::FromRow)]
struct ReceiptRow {
    receipt_id: String,
    payload_digest: String,
    thread_id: String,
    message_id: Option<String>,
}

/// Control's `RunActionDecision` claims, 29 fields in Control's order. Unknown fields are refused,
/// so a Control change that adds a claim fails closed until this reader is updated with it.
#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RunActionClaims {
    decision_ref: String,
    run_id: String,
    thread_id: String,
    org_id: String,
    space_ref: String,
    subject_id: String,
    service_audience: String,
    action_id: String,
    action_schema_hash: String,
    payload_digest: String,
    idempotency_key: String,
    recipient_audience_ref: String,
    recipient_audience_hash: String,
    recipient_audience_revision: i64,
    privacy_policy_ref: String,
    run_context_authorization_ref: String,
    authority_revision: i64,
    permissions: Vec<String>,
    purpose: String,
    lawful_basis: String,
    privacy_class: String,
    third_party_processing_allowed: bool,
    retention_class: String,
    residency: String,
    deletion_scope: String,
    zero_data_retention: bool,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    nonce: String,
}

/// The typed arguments of one action, validated against the golden schema.
#[derive(Debug)]
enum Arguments {
    Rename {
        title: String,
    },
    Pin {
        pinned: bool,
    },
    PostCard {
        title: String,
        body: String,
        tone: String,
        fields: Vec<(String, String)>,
    },
}

/// How an application ended, on the wire and in the receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Applied,
    Replayed,
    Noop,
}

impl Outcome {
    /// The receipt word. Only `applied` and `noop` are ever stored.
    fn receipt_word(self) -> &'static str {
        match self {
            Self::Noop => "noop",
            Self::Applied | Self::Replayed => "applied",
        }
    }

    fn wire(self) -> i32 {
        match self {
            Self::Applied => pb::RunActionOutcome::Applied as i32,
            Self::Replayed => pb::RunActionOutcome::Replayed as i32,
            Self::Noop => pb::RunActionOutcome::Noop as i32,
        }
    }
}

/// What the request asked for. The decision must bind exactly these.
struct Asked<'a> {
    org_id: &'a str,
    run_id: &'a str,
    action_id: &'a str,
    idempotency_key: &'a str,
    digest: &'a str,
    owner: &'a str,
}

/// One event row to write in the effect's transaction.
struct EventDraft<'a> {
    event_type: &'static str,
    run_id: &'a str,
    thread_id: &'a str,
    org_id: &'a str,
    owner: &'a str,
    key: &'a str,
    type_url: &'static str,
    payload: serde_json::Value,
}

fn coded(status: Status, code: &'static str) -> Status {
    coded_status(status, code)
}

fn zdr_refusal() -> Status {
    Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(tonic::Code::FailedPrecondition)
}

fn db_error(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

/// The one refusal for every failed decision check: the decision is not evidence of authority.
fn not_authorized() -> Status {
    coded(
        Status::permission_denied("Control run action decision does not authorize this action"),
        NOT_AUTHORIZED,
    )
}

fn golden() -> Result<&'static sa::Registry, Status> {
    sa::registry().map_err(|error| Status::internal(error.to_string()))
}

fn within_chars(value: &str, min: usize, max: usize) -> bool {
    let count = value.chars().count();
    count >= min && count <= max
}

fn bounded_id(value: &str, max: usize, field: &str) -> Result<(), Status> {
    if value.trim().is_empty() || value.len() > max {
        return Err(Status::invalid_argument(format!(
            "{field} is required and must be at most {max} bytes"
        )));
    }
    Ok(())
}

/// The owner of a run is a user principal. A service and an api key are never the owner.
fn run_owner(caller: &VerifiedIdentity) -> Result<String, Status> {
    match caller.user_id() {
        Some(user)
            if !caller.is_service() && !caller.is_api_key() && !user.trim().is_empty() =>
        {
            Ok(user.to_owned())
        }
        _ => Err(Status::permission_denied(
            "a run action requires the run owner's user principal",
        )),
    }
}

fn expected_arguments(action_id: &str) -> Option<&'static [&'static str]> {
    match action_id {
        RENAME_ACTION => Some(RENAME_ARGUMENTS),
        PIN_ACTION => Some(PIN_ARGUMENTS),
        POST_CARD_ACTION => Some(POST_CARD_ARGUMENTS),
        _ => None,
    }
}

/// Validates a card against the golden `space.thread.post_card` schema.
fn parse_post_card(card: pb::PostCardArgs) -> Result<Arguments, Status> {
    let invalid = |detail: &'static str| coded(Status::invalid_argument(detail), INVALID_ARGUMENTS);
    if !within_chars(&card.title, 1, 120) {
        return Err(invalid("title must be 1 to 120 characters"));
    }
    if !within_chars(&card.body, 1, 2000) {
        return Err(invalid("body must be 1 to 2000 characters"));
    }
    if !CARD_TONES.contains(&card.tone.as_str()) {
        return Err(invalid("tone must be one of info, progress, done, blocked"));
    }
    if card.fields.len() > 8 {
        return Err(invalid("at most 8 fields"));
    }
    let mut fields = Vec::with_capacity(card.fields.len());
    for field in card.fields {
        if !within_chars(&field.label, 1, 40) || !within_chars(&field.value, 1, 200) {
            return Err(invalid("a field label is 1 to 40 characters and its value 1 to 200"));
        }
        fields.push((field.label, field.value));
    }
    Ok(Arguments::PostCard {
        title: card.title,
        body: card.body,
        tone: card.tone,
        fields,
    })
}

/// Validates the argument message against the action it names. Exactly one message must match.
fn parse_arguments(
    action_id: &str,
    args: Option<pb::apply_run_thread_action_request::Args>,
) -> Result<Arguments, Status> {
    use pb::apply_run_thread_action_request::Args;
    let invalid = |detail: &'static str| coded(Status::invalid_argument(detail), INVALID_ARGUMENTS);
    match (action_id, args) {
        (RENAME_ACTION, Some(Args::Rename(rename))) => {
            if !within_chars(&rename.title, 1, 96) {
                return Err(invalid("title must be 1 to 96 characters"));
            }
            Ok(Arguments::Rename {
                title: rename.title,
            })
        }
        (PIN_ACTION, Some(Args::Pin(pin))) => Ok(Arguments::Pin { pinned: pin.pinned }),
        (POST_CARD_ACTION, Some(Args::PostCard(card))) => parse_post_card(card),
        _ => Err(invalid("the arguments do not match the action")),
    }
}

/// The argument pairs in `argument_order`, arrays flattened in place. An absent `fields` flattens
/// to `fields.len` = 0, so absent and empty give the same digest.
fn argument_pairs(arguments: &Arguments) -> Vec<(String, String)> {
    match arguments {
        Arguments::Rename { title } => vec![("title".to_owned(), title.clone())],
        Arguments::Pin { pinned } => vec![("pinned".to_owned(), pinned.to_string())],
        Arguments::PostCard {
            title,
            body,
            tone,
            fields,
        } => {
            let mut pairs = vec![
                ("title".to_owned(), title.clone()),
                ("body".to_owned(), body.clone()),
                ("tone".to_owned(), tone.clone()),
                ("fields.len".to_owned(), fields.len().to_string()),
            ];
            for (index, (label, value)) in fields.iter().enumerate() {
                pairs.push((format!("fields.{index}.label"), label.clone()));
                pairs.push((format!("fields.{index}.value"), value.clone()));
            }
            pairs
        }
    }
}

/// The payload digest of this call, recomputed here. Control never sees the arguments.
fn argument_digest(
    action_id: &str,
    run_id: &str,
    org_id: &str,
    idempotency_key: &str,
    arguments: &Arguments,
) -> Result<String, Status> {
    let pairs = argument_pairs(arguments);
    let borrowed: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    sa::payload_digest(action_id, run_id, org_id, idempotency_key, &borrowed)
        .map_err(|error| Status::internal(error.to_string()))
}

/// Splits and verifies a `run-action-v1.<kid>.<payload>.<sig>` token. The signature covers the
/// first three segments, exactly as Control signs them.
fn verify_decision(
    token: &str,
    keys: &BTreeMap<String, VerifyingKey>,
) -> Result<RunActionClaims, Status> {
    if token.len() > MAX_DECISION_TOKEN_BYTES {
        return Err(not_authorized());
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 4 || parts[0] != DECISION_VERSION {
        return Err(not_authorized());
    }
    let key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(not_authorized)?;
    let key = lookup_key(keys, &key_id).map_err(|_| not_authorized())?;
    let signature = URL_SAFE_NO_PAD
        .decode(parts[3])
        .ok()
        .and_then(|raw| Signature::from_slice(&raw).ok())
        .ok_or_else(not_authorized)?;
    let signed = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    key.verify(signed.as_bytes(), &signature)
        .map_err(|_| not_authorized())?;
    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| not_authorized())?;
    serde_json::from_slice(&payload).map_err(|_| not_authorized())
}

/// Loads the run joined to its thread. A run the caller does not own, or that is not in this org,
/// is `NotFound`: there is no oracle for another caller's run id.
async fn load_run(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: &str,
    org_id: &str,
    owner: &str,
) -> Result<RunRow, Status> {
    let run: Option<RunRow> = sqlx::query_as(RUN_JOIN_SQL)
        .bind(run_id)
        .bind(org_id)
        .bind(owner)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?;
    run.ok_or_else(|| Status::not_found("run not found"))
}

/// A run that is terminal, a sub-run, a `service:` subject or not persistent cannot act.
fn check_run_is_active(run: &RunRow) -> Result<(), Status> {
    if run.parent_run_id.is_some() || run.subject_id.starts_with("service:") {
        return Err(not_authorized());
    }
    if matches!(run.run_status.as_str(), "completed" | "failed" | "cancelled") {
        return Err(coded(
            Status::failed_precondition("the run is not active"),
            RUN_NOT_ACTIVE,
        ));
    }
    if run.retention_class != "persistent" {
        return Err(zdr_refusal());
    }
    Ok(())
}

/// Every claim of the decision must equal what was asked and what the run row holds, and the
/// decision must be fresh and at most two minutes long.
fn check_decision_binding(
    claims: &RunActionClaims,
    asked: &Asked<'_>,
    run: &RunRow,
    row: &sa::RegistryRow,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    let bound = claims.service_audience == SESSION_CORE_AUDIENCE
        && row.service_audience == SESSION_CORE_AUDIENCE
        && claims.action_id == asked.action_id
        && claims.action_schema_hash == row.schema_sha256
        && claims.payload_digest == asked.digest
        && claims.idempotency_key == asked.idempotency_key
        && claims.permissions.len() == 1
        && claims.permissions[0] == SPACE_ACTION_PERMISSION
        && row.permission == SPACE_ACTION_PERMISSION
        && claims.org_id == asked.org_id
        && run.org_id == asked.org_id
        && claims.run_id == asked.run_id
        && run.run_id == asked.run_id
        && claims.thread_id == run.thread_id
        && claims.subject_id == asked.owner
        && run.subject_id == asked.owner
        && run.space_id.as_deref() == Some(claims.space_ref.as_str())
        && run.recipient_audience_ref.as_deref() == Some(claims.recipient_audience_ref.as_str())
        && run.recipient_audience_hash.as_deref() == Some(claims.recipient_audience_hash.as_str())
        && run.recipient_audience_revision == Some(claims.recipient_audience_revision)
        && run.privacy_policy_ref.as_deref() == Some(claims.privacy_policy_ref.as_str())
        && run.resource_authorization_ref.as_deref()
            == Some(claims.run_context_authorization_ref.as_str())
        && run.authority_revision == Some(claims.authority_revision);
    if !bound {
        return Err(not_authorized());
    }
    if claims.zero_data_retention {
        return Err(zdr_refusal());
    }
    let life = claims.expires_at - claims.issued_at;
    let fresh = claims.expires_at > now
        && claims.issued_at <= now + Duration::minutes(DECISION_SKEW_MINUTES)
        && life > Duration::zero()
        && life <= Duration::minutes(DECISION_MAX_LIFE_MINUTES);
    let complete = [
        claims.purpose.as_str(),
        claims.lawful_basis.as_str(),
        claims.privacy_class.as_str(),
        claims.retention_class.as_str(),
        claims.residency.as_str(),
        claims.deletion_scope.as_str(),
        claims.recipient_audience_ref.as_str(),
        claims.nonce.as_str(),
    ]
    .iter()
    .all(|value| !value.trim().is_empty());
    if !fresh
        || !complete
        || claims.recipient_audience_revision <= 0
        || claims.authority_revision <= 0
    {
        return Err(coded(
            Status::permission_denied("Control run action decision is expired or incomplete"),
            NOT_AUTHORIZED,
        ));
    }
    Ok(())
}

/// The presentation event the human path writes, with the same keys and no content.
fn presentation_payload(
    thread_id: &str,
    title_overridden: bool,
    pinned: Option<bool>,
) -> serde_json::Value {
    serde_json::json!({
        "thread_id": thread_id,
        "title_overridden": title_overridden,
        "preview_overridden": false,
        "pinned": pinned,
        "persona_enabled": null,
        "archived": null,
        "memory_mode": null,
        "starred": null,
        "unread": null,
    })
}

async fn insert_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    now: DateTime<Utc>,
    draft: EventDraft<'_>,
) -> Result<(), Status> {
    let event_id = new_ulid();
    let resource = format!("thread:{}", draft.thread_id);
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id,
                             causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, $2, $3, $4, $5, $6, $7, '', '', $8, $9, $10, 'session-core', 1)",
    )
    .bind(&event_id)
    .bind(draft.event_type)
    .bind(draft.run_id)
    .bind(draft.payload)
    .bind(now)
    .bind(draft.org_id)
    .bind(draft.owner)
    .bind(derive_idempotency_hash(
        "session-core",
        draft.event_type,
        &resource,
        draft.key,
    ))
    .bind(&resource)
    .bind(draft.type_url)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(())
}

/// `space.thread.rename`: allowed while the title is unset or was set by an agent. An identical
/// agent title is a no-op. Empty after whitespace is refused, so an agent cannot clear a title.
async fn apply_rename(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread: &ThreadRow,
    org_id: &str,
    title: &str,
) -> Result<Outcome, Status> {
    if thread.presentation_title.is_some() && thread.title_source != AGENT_SOURCE {
        return Err(coded(
            Status::failed_precondition("the title was set by the owner"),
            TITLE_OWNED_BY_HUMAN,
        ));
    }
    let normalized = normalize_thread_presentation_text(title, "title", THREAD_TITLE_MAX_CHARS)?
        .ok_or_else(|| {
            coded(
                Status::invalid_argument("the title is empty after trimming"),
                INVALID_ARGUMENTS,
            )
        })?;
    if thread.presentation_title.as_deref() == Some(normalized.as_str()) {
        return Ok(Outcome::Noop);
    }
    sqlx::query(
        "UPDATE threads SET presentation_title = $2, title_source = 'agent'
         WHERE id = $1 AND org_id = $3",
    )
    .bind(&thread.thread_id)
    .bind(&normalized)
    .bind(org_id)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(Outcome::Applied)
}

/// `space.thread.pin`: an agent pins an unpinned thread, and unpins only a pin the agent set. A
/// pin a person set stands, and asking for the state it already has is a no-op.
async fn apply_pin(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread: &ThreadRow,
    org_id: &str,
    pinned: bool,
    now: DateTime<Utc>,
) -> Result<Outcome, Status> {
    let next: Option<DateTime<Utc>> = match (pinned, thread.pinned_at.is_some()) {
        (true, false) => Some(now),
        (false, true) if thread.pinned_source == AGENT_SOURCE => None,
        _ => return Ok(Outcome::Noop),
    };
    sqlx::query(
        "UPDATE threads SET pinned_at = $2, pinned_source = 'agent'
         WHERE id = $1 AND org_id = $3",
    )
    .bind(&thread.thread_id)
    .bind(next)
    .bind(org_id)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(Outcome::Applied)
}

/// Applies one registry action to the run's own thread under a verified Control decision.
pub(crate) async fn apply_run_thread_action(
    pool: &PgPool,
    enabled: bool,
    caller: &VerifiedIdentity,
    req: pb::ApplyRunThreadActionRequest,
) -> Result<pb::ApplyRunThreadActionResponse, Status> {
    // Step 1: a ZDR credential is refused here; a service caller needs the scope.
    authorize_operation(caller, APPLY_SCOPE)?;
    // Step 2: the rollout switch.
    if !enabled {
        return Err(coded(
            Status::failed_precondition(FEATURE_DISABLED),
            FEATURE_DISABLED,
        ));
    }
    let pb::ApplyRunThreadActionRequest {
        org_id,
        run_id,
        action_id,
        idempotency_key,
        control_decision_token,
        args,
    } = req;
    caller.authorize_org(&org_id)?;
    let owner = run_owner(caller)?;
    bounded_id(&run_id, MAX_ID_BYTES, "run_id")?;
    bounded_id(&action_id, MAX_ID_BYTES, "action_id")?;
    bounded_id(&idempotency_key, MAX_IDEMPOTENCY_KEY_BYTES, "idempotency_key")?;

    // Step 3 and 4: the action is a row this service serves, its arguments match the golden schema,
    // and the digest is recomputed from them.
    let registry = golden()?;
    let row = registry
        .row(&action_id)
        .filter(|row| row.service_audience == SESSION_CORE_AUDIENCE)
        .ok_or_else(|| {
            coded(
                Status::failed_precondition("the action is not offered by this service"),
                NOT_OFFERED,
            )
        })?;
    let expected = expected_arguments(&action_id).ok_or_else(|| {
        coded(
            Status::failed_precondition("the action is not offered by this service"),
            NOT_OFFERED,
        )
    })?;
    if !row
        .argument_order
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        return Err(Status::internal(
            "the registry argument order differs from this build",
        ));
    }
    let arguments = parse_arguments(&action_id, args)?;
    let digest = argument_digest(&action_id, &run_id, &org_id, &idempotency_key, &arguments)?;
    let keys = configured_control_space_decision_keys()?;

    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(db_error)?;

    // Step 5: the run, its state and its owner.
    let run = load_run(&mut tx, &run_id, &org_id, &owner).await?;
    check_run_is_active(&run)?;

    // Step 6: the Control decision binds this run, action, key and digest, and nothing else.
    let claims = verify_decision(&control_decision_token, &keys)?;
    let asked = Asked {
        org_id: &org_id,
        run_id: &run_id,
        action_id: &action_id,
        idempotency_key: &idempotency_key,
        digest: &digest,
        owner: &owner,
    };
    check_decision_binding(&claims, &asked, &run, row, now)?;

    // Step 7: a removed member is refused while a decision they hold is still in its life.
    let (revoked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations
         WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(claims.space_ref.as_str())
    .bind(&owner)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    if revoked {
        return Err(coded(
            Status::permission_denied("Space membership required for this run"),
            NOT_AUTHORIZED,
        ));
    }

    // Step 8: lock the thread, then replay or conflict on the key, then cap the run's use of it.
    let thread: ThreadRow = sqlx::query_as(
        "SELECT id AS thread_id, presentation_title, pinned_at, title_source, pinned_source
         FROM threads WHERE id = $1 AND org_id = $2 AND archived_at IS NULL FOR UPDATE",
    )
    .bind(&run.thread_id)
    .bind(&org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?
    .ok_or_else(|| Status::not_found("thread not found or archived"))?;

    let earlier: Option<ReceiptRow> = sqlx::query_as(
        "SELECT receipt_id, payload_digest, thread_id, message_id
         FROM run_thread_action_receipts
         WHERE org_id = $1 AND run_id = $2 AND idempotency_key = $3",
    )
    .bind(&org_id)
    .bind(&run_id)
    .bind(&idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    if let Some(receipt) = earlier {
        if receipt.payload_digest != digest {
            return Err(coded(
                Status::already_exists("the idempotency key was used for another action"),
                IDEMPOTENCY_KEY_REUSED,
            ));
        }
        tx.commit().await.map_err(db_error)?;
        return Ok(pb::ApplyRunThreadActionResponse {
            receipt_id: receipt.receipt_id,
            outcome: Outcome::Replayed.wire(),
            thread_id: receipt.thread_id,
            message_id: receipt.message_id.unwrap_or_default(),
        });
    }

    let (used,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM run_thread_action_receipts
         WHERE org_id = $1 AND run_id = $2 AND action_id = $3",
    )
    .bind(&org_id)
    .bind(&run_id)
    .bind(&action_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    if used >= i64::from(row.max_per_run) {
        return Err(coded(
            Status::resource_exhausted("the run has used this action's limit"),
            LIMIT_REACHED,
        ));
    }

    // Step 9: the effect, the receipt and the events in one transaction.
    let receipt_id = new_ulid();
    let outcome = match &arguments {
        Arguments::Rename { title } => apply_rename(&mut tx, &thread, &org_id, title).await?,
        Arguments::Pin { pinned } => apply_pin(&mut tx, &thread, &org_id, *pinned, now).await?,
        Arguments::PostCard { .. } => {
            return Err(coded(
                Status::unimplemented("space.thread.post_card is not built in this service yet"),
                NOT_OFFERED,
            ));
        }
    };
    if outcome == Outcome::Applied {
        let (title_overridden, pinned) = match &arguments {
            Arguments::Rename { .. } => (true, None),
            Arguments::Pin { pinned } => (false, Some(*pinned)),
            Arguments::PostCard { .. } => (false, None),
        };
        insert_event(
            &mut tx,
            now,
            EventDraft {
                event_type: "THREAD_PRESENTATION_UPDATED",
                run_id: &thread.thread_id,
                thread_id: &thread.thread_id,
                org_id: &org_id,
                owner: &owner,
                key: &receipt_id,
                type_url: THREAD_PRESENTATION_UPDATED_TYPE_URL,
                payload: presentation_payload(&thread.thread_id, title_overridden, pinned),
            },
        )
        .await?;
    }

    let receipt_word = outcome.receipt_word();
    sqlx::query(
        "INSERT INTO run_thread_action_receipts
            (org_id, run_id, idempotency_key, receipt_id, action_id, payload_digest,
             decision_ref, subject_id, thread_id, outcome, message_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NULL)",
    )
    .bind(&org_id)
    .bind(&run_id)
    .bind(&idempotency_key)
    .bind(&receipt_id)
    .bind(&action_id)
    .bind(&digest)
    .bind(&claims.decision_ref)
    .bind(&owner)
    .bind(&thread.thread_id)
    .bind(receipt_word)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;

    insert_event(
        &mut tx,
        now,
        EventDraft {
            event_type: RUN_THREAD_ACTION_EVENT,
            run_id: &run_id,
            thread_id: &thread.thread_id,
            org_id: &org_id,
            owner: &owner,
            key: &receipt_id,
            type_url: RUN_THREAD_ACTION_TYPE_URL,
            payload: serde_json::json!({
                "thread_id": thread.thread_id,
                "run_id": run_id,
                "action_id": action_id,
                "receipt_id": receipt_id,
                "outcome": receipt_word,
            }),
        },
    )
    .await?;

    tx.commit().await.map_err(db_error)?;
    Ok(pb::ApplyRunThreadActionResponse {
        receipt_id,
        outcome: outcome.wire(),
        thread_id: thread.thread_id,
        message_id: String::new(),
    })
}

/// `ProbeRunThreadActions`: only execution-core, only this scope. Reports the three booleans.
fn probe(caller: &VerifiedIdentity, enabled: bool) -> Result<pb::ProbeRunThreadActionsResponse, Status> {
    if !caller.is_service() || caller.principal_id() != PROBE_CALLER {
        return Err(Status::permission_denied(
            "only execution-core may probe run thread actions",
        ));
    }
    caller.require_service_scope(PROBE_SCOPE)?;
    let schema_present = sa::registry().is_ok_and(|registry| {
        [RENAME_ACTION, PIN_ACTION, POST_CARD_ACTION]
            .iter()
            .all(|id| registry.row(id).is_some())
    });
    Ok(pb::ProbeRunThreadActionsResponse {
        verifier_configured: configured_control_space_decision_keys().is_ok(),
        schema_present,
        flag_on: enabled,
    })
}

/// The handler state: a pool handle and the switch, read once at boot (a rollout control).
#[derive(Clone)]
pub(crate) struct SpaceRunActionsGrpc {
    pool: PgPool,
    enabled: bool,
}

impl SpaceRunActionsGrpc {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self {
            pool,
            enabled: sa::session_core_enabled(),
        }
    }
}

impl SpaceRunActions for SpaceRunActionsGrpc {
    async fn apply_run_thread_action(
        &self,
        request: Request<pb::ApplyRunThreadActionRequest>,
    ) -> Result<Response<pb::ApplyRunThreadActionResponse>, Status> {
        let started = Instant::now();
        let caller = identity(&request)?;
        let result =
            apply_run_thread_action(&self.pool, self.enabled, &caller, request.into_inner()).await;
        record_metrics("apply_run_thread_action", started, result.is_ok());
        result.map(Response::new)
    }

    async fn probe_run_thread_actions(
        &self,
        request: Request<pb::ProbeRunThreadActionsRequest>,
    ) -> Result<Response<pb::ProbeRunThreadActionsResponse>, Status> {
        let caller = identity(&request)?;
        probe(&caller, self.enabled).map(Response::new)
    }
}
