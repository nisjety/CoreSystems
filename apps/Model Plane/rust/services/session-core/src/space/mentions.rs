//! Mentions of a human post and the facts a wake is verified against (build plan WS08-S2).
//!
//! A mention is an id, never text: `message_mentions` rows (migration 0075) sit beside a human
//! `post` and are written in the same transaction as the message. Agent output never carries
//! them, so an agent cannot wake an agent. A wake starts ONE run from a stored post for ONE
//! addressed agent; `StartManagedRun.wake` carries the Control-signed `model.run.wake` decision
//! and this module checks the stored facts that decision cannot know:
//!
//! * the thread is the caller's own and lives in the decision's Space;
//! * the trigger is the caller's own `post`, and its stored text hashes to the digest the
//!   decision bound;
//! * the agent is an `expected` mention on that post.
//!
//! The decision's signature, subject, schema and payload digest are verified in `grpc.rs`
//! (`verify_run_wake_space_decision`) beside the other decision verifiers. Everything here is
//! ids, enums and digests: no message text reaches a log or an event.

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::space_mentions as sm;
use sha2::{Digest as _, Sha256};
use sqlx::{PgExecutor, PgPool};
use tonic::metadata::MetadataValue;
use tonic::Status;

use crate::run_phase::ERROR_CODE_TRAILER;

// ---------------------------------------------------------------------------------------------
// Input validation.
// ---------------------------------------------------------------------------------------------

/// One validated mention, ready to insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidMention {
    pub(crate) subject_id: String,
    pub(crate) kind: &'static str,
    pub(crate) source: &'static str,
    /// `Some` exactly for an agent.
    pub(crate) wake_expectation: Option<&'static str>,
    pub(crate) refusal_code: Option<&'static str>,
    pub(crate) ordinal: i16,
}

impl ValidMention {
    /// The wire form an append reports: the effective wake state is the stored expectation.
    pub(crate) fn to_ref(&self) -> pb::MentionRef {
        pb::MentionRef {
            subject_id: self.subject_id.clone(),
            kind: self.kind.to_owned(),
            wake_state: self.wake_expectation.unwrap_or_default().to_owned(),
            reason: self.refusal_code.unwrap_or_default().to_owned(),
            source: self.source.to_owned(),
            ordinal: u32::try_from(self.ordinal).unwrap_or(0),
        }
    }
}

fn refusal_code_static(code: &str) -> Option<&'static str> {
    sm::REFUSAL_CODES.iter().copied().find(|known| *known == code)
}

/// Validates the `mentions` of an append: caps (20 people, 3 agents), the id pattern, no
/// duplicates, kind/source/expectation vocabulary. An agent mention on a thread the caller does
/// not own is forced to `refused/owner_only_thread` (Session Core holds that fact; the BFF
/// cannot). Agent ordinals are assigned here, in the order given.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_mentions(
    inputs: &[pb::MentionInput],
    caller_is_thread_owner: bool,
) -> Result<Vec<ValidMention>, Status> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(inputs.len());
    let (mut people, mut agents) = (0usize, 0usize);
    for input in inputs {
        let subject_id = input.subject_id.trim();
        if !sm::valid_subject_id(subject_id) {
            return Err(Status::invalid_argument("mention subject_id is not a valid id"));
        }
        if !seen.insert(subject_id.to_owned()) {
            return Err(Status::invalid_argument("mention subject_id is duplicated"));
        }
        let source = match input.source.trim() {
            "" | sm::SOURCE_TYPED => sm::SOURCE_TYPED,
            sm::SOURCE_LOCKED => sm::SOURCE_LOCKED,
            _ => return Err(Status::invalid_argument("mention source must be typed or locked")),
        };
        match input.kind.trim() {
            sm::KIND_USER => {
                people += 1;
                if !input.wake_expectation.trim().is_empty() || !input.refusal_code.trim().is_empty() {
                    return Err(Status::invalid_argument(
                        "a person mention carries no wake expectation",
                    ));
                }
                out.push(ValidMention {
                    subject_id: subject_id.to_owned(),
                    kind: sm::KIND_USER,
                    source,
                    wake_expectation: None,
                    refusal_code: None,
                    ordinal: 0,
                });
            }
            sm::KIND_AGENT => {
                let ordinal = i16::try_from(agents).unwrap_or(i16::MAX);
                agents += 1;
                let (expectation, refusal) = match input.wake_expectation.trim() {
                    sm::WAKE_EXPECTED if input.refusal_code.trim().is_empty() => {
                        if caller_is_thread_owner {
                            (sm::WAKE_EXPECTED, None)
                        } else {
                            (sm::WAKE_REFUSED, Some(sm::REFUSAL_OWNER_ONLY_THREAD))
                        }
                    }
                    sm::WAKE_REFUSED => {
                        let code = refusal_code_static(input.refusal_code.trim()).ok_or_else(|| {
                            Status::invalid_argument("a refused agent mention needs a known refusal code")
                        })?;
                        (sm::WAKE_REFUSED, Some(code))
                    }
                    _ => {
                        return Err(Status::invalid_argument(
                            "an agent mention needs wake_expectation expected or refused",
                        ))
                    }
                };
                out.push(ValidMention {
                    subject_id: subject_id.to_owned(),
                    kind: sm::KIND_AGENT,
                    source,
                    wake_expectation: Some(expectation),
                    refusal_code: refusal,
                    ordinal,
                });
            }
            _ => return Err(Status::invalid_argument("mention kind must be user or agent")),
        }
        if people > sm::MAX_PEOPLE_MENTIONS || agents > sm::MAX_AGENT_MENTIONS {
            return Err(Status::invalid_argument("too many mentions on one post"));
        }
    }
    Ok(out)
}

