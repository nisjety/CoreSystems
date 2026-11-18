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
//!    `space.thread.post_card` (WS18-S7) inserts one plain-text assistant row that carries the run id in
//!    `card_run_id`, so the run's final answer still finds its reply slot open (`grpc.rs`). The row's
//!    typed card goes under `metadata.entry`, its body is the content, and nothing is extracted from it.
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
use crate::space::entry::{self as card_entry, CARD_TONES, ENTRY_METADATA_KEY};
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

/// Validates a card against the golden `space.thread.post_card` schema, and its text as plain text
/// (WS18-S7): no line break in a title, label or value, no control character, no link. The SPA draws
/// the text as text, so a card cannot carry a link it would have to render.
fn parse_post_card(card: pb::PostCardArgs) -> Result<Arguments, Status> {
    let invalid = |detail: &'static str| coded(Status::invalid_argument(detail), INVALID_ARGUMENTS);
    if !card_entry::plain_card_text(&card.title, 1, card_entry::CARD_TITLE_MAX_CHARS, false) {
        return Err(invalid("title must be 1 to 120 plain characters: no line break, link or control character"));
    }
    if !card_entry::plain_card_text(&card.body, 1, card_entry::CARD_BODY_MAX_CHARS, true) {
        return Err(invalid("body must be 1 to 2000 plain characters: no link or control character"));
    }
    if !CARD_TONES.contains(&card.tone.as_str()) {
        return Err(invalid("tone must be one of info, progress, done, blocked"));
    }
    if card.fields.len() > card_entry::CARD_FIELDS_MAX {
        return Err(invalid("at most 8 fields"));
    }
    let mut fields = Vec::with_capacity(card.fields.len());
    for field in card.fields {
        if !card_entry::plain_card_text(&field.label, 1, card_entry::CARD_FIELD_LABEL_MAX_CHARS, false)
            || !card_entry::plain_card_text(&field.value, 1, card_entry::CARD_FIELD_VALUE_MAX_CHARS, false)
        {
            return Err(invalid("a field label is 1 to 40 plain characters and its value 1 to 200"));
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

/// `space.thread.post_card` (WS18-S7): one plain-text assistant row in the run's own thread, written
/// under the thread lock. The row takes the thread's own audience and authority stamps, `post_kind`
/// `turn`, and `card_run_id` = the run, which is what keeps the run's final answer's reply slot open
/// (`grpc.rs`). The typed card goes under `metadata.entry`; the body is the content. The card is not
/// appended through `append_message_in_tx_as`: it takes no human authority, extracts no memory and
/// writes no Letta mirror. Returns the new message id.
async fn apply_post_card(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread: &ThreadRow,
    org_id: &str,
    run_id: &str,
    now: DateTime<Utc>,
    arguments: &Arguments,
) -> Result<String, Status> {
    let Arguments::PostCard {
        title,
        body,
        tone,
        fields,
    } = arguments
    else {
        return Err(Status::internal("the card write takes a card"));
    };
    let message_id = new_ulid();
    let metadata = serde_json::json!({
        ENTRY_METADATA_KEY: card_entry::agent_card_json(title, tone, fields),
    });
    sqlx::query(
        "INSERT INTO messages
            (id, thread_id, role, content, created_at, metadata, space_id, recipient_audience_ref,
             recipient_audience_revision, recipient_audience_hash, authority_revision,
             resource_authorization_ref, post_kind, card_run_id)
         SELECT $1, t.id, 'assistant', $2, $3, $4, t.space_id, t.recipient_audience_ref,
                t.recipient_audience_revision, t.recipient_audience_hash, t.authority_revision,
                t.resource_authorization_ref, 'turn', $5
         FROM threads t WHERE t.id = $6 AND t.org_id = $7",
    )
    .bind(&message_id)
    .bind(body)
    .bind(now)
    .bind(&metadata)
    .bind(run_id)
    .bind(&thread.thread_id)
    .bind(org_id)
    .execute(&mut **tx)
    .await
    .map_err(db_error)
    .and_then(|done| {
        if done.rows_affected() == 1 {
            Ok(())
        } else {
            Err(Status::internal("the card row was not written"))
        }
    })?;
    // The thread's activity clock moves with any agent reply (as `append_message_in_tx_as` stamps it),
    // so the room shows the card as new.
    sqlx::query(
        "UPDATE threads
         SET last_activity_at = GREATEST(last_activity_at, $2),
             last_assistant_at = clock_timestamp()
         WHERE id = $1",
    )
    .bind(&thread.thread_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(message_id)
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

    // Step 9: the effect, the receipt and the events in one transaction. A card's message id is the
    // only id the effect returns; rename and pin return none.
    let receipt_id = new_ulid();
    let (outcome, message_id) = match &arguments {
        Arguments::Rename { title } => (
            apply_rename(&mut tx, &thread, &org_id, title).await?,
            String::new(),
        ),
        Arguments::Pin { pinned } => (
            apply_pin(&mut tx, &thread, &org_id, *pinned, now).await?,
            String::new(),
        ),
        Arguments::PostCard { .. } => (
            Outcome::Applied,
            apply_post_card(&mut tx, &thread, &org_id, &run_id, now, &arguments).await?,
        ),
    };
    // Rename and pin change the thread's presentation, so the human path's presentation event follows
    // them. A card changes no presentation column and emits no presentation event.
    let presentation = match &arguments {
        Arguments::Rename { .. } if outcome == Outcome::Applied => Some((true, None)),
        Arguments::Pin { pinned } if outcome == Outcome::Applied => Some((false, Some(*pinned))),
        _ => None,
    };
    if let Some((title_overridden, pinned)) = presentation {
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
    let receipt_message: Option<String> = (!message_id.is_empty()).then(|| message_id.clone());
    sqlx::query(
        "INSERT INTO run_thread_action_receipts
            (org_id, run_id, idempotency_key, receipt_id, action_id, payload_digest,
             decision_ref, subject_id, thread_id, outcome, message_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
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
    .bind(&receipt_message)
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
                "message_id": receipt_message,
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
        message_id,
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

#[tonic::async_trait]
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

#[cfg(test)]
mod tests {
    //! Unit tests of the decision reader, the claim binding, the argument digest and the refusals that are
    //! answered before any row is read (no database: the pool is lazy and never connects) and, marked
    //! `--ignored`, the real-Postgres proof of the whole call. The DB tests mint their own Control
    //! `run-action-v1` decisions with the test key and clean up after themselves.

    use super::*;
    use crate::grpc::space_w1_tests::{signing_key, StreamKeysEnv, KEY_ID};
    use crate::run_phase::ERROR_CODE_TRAILER;
    use crate::space::test_support::{clean_space, connect, seed_space_run, seed_space_thread};
    use ed25519_dalek::{Signer as _, SigningKey};
    use mp_contracts::zdr_refusal::code as zdr_code;
    use serde_json::{json, Value};

    type Args = pb::apply_run_thread_action_request::Args;

    fn trailer(status: &Status) -> Option<String> {
        status
            .metadata()
            .get(ERROR_CODE_TRAILER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool")
    }

    fn registry_row(action: &str) -> sa::RegistryRow {
        golden()
            .expect("the golden registry loads")
            .row(action)
            .expect("the golden registry names the action")
            .clone()
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-02T10:00:00Z")
            .expect("test time")
            .with_timezone(&Utc)
    }

    /// The 29 claims of a decision for a rename of `thread-1` by `run-1` of `org-1`, issued ten seconds
    /// before `now()` and valid for sixty more.
    fn claims_json() -> Value {
        json!({
            "decision_ref": "decision-1",
            "run_id": "run-1",
            "thread_id": "thread-1",
            "org_id": "org-1",
            "space_ref": "space-1",
            "subject_id": "owner-1",
            "service_audience": SESSION_CORE_AUDIENCE,
            "action_id": RENAME_ACTION,
            "action_schema_hash": registry_row(RENAME_ACTION).schema_sha256,
            "payload_digest": "sha256:digest-1",
            "idempotency_key": "key-1",
            "recipient_audience_ref": "audience-1",
            "recipient_audience_hash": "sha256:audience-1",
            "recipient_audience_revision": 3,
            "privacy_policy_ref": "privacy-1",
            "run_context_authorization_ref": "resource-1",
            "authority_revision": 7,
            "permissions": [SPACE_ACTION_PERMISSION],
            "purpose": "assistant_collaboration",
            "lawful_basis": "contract",
            "privacy_class": "internal",
            "third_party_processing_allowed": false,
            "retention_class": "standard",
            "residency": "swedencentral",
            "deletion_scope": "space",
            "zero_data_retention": false,
            "issued_at": (now() - Duration::seconds(10)).to_rfc3339(),
            "expires_at": (now() + Duration::seconds(60)).to_rfc3339(),
            "nonce": "nonce-1",
        })
    }

    /// Signs `payload` as Control signs a decision: the signature covers the first three segments.
    fn sign_raw(key: &SigningKey, kid: &str, payload: &[u8]) -> String {
        let signing_input = format!(
            "{DECISION_VERSION}.{}.{}",
            URL_SAFE_NO_PAD.encode(kid.as_bytes()),
            URL_SAFE_NO_PAD.encode(payload)
        );
        format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(key.sign(signing_input.as_bytes()).to_bytes())
        )
    }

    fn sign_with(key: &SigningKey, kid: &str, claims: &Value) -> String {
        sign_raw(key, kid, &serde_json::to_vec(claims).expect("claims serialize"))
    }

    // ---- verify_decision ---------------------------------------------------------------------

    #[test]
    fn verify_decision_refuses_a_bad_envelope_key_signature_size_and_any_claim_outside_the_29() {
        let key = SigningKey::from_bytes(&[7_u8; 32]);
        let other = SigningKey::from_bytes(&[8_u8; 32]);
        let keys: BTreeMap<String, VerifyingKey> =
            BTreeMap::from([("kid-1".to_owned(), key.verifying_key())]);
        let claims = claims_json();
        assert_eq!(claims.as_object().expect("an object").len(), 29, "Control's RunActionDecision has 29 claims");
        let good = sign_with(&key, "kid-1", &claims);
        let refused = |token: &str, why: &str| {
            let status = verify_decision(token, &keys).expect_err(why);
            assert_eq!(status.code(), tonic::Code::PermissionDenied, "{why}");
            assert_eq!(trailer(&status).as_deref(), Some("not_authorized"), "{why}");
        };

        // The control: a well-formed decision verifies and carries what it was signed with.
        let verified = verify_decision(&good, &keys).expect("a well-formed decision verifies");
        assert_eq!(
            (verified.run_id.as_str(), verified.org_id.as_str(), verified.action_id.as_str()),
            ("run-1", "org-1", RENAME_ACTION)
        );
        assert_eq!(verified.permissions, [SPACE_ACTION_PERMISSION]);
        assert_eq!(verified.recipient_audience_revision, 3);
        assert!(!verified.zero_data_retention);

        // A bad envelope: another version, too few or too many segments, nothing at all.
        refused(&good.replacen(DECISION_VERSION, "run-action-v2", 1), "another version");
        refused(&good.replacen(DECISION_VERSION, "v2", 1), "the read decision's version");
        refused(&format!("{good}.extra"), "five segments");
        refused(good.rsplit_once('.').expect("a signature").0, "three segments");
        refused("", "an empty token");
        refused("a.b.c.d", "four segments that are not a decision");

        // A missing or unknown key id.
        let signed_for = |kid: &str| sign_with(&key, kid, &claims);
        refused(&signed_for(""), "an empty key id");
        refused(&signed_for("  "), "a blank key id");
        refused(&signed_for("kid-unknown"), "a key id that is not configured");
        refused(&good.replacen(&URL_SAFE_NO_PAD.encode("kid-1"), "!!!!", 1), "a key id that is not base64");

        // A bad signature: another key's, a tampered payload under the old signature, no signature at all.
        refused(&sign_with(&other, "kid-1", &claims), "another key's signature");
        let parts: Vec<&str> = good.split('.').collect();
        let mut tampered = claims.clone();
        tampered["run_id"] = json!("run-2");
        let tampered_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&tampered).expect("claims"));
        refused(
            &format!("{}.{}.{}.{}", parts[0], parts[1], tampered_payload, parts[3]),
            "a payload changed after it was signed",
        );
        refused(&format!("{}.{}.{}.", parts[0], parts[1], parts[2]), "an empty signature");
        refused(&format!("{}.{}.{}.!!!!", parts[0], parts[1], parts[2]), "a signature that is not base64");
        refused(
            &format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], URL_SAFE_NO_PAD.encode([1_u8; 32])),
            "a signature of the wrong length",
        );

        // A signed payload that is not a decision.
        refused(&sign_raw(&key, "kid-1", b"not json"), "a payload that is not JSON");
        refused(&sign_raw(&key, "kid-1", b"[1, 2]"), "a payload that is not an object");

        // A token over 16 KiB is refused on its size alone: the same claims with a short nonce verify.
        let mut huge = claims.clone();
        huge["nonce"] = json!("n".repeat(MAX_DECISION_TOKEN_BYTES));
        let oversized = sign_with(&key, "kid-1", &huge);
        assert!(oversized.len() > MAX_DECISION_TOKEN_BYTES);
        refused(&oversized, "a token over 16 KiB");

        // deny_unknown_fields: a 30th claim, even a real one of the read decision, is refused...
        for extra in ["history_floor_revision", "membership_revision", "unknown_claim"] {
            let mut widened = claims.clone();
            widened[extra] = json!(1);
            refused(&sign_with(&key, "kid-1", &widened), extra);
        }
        // ...and so is a decision that lacks any one of the 29.
        for name in claims.as_object().expect("an object").keys() {
            let mut narrowed = claims.clone();
            narrowed.as_object_mut().expect("an object").remove(name);
            refused(&sign_with(&key, "kid-1", &narrowed), name);
        }
    }

    // ---- check_decision_binding --------------------------------------------------------------

    /// A decision, the call it must bind, the run row and the registry row, all agreeing.
    struct Fixture {
        claims: Value,
        run: RunRow,
        row: sa::RegistryRow,
        org: String,
        run_id: String,
        action: String,
        key: String,
        digest: String,
        owner: String,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                claims: claims_json(),
                run: RunRow {
                    run_id: "run-1".into(),
                    org_id: "org-1".into(),
                    subject_id: "owner-1".into(),
                    thread_id: "thread-1".into(),
                    run_status: "queued".into(),
                    retention_class: "persistent".into(),
                    parent_run_id: None,
                    space_id: Some("space-1".into()),
                    recipient_audience_ref: Some("audience-1".into()),
                    recipient_audience_revision: Some(3),
                    recipient_audience_hash: Some("sha256:audience-1".into()),
                    privacy_policy_ref: Some("privacy-1".into()),
                    resource_authorization_ref: Some("resource-1".into()),
                    authority_revision: Some(7),
                },
                row: registry_row(RENAME_ACTION),
                org: "org-1".into(),
                run_id: "run-1".into(),
                action: RENAME_ACTION.into(),
                key: "key-1".into(),
                digest: "sha256:digest-1".into(),
                owner: "owner-1".into(),
            }
        }

        fn verdict(&self) -> Result<(), Status> {
            let claims: RunActionClaims =
                serde_json::from_value(self.claims.clone()).expect("a decision of the 29 claims");
            let asked = Asked {
                org_id: &self.org,
                run_id: &self.run_id,
                action_id: &self.action,
                idempotency_key: &self.key,
                digest: &self.digest,
                owner: &self.owner,
            };
            check_decision_binding(&claims, &asked, &self.run, &self.row, now())
        }
    }

    fn at(offset_seconds: i64) -> String {
        (now() + Duration::seconds(offset_seconds)).to_rfc3339()
    }

    #[test]
    fn check_decision_binding_accepts_the_matching_decision_and_its_edges() {
        assert!(Fixture::new().verdict().is_ok());
        // Two minutes is the longest life, and a decision issued up to a minute ahead of this clock is tolerated.
        let mut longest = Fixture::new();
        longest.claims["issued_at"] = json!(at(-60));
        longest.claims["expires_at"] = json!(at(60));
        assert!(longest.verdict().is_ok());
        let mut ahead = Fixture::new();
        ahead.claims["issued_at"] = json!(at(60));
        ahead.claims["expires_at"] = json!(at(120));
        assert!(ahead.verdict().is_ok());
    }

    #[test]
    fn check_decision_binding_refuses_one_case_per_claim_mismatch() {
        let cases: &[(&str, fn(&mut Fixture))] = &[
            // The audience, the action and what the registry row says about both.
            ("the audience claim", |f| f.claims["service_audience"] = json!("model-plane")),
            ("the registry row's audience", |f| {
                f.row.service_audience = "application-plane-conversation-core".into();
            }),
            ("the action claim", |f| f.claims["action_id"] = json!(PIN_ACTION)),
            ("the action asked", |f| f.action = PIN_ACTION.into()),
            ("the schema hash claim", |f| f.claims["action_schema_hash"] = json!("sha256:another-schema")),
            // The digest and the key the call binds.
            ("the digest claim", |f| f.claims["payload_digest"] = json!("sha256:another-digest")),
            ("the digest asked", |f| f.digest = "sha256:another-digest".into()),
            ("the idempotency key claim", |f| f.claims["idempotency_key"] = json!("key-2")),
            ("the idempotency key asked", |f| f.key = "key-2".into()),
            // The permission set is exactly the one registry permission.
            ("no permission", |f| f.claims["permissions"] = json!([])),
            ("an extra permission", |f| {
                f.claims["permissions"] = json!([SPACE_ACTION_PERMISSION, "thread:append"]);
            }),
            ("a write permission instead", |f| f.claims["permissions"] = json!(["thread:append"])),
            ("another registry permission", |f| f.claims["permissions"] = json!(["owner-action:execute"])),
            ("the registry row's permission", |f| {
                f.row.permission = "owner-action:execute".into();
            }),
            // The org, the run, the thread, the subject.
            ("the org claim", |f| f.claims["org_id"] = json!("org-2")),
            ("the org asked", |f| f.org = "org-2".into()),
            ("the run row's org", |f| f.run.org_id = "org-2".into()),
            ("the run claim", |f| f.claims["run_id"] = json!("run-2")),
            ("the run asked", |f| f.run_id = "run-2".into()),
            ("the run row's id", |f| f.run.run_id = "run-2".into()),
            ("the thread claim", |f| f.claims["thread_id"] = json!("thread-2")),
            ("the run row's thread", |f| f.run.thread_id = "thread-2".into()),
            ("the subject claim", |f| f.claims["subject_id"] = json!("member-2")),
            ("the owner asked", |f| f.owner = "member-2".into()),
            ("the run row's subject", |f| f.run.subject_id = "member-2".into()),
            // The Space and the audience the run was stamped with.
            ("the Space claim", |f| f.claims["space_ref"] = json!("space-2")),
            ("the run row's Space", |f| f.run.space_id = Some("space-2".into())),
            ("a run row with no Space", |f| f.run.space_id = None),
            ("the recipient ref claim", |f| f.claims["recipient_audience_ref"] = json!("audience-2")),
            ("the run row's recipient ref", |f| f.run.recipient_audience_ref = Some("audience-2".into())),
            ("a run row with no recipient ref", |f| f.run.recipient_audience_ref = None),
            ("the recipient hash claim", |f| f.claims["recipient_audience_hash"] = json!("sha256:audience-2")),
            ("the run row's recipient hash", |f| {
                f.run.recipient_audience_hash = Some("sha256:audience-2".into());
            }),
            ("a run row with no recipient hash", |f| f.run.recipient_audience_hash = None),
            ("the recipient revision claim", |f| f.claims["recipient_audience_revision"] = json!(4)),
            ("the run row's recipient revision", |f| f.run.recipient_audience_revision = Some(4)),
            ("a run row with no recipient revision", |f| f.run.recipient_audience_revision = None),
            ("the privacy ref claim", |f| f.claims["privacy_policy_ref"] = json!("privacy-2")),
            ("the run row's privacy ref", |f| f.run.privacy_policy_ref = Some("privacy-2".into())),
            ("a run row with no privacy ref", |f| f.run.privacy_policy_ref = None),
            ("the run context ref claim", |f| f.claims["run_context_authorization_ref"] = json!("resource-2")),
            ("the run row's run context ref", |f| {
                f.run.resource_authorization_ref = Some("resource-2".into());
            }),
            ("a run row with no run context ref", |f| f.run.resource_authorization_ref = None),
            ("the authority revision claim", |f| f.claims["authority_revision"] = json!(8)),
            ("the run row's authority revision", |f| f.run.authority_revision = Some(8)),
            ("a run row with no authority revision", |f| f.run.authority_revision = None),
        ];
        for (why, edit) in cases {
            let mut fixture = Fixture::new();
            edit(&mut fixture);
            let status = fixture.verdict().expect_err(why);
            assert_eq!(status.code(), tonic::Code::PermissionDenied, "{why}");
            assert_eq!(trailer(&status).as_deref(), Some("not_authorized"), "{why}");
            assert!(
                status.message().contains("does not authorize"),
                "{why} is a binding refusal, not a freshness one: {}",
                status.message()
            );
        }
    }

    #[test]
    fn check_decision_binding_refuses_a_zero_retention_claim_with_the_typed_refusal() {
        let mut fixture = Fixture::new();
        fixture.claims["zero_data_retention"] = json!(true);
        let status = fixture.verdict().expect_err("a zero-retention decision cannot mutate");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            Refusal::from_status(&status).map(|refusal| refusal.code()),
            Some(zdr_code::DURABLE_MUTATION_FORBIDDEN)
        );
        // The ZDR refusal wins over a stale or incomplete decision: it names the real reason.
        fixture.claims["expires_at"] = json!(at(-5));
        fixture.claims["nonce"] = json!("");
        let status = fixture.verdict().expect_err("still refused");
        assert_eq!(
            Refusal::from_status(&status).map(|refusal| refusal.code()),
            Some(zdr_code::DURABLE_MUTATION_FORBIDDEN)
        );
        // A binding mismatch is still the first refusal.
        fixture.claims["org_id"] = json!("org-2");
        assert_eq!(
            trailer(&fixture.verdict().expect_err("a mismatch")).as_deref(),
            Some("not_authorized")
        );
    }

    #[test]
    fn check_decision_binding_refuses_an_expired_long_or_incomplete_decision() {
        let cases: &[(&str, fn(&mut Fixture))] = &[
            ("a decision that expired a second ago", |f| f.claims["expires_at"] = json!(at(-1))),
            ("a decision that expires exactly now", |f| f.claims["expires_at"] = json!(at(0))),
            ("a decision issued over a minute ahead", |f| {
                f.claims["issued_at"] = json!(at(61));
                f.claims["expires_at"] = json!(at(100));
            }),
            ("a decision with no life", |f| {
                f.claims["issued_at"] = json!(at(30));
                f.claims["expires_at"] = json!(at(30));
            }),
            ("a decision that expires before it was issued", |f| {
                f.claims["issued_at"] = json!(at(20));
                f.claims["expires_at"] = json!(at(10));
            }),
            ("a decision that lasts over two minutes", |f| {
                f.claims["issued_at"] = json!(at(-30));
                f.claims["expires_at"] = json!(at(91));
            }),
            ("an empty purpose", |f| f.claims["purpose"] = json!("")),
            ("a blank purpose", |f| f.claims["purpose"] = json!("   ")),
            ("an empty lawful basis", |f| f.claims["lawful_basis"] = json!("")),
            ("an empty privacy class", |f| f.claims["privacy_class"] = json!("")),
            ("an empty retention class", |f| f.claims["retention_class"] = json!("")),
            ("an empty residency", |f| f.claims["residency"] = json!("")),
            ("an empty deletion scope", |f| f.claims["deletion_scope"] = json!("")),
            ("an empty nonce", |f| f.claims["nonce"] = json!("")),
            ("an empty recipient ref (agreed by the run row)", |f| {
                f.claims["recipient_audience_ref"] = json!("");
                f.run.recipient_audience_ref = Some(String::new());
            }),
            ("a zero recipient revision (agreed by the run row)", |f| {
                f.claims["recipient_audience_revision"] = json!(0);
                f.run.recipient_audience_revision = Some(0);
            }),
            ("a negative recipient revision (agreed by the run row)", |f| {
                f.claims["recipient_audience_revision"] = json!(-1);
                f.run.recipient_audience_revision = Some(-1);
            }),
            ("a zero authority revision (agreed by the run row)", |f| {
                f.claims["authority_revision"] = json!(0);
                f.run.authority_revision = Some(0);
            }),
        ];
        for (why, edit) in cases {
            let mut fixture = Fixture::new();
            edit(&mut fixture);
            let status = fixture.verdict().expect_err(why);
            assert_eq!(status.code(), tonic::Code::PermissionDenied, "{why}");
            assert_eq!(trailer(&status).as_deref(), Some("not_authorized"), "{why}");
            assert!(
                status.message().contains("expired or incomplete"),
                "{why} is a freshness refusal, not a binding one: {}",
                status.message()
            );
        }
    }

    // ---- the arguments and their digest ------------------------------------------------------

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    fn card(fields: &[(&str, &str)]) -> Arguments {
        Arguments::PostCard {
            title: "T".to_owned(),
            body: "B".to_owned(),
            tone: "info".to_owned(),
            fields: fields
                .iter()
                .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn argument_pairs_flatten_fields_to_len_label_and_value_and_an_absent_list_to_len_zero() {
        // Rename and pin are one pair each; the values are digested untrimmed.
        assert_eq!(
            argument_pairs(&Arguments::Rename { title: " Weekly  plan ".to_owned() }),
            [pair("title", " Weekly  plan ")]
        );
        assert_eq!(argument_pairs(&Arguments::Pin { pinned: true }), [pair("pinned", "true")]);
        assert_eq!(argument_pairs(&Arguments::Pin { pinned: false }), [pair("pinned", "false")]);

        // A card with no fields is `fields.len` = 0 and nothing after it.
        let none = [pair("title", "T"), pair("body", "B"), pair("tone", "info"), pair("fields.len", "0")];
        assert_eq!(argument_pairs(&card(&[])), none);
        // Fields flatten in place, in order, as `fields.<i>.label` and `fields.<i>.value`.
        assert_eq!(
            argument_pairs(&card(&[("Owner", "Ima"), ("Due", "Friday")])),
            [
                pair("title", "T"),
                pair("body", "B"),
                pair("tone", "info"),
                pair("fields.len", "2"),
                pair("fields.0.label", "Owner"),
                pair("fields.0.value", "Ima"),
                pair("fields.1.label", "Due"),
                pair("fields.1.value", "Friday"),
            ]
        );
        // The wire has no "absent" list apart from the empty one: both reach the same pairs and the same digest.
        let absent = parse_arguments(
            POST_CARD_ACTION,
            Some(Args::PostCard(pb::PostCardArgs {
                title: "T".to_owned(),
                body: "B".to_owned(),
                tone: "info".to_owned(),
                fields: Vec::new(),
            })),
        )
        .expect("a card without fields is valid");
        assert_eq!(argument_pairs(&absent), none);
        let digest_of = |arguments: &Arguments| {
            argument_digest(POST_CARD_ACTION, "run-1", "org-1", "key-1", arguments).expect("the digest frames")
        };
        assert_eq!(digest_of(&absent), digest_of(&card(&[])));
        assert_ne!(digest_of(&card(&[("a", "1")])), digest_of(&card(&[("a", "2")])));
        // A value cannot forge a boundary: moving text from a label to a value changes the digest.
        assert_ne!(digest_of(&card(&[("ab", "c")])), digest_of(&card(&[("a", "bc")])));
    }

    #[test]
    fn the_flattening_follows_the_golden_argument_order_of_every_served_action() {
        for (action, arguments) in [
            (RENAME_ACTION, Arguments::Rename { title: "T".to_owned() }),
            (PIN_ACTION, Arguments::Pin { pinned: true }),
            (POST_CARD_ACTION, card(&[("a", "1"), ("b", "2")])),
        ] {
            let order = registry_row(action).argument_order;
            // The constant this build serves from is the golden's order, or the service refuses to serve.
            let served: Vec<String> = expected_arguments(action)
                .expect("the action is served")
                .iter()
                .map(|name| (*name).to_owned())
                .collect();
            assert_eq!(served, order, "{action}");
            // And the flattened names, with `.len` and `.<i>.<part>` folded back, are that order.
            let mut names: Vec<String> = Vec::new();
            for (name, _) in argument_pairs(&arguments) {
                let base = name.split('.').next().expect("a name").to_owned();
                if names.last() != Some(&base) {
                    names.push(base);
                }
            }
            assert_eq!(names, order, "{action}");
        }
        assert_eq!(expected_arguments("space.canvas.read"), None, "an action of another service");
        assert_eq!(expected_arguments("tickets.create"), None);
    }

    #[test]
    fn the_rename_digest_equals_the_golden_vector() {
        let registry = golden().expect("the golden registry loads");
        let vector = &registry.digest_vectors[0];
        let text = |field: &str| {
            vector[field]
                .as_str()
                .unwrap_or_else(|| panic!("the first vector names {field}"))
                .to_owned()
        };
        // The vector is the one the plan names: space.thread.rename, run-1, org-1, step-1, "Weekly plan".
        assert_eq!(text("action_id"), RENAME_ACTION);
        assert_eq!(text("run_id"), "run-1");
        assert_eq!(text("org_id"), "org-1");
        assert_eq!(text("step_id"), "step-1");
        assert_eq!(vector["argument_pairs"], json!([["title", "Weekly plan"]]));

        let key = sa::idempotency_key(RENAME_ACTION, "run-1", "step-1");
        assert_eq!(key, text("idempotency_key"));
        let rename = |title: &str| Arguments::Rename { title: title.to_owned() };
        let digest = argument_digest(RENAME_ACTION, "run-1", "org-1", &key, &rename("Weekly plan"))
            .expect("the digest frames");
        assert_eq!(digest, text("payload_digest"), "Session Core computes the digest the golden pins");
        // Every input is part of it: the title (untrimmed), the run, the org and the key.
        for other in [
            argument_digest(RENAME_ACTION, "run-1", "org-1", &key, &rename("Weekly plan ")),
            argument_digest(RENAME_ACTION, "run-2", "org-1", &key, &rename("Weekly plan")),
            argument_digest(RENAME_ACTION, "run-1", "org-2", &key, &rename("Weekly plan")),
            argument_digest(RENAME_ACTION, "run-1", "org-1", "another-key", &rename("Weekly plan")),
        ] {
            assert_ne!(other.expect("the digest frames"), digest);
        }
    }

    // ---- refusals answered before any row is read ---------------------------------------------

    fn rename_request(org: &str) -> pb::ApplyRunThreadActionRequest {
        pb::ApplyRunThreadActionRequest {
            org_id: org.to_owned(),
            run_id: "run-1".to_owned(),
            action_id: RENAME_ACTION.to_owned(),
            idempotency_key: "key-1".to_owned(),
            control_decision_token: "not-a-token".to_owned(),
            args: Some(Args::Rename(pb::RenameThreadArgs { title: "Weekly plan".to_owned() })),
        }
    }

    #[tokio::test]
    async fn zdr_caller_refused() {
        let pool = lazy_pool();
        let caller = VerifiedIdentity::user_for_test_with_zdr("org-1", "owner-1", true);
        // The refusal is typed, and it comes first: with the switch off it is still this refusal.
        for enabled in [true, false] {
            let status = apply_run_thread_action(&pool, enabled, &caller, rename_request("org-1"))
                .await
                .expect_err("a zero-retention credential cannot mutate");
            assert_eq!(status.code(), tonic::Code::FailedPrecondition);
            assert_eq!(
                Refusal::from_status(&status).map(|refusal| refusal.code()),
                Some(zdr_code::DURABLE_MUTATION_FORBIDDEN),
                "switch {enabled}"
            );
        }
        assert_eq!(pool.size(), 0, "no row was read");
    }

    #[tokio::test]
    async fn feature_disabled_when_switch_off() {
        let pool = lazy_pool();
        let caller = VerifiedIdentity::user_for_test("org-1", "owner-1");
        let status = apply_run_thread_action(&pool, false, &caller, rename_request("org-1"))
            .await
            .expect_err("the switch is off");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(trailer(&status).as_deref(), Some("feature_disabled"));
        // The switch answers before the decision is looked at: the token above is not even a token.
        assert_eq!(pool.size(), 0, "no row was read");
        // With the switch on, the same call gets past it and is refused for what it is.
        let status = apply_run_thread_action(&pool, true, &caller, rename_request("org-2"))
            .await
            .expect_err("another tenant");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        assert_ne!(trailer(&status).as_deref(), Some("feature_disabled"));
        assert_eq!(pool.size(), 0);
    }

    #[test]
    fn probe_answers_only_execution_core_holding_the_probe_scope() {
        let _keys = StreamKeysEnv::publish();
        let execution_core = VerifiedIdentity::service_for_test_as("org-1", PROBE_CALLER, &[PROBE_SCOPE], false);
        let on = probe(&execution_core, true).expect("execution-core may probe");
        assert!(on.verifier_configured && on.schema_present && on.flag_on);
        let off = probe(&execution_core, false).expect("the probe reports the switch");
        assert!(off.verifier_configured && off.schema_present && !off.flag_on);
        // Nobody else: another service with the scope, the right service without it or with the apply
        // scope only, and a user principal.
        for (why, caller) in [
            (
                "another service",
                VerifiedIdentity::service_for_test_as("org-1", "service:model-gateway", &[PROBE_SCOPE], false),
            ),
            (
                "no scope",
                VerifiedIdentity::service_for_test_as("org-1", PROBE_CALLER, &[], false),
            ),
            (
                "the apply scope only",
                VerifiedIdentity::service_for_test_as("org-1", PROBE_CALLER, &[APPLY_SCOPE], false),
            ),
            ("a user", VerifiedIdentity::user_for_test("org-1", "owner-1")),
        ] {
            let status = probe(&caller, true).expect_err(why);
            assert_eq!(status.code(), tonic::Code::PermissionDenied, "{why}");
        }
    }

    // ---- against a disposable database (`--ignored`) ------------------------------------------

    /// The owner `seed_space_thread` and `seed_space_run` give their rows.
    const OWNER: &str = "owner-1";

    /// A Space thread with one persistent run of its owner, and the test key published as Control's.
    struct Room {
        pool: PgPool,
        org: String,
        space: String,
        thread: String,
        run: String,
        _keys: StreamKeysEnv,
    }

    impl Room {
        async fn new(prefix: &str) -> Option<Self> {
            let pool = connect().await?;
            let suffix = mp_ids::new_ulid();
            let room = Self {
                pool,
                org: format!("org-{prefix}-{suffix}"),
                space: format!("space-{prefix}-{suffix}"),
                thread: format!("thread-{suffix}"),
                run: format!("run-{suffix}"),
                _keys: StreamKeysEnv::publish(),
            };
            seed_space_thread(&room.pool, &room.org, &room.space, &room.thread, OWNER).await;
            seed_space_run(&room.pool, &room.org, &room.space, &room.thread, &room.run, "queued", "persistent").await;
            Some(room)
        }

        /// Another run of the owner in the same thread, so a test can use an action more often than one run may.
        async fn add_run(&self, name: &str) -> String {
            let run = format!("run-{name}-{}", self.thread);
            seed_space_run(&self.pool, &self.org, &self.space, &self.thread, &run, "queued", "persistent").await;
            run
        }

        fn owner(&self) -> VerifiedIdentity {
            VerifiedIdentity::user_for_test(&self.org, OWNER)
        }

        async fn finish(self) {
            sqlx::query("DELETE FROM events WHERE org_id = $1")
                .bind(&self.org)
                .execute(&self.pool)
                .await
                .ok();
            // The receipts go with the thread (their foreign key cascades).
            clean_space(&self.pool, &self.org, &self.space).await;
        }
    }

    fn rename(title: &str) -> Args {
        Args::Rename(pb::RenameThreadArgs { title: title.to_owned() })
    }

    fn pin(pinned: bool) -> Args {
        Args::Pin(pb::PinThreadArgs { pinned })
    }

    fn action_of(args: &Args) -> &'static str {
        match args {
            Args::Rename(_) => RENAME_ACTION,
            Args::Pin(_) => PIN_ACTION,
            Args::PostCard(_) => POST_CARD_ACTION,
        }
    }

    /// The Control decision for `run` applying `action` under `key`, which binds `digest`: valid for 90 seconds.
    fn decision_for(room: &Room, run: &str, action: &str, key: &str, digest: &str) -> Value {
        let mut claims = claims_json();
        claims["decision_ref"] = json!(format!("decision-{key}"));
        claims["run_id"] = json!(run);
        claims["thread_id"] = json!(room.thread);
        claims["org_id"] = json!(room.org);
        claims["space_ref"] = json!(room.space);
        claims["subject_id"] = json!(OWNER);
        claims["action_id"] = json!(action);
        claims["action_schema_hash"] = json!(registry_row(action).schema_sha256);
        claims["payload_digest"] = json!(digest);
        claims["idempotency_key"] = json!(key);
        // The audience context `seed_space_thread` and `seed_space_run` stamp.
        claims["recipient_audience_ref"] = json!("audience-1");
        claims["recipient_audience_hash"] = json!("sha256:audience-1");
        claims["recipient_audience_revision"] = json!(1);
        claims["privacy_policy_ref"] = json!("privacy-1");
        claims["run_context_authorization_ref"] = json!("resource-1");
        claims["authority_revision"] = json!(1);
        claims["issued_at"] = json!(Utc::now().to_rfc3339());
        claims["expires_at"] = json!((Utc::now() + Duration::seconds(90)).to_rfc3339());
        claims
    }

    /// One call of `caller` for `run`, with a decision minted for exactly these arguments and `step`'s key.
    async fn apply_as(
        room: &Room,
        run: &str,
        caller: &VerifiedIdentity,
        step: &str,
        args: Args,
    ) -> Result<pb::ApplyRunThreadActionResponse, Status> {
        let action = action_of(&args);
        let key = sa::idempotency_key(action, run, step);
        let arguments = parse_arguments(action, Some(args.clone()))?;
        let digest = argument_digest(action, run, &room.org, &key, &arguments)?;
        let token = sign_with(&signing_key(), KEY_ID, &decision_for(room, run, action, &key, &digest));
        apply_run_thread_action(
            &room.pool,
            true,
            caller,
            pb::ApplyRunThreadActionRequest {
                org_id: room.org.clone(),
                run_id: run.to_owned(),
                action_id: action.to_owned(),
                idempotency_key: key,
                control_decision_token: token,
                args: Some(args),
            },
        )
        .await
    }

    #[derive(Debug, PartialEq)]
    struct Presentation {
        title: Option<String>,
        title_source: String,
        pinned: bool,
        pinned_source: String,
    }

    async fn presentation(room: &Room) -> Presentation {
        let (title, title_source, pinned, pinned_source): (Option<String>, String, bool, String) = sqlx::query_as(
            "SELECT presentation_title, title_source, pinned_at IS NOT NULL, pinned_source FROM threads WHERE id = $1",
        )
        .bind(&room.thread)
        .fetch_one(&room.pool)
        .await
        .expect("read the thread");
        Presentation { title, title_source, pinned, pinned_source }
    }

    /// `(action, outcome)` of the receipts of `run`, oldest first.
    async fn receipts(room: &Room, run: &str) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT action_id, outcome FROM run_thread_action_receipts WHERE org_id = $1 AND run_id = $2
             ORDER BY created_at, receipt_id",
        )
        .bind(&room.org)
        .bind(run)
        .fetch_all(&room.pool)
        .await
        .expect("read receipts")
    }

    /// `(event_type, payload)` of the events written for the thread, oldest first.
    async fn thread_events(room: &Room) -> Vec<(String, Value)> {
        sqlx::query_as(
            "SELECT event_type, payload FROM events WHERE org_id = $1 AND resource_ref = $2 ORDER BY ts, id",
        )
        .bind(&room.org)
        .bind(format!("thread:{}", room.thread))
        .fetch_all(&room.pool)
        .await
        .expect("read events")
    }

    fn count_of(events: &[(String, Value)], event_type: &str) -> usize {
        events.iter().filter(|(kind, _)| kind == event_type).count()
    }

    fn applied() -> i32 {
        pb::RunActionOutcome::Applied as i32
    }

    fn noop() -> i32 {
        pb::RunActionOutcome::Noop as i32
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn rename_refuses_human_title() {
        let Some(room) = Room::new("human").await else { return };
        sqlx::query("UPDATE threads SET presentation_title = 'Chosen by a person' WHERE id = $1")
            .bind(&room.thread)
            .execute(&room.pool)
            .await
            .expect("a person titles the thread");
        let status = apply_as(&room, &room.run, &room.owner(), "step-1", rename("Agent title"))
            .await
            .expect_err("a person's title stands");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(trailer(&status).as_deref(), Some("title_owned_by_human"));
        assert_eq!(
            presentation(&room).await,
            Presentation {
                title: Some("Chosen by a person".to_owned()),
                title_source: "owner".to_owned(),
                pinned: false,
                pinned_source: "owner".to_owned(),
            }
        );
        assert!(receipts(&room, &room.run).await.is_empty(), "a refused call leaves no receipt");
        assert!(thread_events(&room).await.is_empty(), "and no event");
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn rename_overwrites_own_agent_title() {
        let Some(room) = Room::new("agent").await else { return };
        let first = apply_as(&room, &room.run, &room.owner(), "step-1", rename("First title"))
            .await
            .expect("an unset title may be set by the agent");
        assert_eq!(first.outcome, applied());
        assert_eq!(first.thread_id, room.thread);
        assert_eq!(first.message_id, "", "a rename writes no message");
        assert!(!first.receipt_id.is_empty());
        let after_first = presentation(&room).await;
        assert_eq!(after_first.title.as_deref(), Some("First title"));
        assert_eq!(after_first.title_source, "agent");

        let second = apply_as(&room, &room.run, &room.owner(), "step-2", rename("Second title"))
            .await
            .expect("the agent may overwrite its own title");
        assert_eq!(second.outcome, applied());
        assert_ne!(second.receipt_id, first.receipt_id);
        let after_second = presentation(&room).await;
        assert_eq!(after_second.title.as_deref(), Some("Second title"));
        assert_eq!(after_second.title_source, "agent");
        assert_eq!(
            receipts(&room, &room.run).await,
            [
                (RENAME_ACTION.to_owned(), "applied".to_owned()),
                (RENAME_ACTION.to_owned(), "applied".to_owned())
            ]
        );

        // An agent cannot clear a title: blank after trimming is refused and nothing is written.
        let blank_run = room.add_run("blank").await;
        let status = apply_as(&room, &blank_run, &room.owner(), "step-1", rename("   "))
            .await
            .expect_err("a blank title");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert_eq!(trailer(&status).as_deref(), Some("invalid_arguments"));
        assert_eq!(presentation(&room).await.title.as_deref(), Some("Second title"));
        assert!(receipts(&room, &blank_run).await.is_empty());
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn rename_identical_title_is_noop() {
        let Some(room) = Room::new("same").await else { return };
        let first = apply_as(&room, &room.run, &room.owner(), "step-1", rename("Same title"))
            .await
            .expect("the first rename applies");
        assert_eq!(first.outcome, applied());
        // The same title, however it is spaced, is already there: a receipt records the call, nothing changes.
        let second = apply_as(&room, &room.run, &room.owner(), "step-2", rename("  Same   title "))
            .await
            .expect("an identical title is not an error");
        assert_eq!(second.outcome, noop());
        assert_ne!(second.receipt_id, first.receipt_id);
        assert_eq!(presentation(&room).await.title.as_deref(), Some("Same title"));
        assert_eq!(
            receipts(&room, &room.run).await,
            [
                (RENAME_ACTION.to_owned(), "applied".to_owned()),
                (RENAME_ACTION.to_owned(), "noop".to_owned())
            ],
            "only `applied` and `noop` are ever stored"
        );
        let events = thread_events(&room).await;
        assert_eq!(count_of(&events, "THREAD_PRESENTATION_UPDATED"), 1, "a no-op changes no presentation");
        assert_eq!(count_of(&events, RUN_THREAD_ACTION_EVENT), 2, "but both calls are recorded");
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn pin_unpin_rules() {
        let Some(room) = Room::new("pin").await else { return };
        // Run A: the agent pins an unpinned thread, then unpins its own pin.
        let pinned = apply_as(&room, &room.run, &room.owner(), "step-1", pin(true))
            .await
            .expect("pin");
        assert_eq!(pinned.outcome, applied());
        let state = presentation(&room).await;
        assert!(state.pinned);
        assert_eq!(state.pinned_source, "agent");
        let unpinned = apply_as(&room, &room.run, &room.owner(), "step-2", pin(false))
            .await
            .expect("the agent may unpin its own pin");
        assert_eq!(unpinned.outcome, applied());
        let state = presentation(&room).await;
        assert!(!state.pinned);
        assert_eq!(state.pinned_source, "agent");

        // Run B: unpinning what is not pinned is a no-op; a pin a person set is not the agent's to remove.
        let run_b = room.add_run("b").await;
        let nothing = apply_as(&room, &run_b, &room.owner(), "step-1", pin(false))
            .await
            .expect("unpin of an unpinned thread");
        assert_eq!(nothing.outcome, noop());
        sqlx::query("UPDATE threads SET pinned_at = now(), pinned_source = 'owner' WHERE id = $1")
            .bind(&room.thread)
            .execute(&room.pool)
            .await
            .expect("a person pins the thread");
        let stands = apply_as(&room, &run_b, &room.owner(), "step-2", pin(false))
            .await
            .expect("the person's pin stands");
        assert_eq!(stands.outcome, noop());
        let state = presentation(&room).await;
        assert!(state.pinned, "the agent did not unpin it");
        assert_eq!(state.pinned_source, "owner");

        // Run C: pinning what is already pinned is a no-op and does not take the pin over.
        let run_c = room.add_run("c").await;
        let already = apply_as(&room, &run_c, &room.owner(), "step-1", pin(true))
            .await
            .expect("pin of a pinned thread");
        assert_eq!(already.outcome, noop());
        assert_eq!(presentation(&room).await.pinned_source, "owner");

        // Only the two changes of run A moved the presentation: pinned true, then false.
        let presentation_events: Vec<Value> = thread_events(&room)
            .await
            .into_iter()
            .filter(|(kind, _)| kind == "THREAD_PRESENTATION_UPDATED")
            .map(|(_, payload)| payload["pinned"].clone())
            .collect();
        assert_eq!(presentation_events, [json!(true), json!(false)]);
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn replay_returns_receipt_and_conflict_on_changed_digest() {
        let Some(room) = Room::new("replay").await else { return };
        let first = apply_as(&room, &room.run, &room.owner(), "step-1", rename("Replay title"))
            .await
            .expect("the first call applies");
        assert_eq!(first.outcome, applied());

        // The same key and digest again (with a fresh decision): the receipt comes back, nothing is written twice.
        let replay = apply_as(&room, &room.run, &room.owner(), "step-1", rename("Replay title"))
            .await
            .expect("a replay is served");
        assert_eq!(replay.outcome, pb::RunActionOutcome::Replayed as i32);
        assert_eq!(replay.receipt_id, first.receipt_id);
        assert_eq!(replay.thread_id, room.thread);
        assert_eq!(receipts(&room, &room.run).await.len(), 1);
        let events = thread_events(&room).await;
        assert_eq!(count_of(&events, RUN_THREAD_ACTION_EVENT), 1);
        assert_eq!(count_of(&events, "THREAD_PRESENTATION_UPDATED"), 1);

        // The same key with other arguments is not a replay: it conflicts, and the thread is untouched.
        let status = apply_as(&room, &room.run, &room.owner(), "step-1", rename("Another title"))
            .await
            .expect_err("a changed digest under the same key");
        assert_eq!(status.code(), tonic::Code::AlreadyExists);
        assert_eq!(trailer(&status).as_deref(), Some("idempotency_key_reused"));
        assert_eq!(presentation(&room).await.title.as_deref(), Some("Replay title"));
        assert_eq!(receipts(&room, &room.run).await.len(), 1);
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn per_run_cap() {
        let Some(room) = Room::new("cap").await else { return };
        // The golden allows two renames per run.
        assert_eq!(registry_row(RENAME_ACTION).max_per_run, 2);
        for (step, title) in [("step-1", "A"), ("step-2", "B")] {
            let done = apply_as(&room, &room.run, &room.owner(), step, rename(title)).await.expect("within the cap");
            assert_eq!(done.outcome, applied());
        }
        let status = apply_as(&room, &room.run, &room.owner(), "step-3", rename("C"))
            .await
            .expect_err("a third rename is over the cap");
        assert_eq!(status.code(), tonic::Code::ResourceExhausted);
        assert_eq!(trailer(&status).as_deref(), Some("limit_reached"));
        assert_eq!(presentation(&room).await.title.as_deref(), Some("B"));
        assert_eq!(receipts(&room, &room.run).await.len(), 2);

        // A replay of an earlier call is still served at the cap; the cap counts what a run applied.
        let replay = apply_as(&room, &room.run, &room.owner(), "step-1", rename("A"))
            .await
            .expect("a replay is not a new application");
        assert_eq!(replay.outcome, pb::RunActionOutcome::Replayed as i32);
        // The cap is per run: another run of the owner has its own.
        let other = room.add_run("other").await;
        let fresh = apply_as(&room, &other, &room.owner(), "step-1", rename("D"))
            .await
            .expect("another run's cap");
        assert_eq!(fresh.outcome, applied());
        // And per action: this run may still pin.
        let pinned = apply_as(&room, &room.run, &room.owner(), "step-4", pin(true)).await.expect("pin");
        assert_eq!(pinned.outcome, applied());
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn receipts_cascade_on_thread_delete() {
        let Some(room) = Room::new("cascade").await else { return };
        apply_as(&room, &room.run, &room.owner(), "step-1", rename("Doomed"))
            .await
            .expect("a rename");
        apply_as(&room, &room.run, &room.owner(), "step-2", pin(true))
            .await
            .expect("a pin");
        let held: i64 = sqlx::query_scalar("SELECT count(*) FROM run_thread_action_receipts WHERE org_id = $1")
            .bind(&room.org)
            .fetch_one(&room.pool)
            .await
            .expect("count receipts");
        assert_eq!(held, 2);
        // The thread goes (its runs first: they reference it), and its receipts go with it.
        sqlx::query("DELETE FROM runs WHERE thread_id = $1")
            .bind(&room.thread)
            .execute(&room.pool)
            .await
            .expect("delete the runs");
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&room.thread)
            .execute(&room.pool)
            .await
            .expect("delete the thread");
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM run_thread_action_receipts WHERE org_id = $1")
            .bind(&room.org)
            .fetch_one(&room.pool)
            .await
            .expect("count receipts");
        assert_eq!(left, 0, "the receipts' foreign key cascades");
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn zdr_run_refused() {
        let Some(room) = Room::new("zdrrun").await else { return };
        // A run that is zero-retention, or held under a waiver, cannot mutate the thread, whatever its decision says.
        for class in ["zdr", "zdr_waived"] {
            let run = format!("run-{class}-{}", room.thread);
            seed_space_run(&room.pool, &room.org, &room.space, &room.thread, &run, "queued", class).await;
            let status = apply_as(&room, &run, &room.owner(), "step-1", rename("Never written"))
                .await
                .expect_err(class);
            assert_eq!(status.code(), tonic::Code::FailedPrecondition, "{class}");
            assert_eq!(
                Refusal::from_status(&status).map(|refusal| refusal.code()),
                Some(zdr_code::DURABLE_MUTATION_FORBIDDEN),
                "{class}"
            );
            assert!(receipts(&room, &run).await.is_empty(), "{class}");
        }
        assert_eq!(presentation(&room).await.title, None);
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn non_owner_run_is_not_found() {
        let Some(room) = Room::new("owner").await else { return };
        // Another member of the same org calls with the owner's run id: the owner predicate is in the lookup,
        // so the run is simply not found, exactly as a run that does not exist is. There is no oracle.
        let member = VerifiedIdentity::user_for_test(&room.org, "member-2");
        let status = apply_as(&room, &room.run, &member, "step-1", rename("Not yours"))
            .await
            .expect_err("another member's run");
        assert_eq!(status.code(), tonic::Code::NotFound);
        let missing = apply_as(&room, "run-never-existed", &room.owner(), "step-1", rename("Not there"))
            .await
            .expect_err("a run that does not exist");
        assert_eq!(missing.code(), status.code());
        assert_eq!(missing.message(), status.message());
        assert_eq!(presentation(&room).await.title, None);
        assert!(receipts(&room, &room.run).await.is_empty());
        room.finish().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn events_carry_ids_only() {
        let Some(room) = Room::new("events").await else { return };
        apply_as(&room, &room.run, &room.owner(), "step-1", rename("Quarterly canary plan"))
            .await
            .expect("a rename");
        apply_as(&room, &room.run, &room.owner(), "step-2", pin(true))
            .await
            .expect("a pin");
        let events = thread_events(&room).await;
        assert_eq!(count_of(&events, "THREAD_PRESENTATION_UPDATED"), 2);
        assert_eq!(count_of(&events, RUN_THREAD_ACTION_EVENT), 2);
        assert_eq!(events.len(), 4);
        for (event_type, payload) in &events {
            let keys: Vec<&str> = payload.as_object().expect("an object").keys().map(String::as_str).collect();
            let mut keys = keys;
            keys.sort_unstable();
            if event_type == RUN_THREAD_ACTION_EVENT {
                assert_eq!(keys, ["action_id", "message_id", "outcome", "receipt_id", "run_id", "thread_id"]);
                assert_eq!(payload["thread_id"], room.thread.as_str());
                assert_eq!(payload["run_id"], room.run.as_str());
                assert_eq!(payload["outcome"], "applied");
            } else {
                // The human path's presentation event: ids and flags, no title.
                assert_eq!(
                    keys,
                    [
                        "archived", "memory_mode", "persona_enabled", "pinned", "preview_overridden", "starred",
                        "thread_id", "title_overridden", "unread"
                    ]
                );
                assert_eq!(payload["thread_id"], room.thread.as_str());
            }
        }
        // No title text reaches an event or a receipt.
        let everything: String = events.iter().map(|(_, payload)| payload.to_string()).collect();
        assert!(!everything.contains("canary"), "{everything}");
        let stored: String = sqlx::query_scalar(
            "SELECT concat_ws('|', receipt_id, action_id, payload_digest, decision_ref, subject_id, thread_id, outcome, message_id)
               FROM run_thread_action_receipts WHERE org_id = $1 ORDER BY created_at LIMIT 1",
        )
        .bind(&room.org)
        .fetch_one(&room.pool)
        .await
        .expect("read a receipt");
        assert!(!stored.contains("canary"), "{stored}");
        assert!(stored.contains("sha256:"), "the receipt holds the digest only: {stored}");
        room.finish().await;
    }
}