/// Inserts the rows of a post's mentions. Call inside the append transaction, after the message
/// row exists. Replays insert nothing (`ON CONFLICT DO NOTHING` on `(message_id, subject_id)`).
pub(crate) async fn insert_mentions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: &str,
    thread_id: &str,
    space_id: &str,
    message_id: &str,
    mentions: &[ValidMention],
) -> Result<(), Status> {
    for mention in mentions {
        sqlx::query(
            "INSERT INTO message_mentions
             (org_id, message_id, thread_id, space_id, subject_id, subject_kind, source,
              wake_expectation, refusal_code, ordinal)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (message_id, subject_id) DO NOTHING",
        )
        .bind(org_id)
        .bind(message_id)
        .bind(thread_id)
        .bind(space_id)
        .bind(&mention.subject_id)
        .bind(mention.kind)
        .bind(mention.source)
        .bind(mention.wake_expectation)
        .bind(mention.refusal_code)
        .bind(mention.ordinal)
        .execute(&mut **tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The reply slot.
// ---------------------------------------------------------------------------------------------

/// Whether a decision-less assistant reply may land in a Space thread.
///
/// Open directly after a user message (the old rule). Also open when the last row is an assistant
/// answer, the newest user row is a human post, and fewer assistants have answered since than the
/// post expected agent mentions (at most three): the second and third addressed agent each get
/// their reply. Pure; the facts come from [`load_reply_slot_facts`].
#[must_use]
pub(crate) fn reply_slot_is_open(
    last_role: Option<&str>,
    newest_user_is_post: bool,
    assistants_since_last_user: i64,
    expected_agent_mentions: i64,
) -> bool {
    match last_role.map(str::trim) {
        Some("user") => true,
        Some("assistant") => {
            let cap = i64::try_from(sm::MAX_AGENT_MENTIONS).unwrap_or(3);
            newest_user_is_post && assistants_since_last_user < expected_agent_mentions.min(cap)
        }
        _ => false,
    }
}

/// `(newest user row is a post, assistant rows after it, expected agent mentions on it)`.
pub(crate) async fn load_reply_slot_facts<'e>(
    executor: impl PgExecutor<'e>,
    thread_id: &str,
) -> Result<(bool, i64, i64), Status> {
    sqlx::query_as(
        "WITH lu AS (
            SELECT id, sequence, post_kind FROM messages
            WHERE thread_id = $1 AND on_active_path AND role = 'user'
            ORDER BY sequence DESC LIMIT 1)
         SELECT COALESCE((SELECT post_kind = 'post' FROM lu), FALSE),
                (SELECT COUNT(*) FROM messages m, lu
                  WHERE m.thread_id = $1 AND m.on_active_path AND m.role = 'assistant'
                    AND m.sequence > lu.sequence),
                (SELECT COUNT(*) FROM message_mentions mm, lu
                  WHERE mm.message_id = lu.id AND mm.wake_expectation = 'expected')",
    )
    .bind(thread_id)
    .fetch_one(executor)
    .await
    .map_err(|e| Status::internal(e.to_string()))
}

/// The extended reply-slot rule for a thread whose last row is not a user row. Off with
/// `SPACE_WAKE_CORE`, so flag-off keeps the old rule exactly.
pub(crate) async fn extended_reply_slot_is_open(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread_id: &str,
    last_role: Option<&str>,
) -> Result<bool, Status> {
    if !sm::wake_core_enabled() || last_role.map(str::trim) != Some("assistant") {
        return Ok(false);
    }
    let (newest_is_post, assistants_since, expected) = load_reply_slot_facts(&mut **tx, thread_id).await?;
    Ok(reply_slot_is_open(last_role, newest_is_post, assistants_since, expected))
}

// ---------------------------------------------------------------------------------------------
// Reads: the mentions of a post with the wake state derived from its newest run.
// ---------------------------------------------------------------------------------------------

/// The derived wake state: no run yet keeps the stored expectation; a run reports its phase.
#[must_use]
pub(crate) fn wake_state(expectation: Option<&str>, run_phase: Option<&str>) -> &'static str {
    match (expectation, run_phase) {
        (Some(sm::WAKE_REFUSED), _) => sm::STATE_REFUSED,
        (_, Some("queued" | "running")) => sm::STATE_RUNNING,
        (_, Some("awaiting_approval")) => sm::STATE_AWAITING_APPROVAL,
        (_, Some("completed")) => sm::STATE_COMPLETED,
        (_, Some("cancelled")) => sm::STATE_CANCELLED,
        (_, Some(_)) => sm::STATE_FAILED,
        (Some(_), None) => sm::STATE_EXPECTED,
        (None, None) => "",
    }
}

type MentionRow = (String, String, String, String, Option<String>, Option<String>, i16, Option<String>);

/// Fills `mentions` on the human posts of a page. One query for the whole page, and none when
/// the page has no post. A person's mention carries no wake state.
pub(crate) async fn attach_mentions(
    pool: &PgPool,
    org_id: &str,
    thread_id: &str,
    messages: &mut [pb::SessionMessage],
) -> Result<(), Status> {
    let ids: Vec<String> = messages
        .iter()
        .filter(|m| m.post_kind == "post" && !m.message_id.is_empty())
        .map(|m| m.message_id.clone())
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "SELECT mm.message_id, mm.subject_id, mm.subject_kind, mm.source, mm.wake_expectation,
                mm.refusal_code, mm.ordinal, rr.phase
         FROM message_mentions mm
         LEFT JOIN LATERAL (
             SELECT {phase} AS phase FROM runs r
             WHERE r.trigger_message_id = mm.message_id AND r.agent_subject_id = mm.subject_id
             ORDER BY r.created_at DESC LIMIT 1
         ) rr ON mm.subject_kind = 'agent'
         WHERE mm.org_id = $1 AND mm.thread_id = $2 AND mm.message_id = ANY($3)
         ORDER BY mm.message_id, mm.subject_kind, mm.ordinal, mm.subject_id",
        phase = crate::run_phase::RUN_PHASE_SQL
    );
    let rows: Vec<MentionRow> = sqlx::query_as(&sql)
        .bind(org_id)
        .bind(thread_id)
        .bind(&ids)
        .fetch_all(pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    for (message_id, subject_id, kind, source, expectation, refusal, ordinal, phase) in rows {
        let state = if kind == sm::KIND_AGENT {
            wake_state(expectation.as_deref(), phase.as_deref())
        } else {
            ""
        };
        if let Some(message) = messages.iter_mut().find(|m| m.message_id == message_id) {
            message.mentions.push(pb::MentionRef {
                subject_id,
                kind,
                wake_state: state.to_owned(),
                reason: refusal.unwrap_or_default(),
                source,
                ordinal: u32::try_from(ordinal).unwrap_or(0),
            });
        }
    }
    Ok(())
}

/// Keeps only the newest `cap` human posts of a page (0 = no cap); turns are never dropped.
pub(crate) fn apply_post_context_cap(messages: &mut Vec<pb::SessionMessage>, cap: u32) {
    if cap == 0 {
        return;
    }
    let mut kept = 0u32;
    let mut keep = vec![true; messages.len()];
    for (index, message) in messages.iter().enumerate().rev() {
        if message.post_kind == "post" {
            kept += 1;
            if kept > cap {
                keep[index] = false;
            }
        }
    }
    let mut index = 0;
    messages.retain(|_| {
        let k = keep[index];
        index += 1;
        k
    });
}

// ---------------------------------------------------------------------------------------------
// The wake.
// ---------------------------------------------------------------------------------------------

/// What a verified wake decision admits. Every field came from the signed decision's digest or
/// from the verified caller, never from a caller-supplied value that is not bound by it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerifiedWake {
    pub(crate) space_id: String,
    pub(crate) trigger_message_id: String,
    pub(crate) trigger_content_digest: String,
    pub(crate) agent_subject_id: String,
    pub(crate) turn_origin: String,
}

/// A refusal carrying a typed code on the `x-mp-error-code` trailer.
pub(crate) fn coded_status(mut status: Status, code: &'static str) -> Status {
    status
        .metadata_mut()
        .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(code));
    status
}

/// `wake_trigger_mismatch`: the trigger is not the caller's own stored post with the bound digest,
/// or the agent is not an expected mention on it.
fn trigger_mismatch(message: &'static str) -> Status {
    coded_status(Status::failed_precondition(message), "wake_trigger_mismatch")
}

/// The `sha256:` hex digest of a stored post's text (the commitment the decision binds).
#[must_use]
pub(crate) fn content_digest(content: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(content.as_bytes()))
}

/// The payload digest of a `model.run.wake` decision, recomputed from the request fields and the
/// verified caller (see `mp_contracts::space_mentions` for the byte layout). The thread id is the
/// start request's own.
#[must_use]
pub(crate) fn run_wake_payload_digest(
    wake: &pb::RunWakeContext,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.run.wake\0v1\0");
    let mut write_field = |name: &str, value: &str| {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    };
    for (name, value) in [
        ("org_id", org_id),
        ("user_id", user_id),
        ("thread_id", thread_id),
        ("space_id", wake.space_id.as_str()),
        ("space_decision_ref", wake.space_decision_ref.as_str()),
        ("recipient_audience_ref", wake.recipient_audience_ref.as_str()),
        ("recipient_audience_hash", wake.recipient_audience_hash.as_str()),
        ("privacy_policy_ref", wake.privacy_policy_ref.as_str()),
        ("resource_authorization_ref", wake.resource_authorization_ref.as_str()),
        ("trigger_message_id", wake.trigger_message_id.as_str()),
        ("content_digest", wake.trigger_content_digest.as_str()),
        ("agent_subject_id", wake.agent_subject_id.as_str()),
        ("turn_origin", wake.turn_origin.as_str()),
        ("action_schema_hash", wake.action_schema_hash.as_str()),
        ("idempotency_key", wake.idempotency_key.as_str()),
    ] {
        write_field(name, value);
    }
    digest.update(b"authority_revision\0");
    digest.update(wake.authority_revision.to_be_bytes());
    digest.update(b"recipient_audience_revision\0");
    digest.update(wake.recipient_audience_revision.to_be_bytes());
    format!("sha256:{:x}", digest.finalize())
}

/// Whether the wake carries the complete decision envelope and the four wake fields (all or
/// nothing: a wake has no decision-less form).
#[must_use]
pub(crate) fn wake_context_is_complete(wake: &pb::RunWakeContext) -> bool {
    [
        wake.space_id.trim(),
        wake.space_decision_ref.trim(),
        wake.recipient_audience_ref.trim(),
        wake.recipient_audience_hash.trim(),
        wake.privacy_policy_ref.trim(),
        wake.resource_authorization_ref.trim(),
        wake.space_decision_token.trim(),
        wake.action_schema_hash.trim(),
        wake.payload_digest.trim(),
        wake.idempotency_key.trim(),
        wake.trigger_message_id.trim(),
        wake.trigger_content_digest.trim(),
        wake.agent_subject_id.trim(),
        wake.turn_origin.trim(),
    ]
    .iter()
    .all(|value| !value.is_empty())
        && wake.authority_revision != 0
        && wake.recipient_audience_revision != 0
}

/// The known turn origins a verified decision may name.
#[must_use]
pub(crate) fn known_turn_origin(origin: &str) -> bool {
    sm::run_origin_for_turn_origin(origin).is_some()
}

/// Checks the stored facts of a verified wake: the thread is the caller's own and lives in the
/// decision's Space; the trigger is the caller's own `post` and its text hashes to the bound
/// digest; the agent is an `expected` mention on it. The run slot guard runs in the start
/// transaction. Typed refusals: PermissionDenied `wake_owner_required`, FailedPrecondition
/// `wake_trigger_mismatch`.
pub(crate) async fn check_wake_facts(
    pool: &PgPool,
    org_id: &str,
    thread_id: &str,
    caller_id: &str,
    wake: &VerifiedWake,
) -> Result<(), Status> {
    let thread: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT user_id, space_id FROM threads WHERE id = $1 AND org_id = $2")
            .bind(thread_id)
            .bind(org_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
    let (owner_id, space_id) = thread.ok_or_else(|| Status::not_found("thread not found"))?;
    if owner_id != caller_id {
        return Err(coded_status(
            Status::permission_denied("only the thread owner can wake an agent in it"),
            "wake_owner_required",
        ));
    }
    if space_id.as_deref() != Some(wake.space_id.as_str()) {
        return Err(Status::permission_denied(
            "Control wake decision targets another Space",
        ));
    }
    let trigger: Option<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT role, post_kind, author_subject_id, content FROM messages
         WHERE id = $1 AND thread_id = $2",
    )
    .bind(&wake.trigger_message_id)
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    let Some((role, post_kind, author, content)) = trigger else {
        return Err(trigger_mismatch("wake trigger is not a message of this thread"));
    };
    if role != "user" || post_kind != "post" || author.as_deref() != Some(caller_id) {
        return Err(trigger_mismatch("wake trigger is not the caller's own post"));
    }
    if content_digest(&content) != wake.trigger_content_digest {
        return Err(trigger_mismatch("wake trigger text does not match the bound digest"));
    }
    let expected: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT wake_expectation FROM message_mentions
         WHERE message_id = $1 AND subject_id = $2 AND subject_kind = 'agent'",
    )
    .bind(&wake.trigger_message_id)
    .bind(&wake.agent_subject_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    if expected.and_then(|(e,)| e).as_deref() != Some(sm::WAKE_EXPECTED) {
        return Err(trigger_mismatch("agent is not an expected mention on the trigger post"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Append helpers.
// ---------------------------------------------------------------------------------------------

/// Adds `mention_count` and `agent_mention_count` to a `MESSAGE_APPENDED` payload, only when the
/// post recorded mentions. Counts only: ids and text never reach the event.
#[must_use]
pub(crate) fn with_mention_counts(
    mut payload: serde_json::Value,
    recorded: &[pb::MentionRef],
) -> serde_json::Value {
    if recorded.is_empty() {
        return payload;
    }
    if let Some(map) = payload.as_object_mut() {
        map.insert("mention_count".to_owned(), serde_json::json!(recorded.len()));
        map.insert(
            "agent_mention_count".to_owned(),
            serde_json::json!(recorded.iter().filter(|m| m.kind == sm::KIND_AGENT).count()),
        );
    }
    payload
}

/// The mentions stored on one message, as an append reports them (the answer to a replay).
pub(crate) async fn load_recorded_mentions<'e>(
    executor: impl PgExecutor<'e>,
    message_id: &str,
) -> Result<Vec<pb::MentionRef>, Status> {
    let rows: Vec<(String, String, String, Option<String>, Option<String>, i16)> = sqlx::query_as(
        "SELECT subject_id, subject_kind, source, wake_expectation, refusal_code, ordinal
         FROM message_mentions WHERE message_id = $1
         ORDER BY subject_kind, ordinal, subject_id",
    )
    .bind(message_id)
    .fetch_all(executor)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|(subject_id, kind, source, expectation, refusal, ordinal)| pb::MentionRef {
            subject_id,
            kind,
            wake_state: expectation.unwrap_or_default(),
            reason: refusal.unwrap_or_default(),
            source,
            ordinal: u32::try_from(ordinal).unwrap_or(0),
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Pure tier (no database, no environment). The verifier and DB tiers live in
// `grpc_space_mentions_tests.rs`, which can reach the private verifiers of `grpc.rs`.
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str) -> pb::MentionInput {
        pb::MentionInput {
            subject_id: id.to_owned(),
            kind: "user".to_owned(),
            ..Default::default()
        }
    }

    fn agent(id: &str, expectation: &str, refusal: &str) -> pb::MentionInput {
        pb::MentionInput {
            subject_id: id.to_owned(),
            kind: "agent".to_owned(),
            wake_expectation: expectation.to_owned(),
            refusal_code: refusal.to_owned(),
            ..Default::default()
        }
    }

    fn message(id: &str, post_kind: &str) -> pb::SessionMessage {
        pb::SessionMessage {
            message_id: id.to_owned(),
            post_kind: post_kind.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn reply_slot_table() {
        // (last_role, newest user row is a post, assistants since, expected agent mentions) -> open
        let cases: [(Option<&str>, bool, i64, i64, bool); 14] = [
            // The old rule: directly after a user row the slot is open, whatever the rest.
            (Some("user"), false, 0, 0, true),
            (Some("user"), true, 0, 3, true),
            (Some(" user "), false, 5, 0, true),
            // An empty thread or a system/tool tail never opens it.
            (None, true, 0, 3, false),
            (Some("system"), true, 0, 3, false),
            (Some("tool"), true, 0, 3, false),
            // After an assistant: only a post with more expected agents than answers so far.
            (Some("assistant"), true, 1, 2, true),
            (Some("assistant"), true, 2, 3, true),
            (Some("assistant"), true, 2, 2, false),
            (Some("assistant"), true, 3, 3, false),
            (Some("assistant"), true, 1, 1, false),
            // A turn (not a post) never opens a second answer.
            (Some("assistant"), false, 1, 3, false),
            // The cap is three even when more rows claim to be expected.
            (Some("assistant"), true, 3, 9, false),
            (Some("assistant"), true, 0, 0, false),
        ];
        for (last_role, is_post, since, expected, open) in cases {
            assert_eq!(
                reply_slot_is_open(last_role, is_post, since, expected),
                open,
                "{last_role:?} post={is_post} since={since} expected={expected}"
            );
        }
    }

    #[test]
    fn mention_input_validation() {
        // An owner's post: agents keep their expectation, ordinals are the agents' order.
        let valid = validate_mentions(
            &[
                person("user-2"),
                agent("agent-x", "expected", ""),
                person("user-3"),
                agent("agent-y", "refused", "respond_to_denied"),
                agent("agent-z", "expected", ""),
            ],
            true,
        )
        .expect("valid mentions");
        assert_eq!(valid.len(), 5);
        assert_eq!(valid[0].kind, "user");
        assert_eq!(valid[0].wake_expectation, None);
        assert_eq!(valid[0].source, "typed");
        assert_eq!(
            (valid[1].wake_expectation, valid[1].refusal_code, valid[1].ordinal),
            (Some("expected"), None, 0)
        );
        assert_eq!(
            (valid[3].wake_expectation, valid[3].refusal_code, valid[3].ordinal),
            (Some("refused"), Some("respond_to_denied"), 1)
        );
        assert_eq!(valid[4].ordinal, 2);
        let wire = valid[4].to_ref();
        assert_eq!((wire.kind.as_str(), wire.wake_state.as_str(), wire.ordinal), ("agent", "expected", 2));

        // A locked source survives; an unknown one is refused.
        let locked = pb::MentionInput {
            source: "locked".to_owned(),
            ..agent("agent-x", "expected", "")
        };
        assert_eq!(validate_mentions(&[locked], true).expect("locked")[0].source, "locked");
        let bad_source = pb::MentionInput {
            source: "forged".to_owned(),
            ..person("user-2")
        };
        assert!(validate_mentions(&[bad_source], true).is_err());

        // Not the thread owner: an expected agent is downgraded, a refused one keeps its code.
        let downgraded = validate_mentions(
            &[agent("agent-x", "expected", ""), agent("agent-y", "refused", "invocation_blocked")],
            false,
        )
        .expect("downgraded");
        assert_eq!(
            (downgraded[0].wake_expectation, downgraded[0].refusal_code),
            (Some("refused"), Some("owner_only_thread"))
        );
        assert_eq!(downgraded[1].refusal_code, Some("invocation_blocked"));

        // Refusals: every one is InvalidArgument and nothing partial comes back.
        let twenty_one: Vec<pb::MentionInput> = (0..21).map(|n| person(&format!("user-{n}"))).collect();
        let four_agents: Vec<pb::MentionInput> =
            (0..4).map(|n| agent(&format!("agent-{n}"), "expected", "")).collect();
        let twenty: Vec<pb::MentionInput> = (0..20).map(|n| person(&format!("user-{n}"))).collect();
        assert_eq!(validate_mentions(&twenty, true).expect("twenty people").len(), 20);
        let three: Vec<pb::MentionInput> = (0..3).map(|n| agent(&format!("agent-{n}"), "expected", "")).collect();
        assert_eq!(validate_mentions(&three, true).expect("three agents").len(), 3);
        for (name, inputs) in [
            ("too many people", twenty_one),
            ("too many agents", four_agents),
            ("duplicate", vec![person("user-2"), agent("user-2", "expected", "")]),
            ("bad id", vec![person("has space")]),
            ("empty id", vec![person("")]),
            ("unknown kind", vec![pb::MentionInput { kind: "group".to_owned(), ..person("g-1") }]),
            ("person with expectation", vec![pb::MentionInput { wake_expectation: "expected".to_owned(), ..person("user-2") }]),
            ("person with refusal", vec![pb::MentionInput { refusal_code: "owner_only_thread".to_owned(), ..person("user-2") }]),
            ("agent without expectation", vec![agent("agent-x", "", "")]),
            ("agent with unknown expectation", vec![agent("agent-x", "maybe", "")]),
            ("expected with a refusal code", vec![agent("agent-x", "expected", "respond_to_denied")]),
            ("refused without a code", vec![agent("agent-x", "refused", "")]),
            ("refused with an unknown code", vec![agent("agent-x", "refused", "because")]),
        ] {
            let status = validate_mentions(&inputs, true).expect_err(name);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{name}");
        }
        assert!(validate_mentions(&[], false).expect("no mentions").is_empty());
    }

    #[test]
    fn wake_state_table() {
        for (expectation, phase, state) in [
            (Some("refused"), None, "refused"),
            (Some("refused"), Some("running"), "refused"),
            (Some("expected"), None, "expected"),
            (Some("expected"), Some("queued"), "running"),
            (Some("expected"), Some("running"), "running"),
            (Some("expected"), Some("awaiting_approval"), "awaiting_approval"),
            (Some("expected"), Some("completed"), "completed"),
            (Some("expected"), Some("cancelled"), "cancelled"),
            (Some("expected"), Some("failed"), "failed"),
            (Some("expected"), Some("anything_else"), "failed"),
            (None, None, ""),
        ] {
            assert_eq!(wake_state(expectation, phase), state, "{expectation:?} {phase:?}");
        }
    }

    #[test]
    fn post_context_cap_keeps_the_newest_posts_and_every_turn() {
        let page = || {
            vec![
                message("p1", "post"),
                message("t1", "turn"),
                message("p2", "post"),
                message("p3", "post"),
                message("t2", "turn"),
                message("p4", "post"),
            ]
        };
        let ids = |messages: &[pb::SessionMessage]| {
            messages.iter().map(|m| m.message_id.clone()).collect::<Vec<_>>()
        };
        let mut uncapped = page();
        apply_post_context_cap(&mut uncapped, 0);
        assert_eq!(ids(&uncapped), ["p1", "t1", "p2", "p3", "t2", "p4"]);
        let mut capped = page();
        apply_post_context_cap(&mut capped, 2);
        assert_eq!(ids(&capped), ["t1", "p3", "t2", "p4"]);
        let mut wide = page();
        apply_post_context_cap(&mut wide, 10);
        assert_eq!(wide.len(), 6);
    }

    #[test]
    fn mention_counts_are_counts_only() {
        let base = serde_json::json!({ "message_id": "m-1" });
        assert_eq!(with_mention_counts(base.clone(), &[]), base, "no mentions, no new keys");
        let recorded = validate_mentions(
            &[person("person-secret-1"), agent("agent-secret-1", "expected", ""), person("person-secret-2")],
            true,
        )
        .expect("valid")
        .iter()
        .map(ValidMention::to_ref)
        .collect::<Vec<_>>();
        let payload = with_mention_counts(base, &recorded);
        assert_eq!(payload["mention_count"], 3);
        assert_eq!(payload["agent_mention_count"], 1);
        let text = payload.to_string();
        for id in ["person-secret-1", "agent-secret-1", "person-secret-2"] {
            assert!(!text.contains(id), "the event carries no mentioned id: {text}");
        }
    }

    fn complete_wake() -> pb::RunWakeContext {
        pb::RunWakeContext {
            space_id: "space-1".to_owned(),
            space_decision_ref: "decision-1".to_owned(),
            recipient_audience_ref: "audience-1".to_owned(),
            recipient_audience_revision: 4,
            recipient_audience_hash: "sha256:audience".to_owned(),
            privacy_policy_ref: "privacy-1".to_owned(),
            authority_revision: 7,
            resource_authorization_ref: "resource-1".to_owned(),
            space_decision_token: "v1.k.p.s".to_owned(),
            action_schema_hash: sm::DECISION_SCHEMA.to_owned(),
            payload_digest: "sha256:digest".to_owned(),
            idempotency_key: "wake-1".to_owned(),
            trigger_message_id: "message-1".to_owned(),
            trigger_content_digest: "sha256:content".to_owned(),
            agent_subject_id: "agent-1".to_owned(),
            turn_origin: "direct".to_owned(),
        }
    }

    #[test]
    fn a_wake_has_no_partial_form() {
        assert!(wake_context_is_complete(&complete_wake()));
        assert!(!wake_context_is_complete(&pb::RunWakeContext::default()));
        let edits: [fn(&mut pb::RunWakeContext); 16] = [
            |w| w.space_id.clear(),
            |w| w.space_decision_ref.clear(),
            |w| w.recipient_audience_ref.clear(),
            |w| w.recipient_audience_revision = 0,
            |w| w.recipient_audience_hash = "  ".to_owned(),
            |w| w.privacy_policy_ref.clear(),
            |w| w.authority_revision = 0,
            |w| w.resource_authorization_ref.clear(),
            |w| w.space_decision_token.clear(),
            |w| w.action_schema_hash.clear(),
            |w| w.payload_digest.clear(),
            |w| w.idempotency_key.clear(),
            |w| w.trigger_message_id.clear(),
            |w| w.trigger_content_digest.clear(),
            |w| w.agent_subject_id.clear(),
            |w| w.turn_origin.clear(),
        ];
        for (index, edit) in edits.iter().enumerate() {
            let mut wake = complete_wake();
            edit(&mut wake);
            assert!(!wake_context_is_complete(&wake), "field edit {index}");
        }
        for origin in ["direct", "ambient", "automation"] {
            assert!(known_turn_origin(origin), "{origin}");
        }
        for origin in ["", "chat", "Direct", "api"] {
            assert!(!known_turn_origin(origin), "{origin}");
        }
    }

    #[test]
    fn coded_status_carries_the_trailer() {
        let status = coded_status(Status::failed_precondition("x"), "wake_trigger_mismatch");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            status
                .metadata()
                .get(ERROR_CODE_TRAILER)
                .and_then(|value| value.to_str().ok()),
            Some("wake_trigger_mismatch")
        );
    }

    /// The digest Control's `runWakePayloadDigest` produces for the shared vector (WS08-S1).
    /// Change the fixture only on an intended wire change, on both sides.
    #[test]
    fn run_wake_digest_matches_the_shared_golden() {
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/run-wake-v1.golden.json"))
                .expect("golden fixture");
        let input = &golden["input"];
        let expected = &golden["expected"];
        let text = |key: &str| input[key].as_str().expect(key).to_owned();
        assert_eq!(expected["action"], sm::DECISION_ACTION_ID);
        assert_eq!(expected["schema"], sm::DECISION_SCHEMA);
        assert_eq!(expected["permissions"], serde_json::json!([sm::PERMISSION_WAKE]));

        let content_digest_hex = content_digest(&text("content"));
        assert_eq!(content_digest_hex, expected["content_digest"].as_str().expect("content digest"));

        let wake = pb::RunWakeContext {
            space_id: text("space_id"),
            space_decision_ref: text("space_decision_ref"),
            recipient_audience_ref: text("recipient_audience_ref"),
            recipient_audience_revision: input["recipient_audience_revision"].as_u64().expect("audience revision"),
            recipient_audience_hash: text("recipient_audience_hash"),
            privacy_policy_ref: text("privacy_policy_ref"),
            authority_revision: input["authority_revision"].as_u64().expect("authority revision"),
            resource_authorization_ref: text("resource_authorization_ref"),
            action_schema_hash: sm::DECISION_SCHEMA.to_owned(),
            idempotency_key: text("idempotency_key"),
            trigger_message_id: text("trigger_message_id"),
            trigger_content_digest: content_digest_hex,
            agent_subject_id: text("agent_subject_id"),
            turn_origin: text("turn_origin"),
            ..Default::default()
        };
        assert_eq!(
            run_wake_payload_digest(&wake, &text("org_id"), &text("user_id"), &text("thread_id")),
            expected["digest"].as_str().expect("digest"),
            "the Rust digest drifted from Control's; update both together"
        );

        // The token and the caller's digest field are not part of the commitment.
        let mut unbound = wake.clone();
        unbound.space_decision_token = "anything".to_owned();
        unbound.payload_digest = "anything".to_owned();
        assert_eq!(
            run_wake_payload_digest(&unbound, &text("org_id"), &text("user_id"), &text("thread_id")),
            expected["digest"].as_str().expect("digest")
        );

        // The mapping table the golden carries is the contract's.
        for row in golden["turn_origin_mapping"].as_array().expect("mapping") {
            assert_eq!(
                sm::run_origin_for_turn_origin(row["turn_origin"].as_str().expect("origin")),
                row["run_origin"].as_str(),
                "{row}"
            );
        }
    }

    #[test]
    fn every_bound_field_moves_the_wake_digest() {
        let base = complete_wake();
        let digest = |w: &pb::RunWakeContext, org: &str, user: &str, thread: &str| {
            run_wake_payload_digest(w, org, user, thread)
        };
        let reference = digest(&base, "org-1", "user-1", "thread-1");
        assert_ne!(reference, digest(&base, "org-2", "user-1", "thread-1"));
        assert_ne!(reference, digest(&base, "org-1", "user-2", "thread-1"));
        assert_ne!(reference, digest(&base, "org-1", "user-1", "thread-2"));
        let edits: [fn(&mut pb::RunWakeContext); 14] = [
            |w| w.space_id.push('x'),
            |w| w.space_decision_ref.push('x'),
            |w| w.recipient_audience_ref.push('x'),
            |w| w.recipient_audience_revision += 1,
            |w| w.recipient_audience_hash.push('x'),
            |w| w.privacy_policy_ref.push('x'),
            |w| w.authority_revision += 1,
            |w| w.resource_authorization_ref.push('x'),
            |w| w.action_schema_hash.push('x'),
            |w| w.idempotency_key.push('x'),
            |w| w.trigger_message_id.push('x'),
            |w| w.trigger_content_digest.push('x'),
            |w| w.agent_subject_id.push('x'),
            |w| w.turn_origin = "ambient".to_owned(),
        ];
        for (index, edit) in edits.iter().enumerate() {
            let mut wake = base.clone();
            edit(&mut wake);
            assert_ne!(reference, digest(&wake, "org-1", "user-1", "thread-1"), "field edit {index}");
        }
    }
}
