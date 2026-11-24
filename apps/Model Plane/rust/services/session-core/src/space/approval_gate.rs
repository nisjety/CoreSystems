//! Gate context, the standing-rule pass and the once-token (build plan WS10-S3).
//!
//! Three things, all content-free:
//!
//! - The gate context an approval carries so a retry can be matched to it: `tool_key`, the
//!   `action_digest` (sha256 of the canonical `{tool, input}`, never the call itself) and the turn
//!   origin. Written by the creator, validated here, and never stored under zero data retention.
//! - The standing-rule pass: a Control-signed `approval-grant-use-v1` token proves a standing rule
//!   covers one MCP tool call. A valid token lets the approval be recorded already granted, with
//!   `decided_by = grant:<rule id>`. The token widens nothing: the approved action still runs as the
//!   requester and the owner plane still authorizes it. A token that does not verify is refused,
//!   never ignored, so a forged claim cannot quietly become a plain request.
//! - The once-token: after a person grants an approval for a non-resumable call, the agent's retry
//!   of that exact call (same tool, same digest, same thread, same person) spends the grant once.
//!   One atomic update, so two concurrent retries have one winner.
//!
//! The approvals table already carries `consumed_at` (migration 0046); the gate columns are
//! migration 0074.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeMap;
use tonic::Status;

use crate::store::Pool;

/// Envelope version of the standing-rule proof. Not the version of a Space thread decision or of
/// `run-action-v1`, so none of them can be accepted for another.
pub(crate) const APPROVAL_GRANT_USE_VERSION: &str = "approval-grant-use-v1";
const APPROVAL_GRANT_USE_AUDIENCE: &str = "model-plane";
/// The only scope a standing rule has (Control stores no other).
const APPROVAL_GRANT_USE_SCOPE: &str = "always";
/// A standing rule covers MCP tools only; provider writes, shell and browser actions are once-only.
const STANDING_RULE_TOOL_PREFIX: &str = "mcp__";
const MAX_TOKEN_BYTES: usize = 4096;
/// Longest life of a proof, matching Control's `ApprovalGrantUseLifetime`.
const MAX_LIFETIME_SECS: i64 = 120;
/// Clock skew allowed on `issued_at`.
const MAX_ISSUED_SKEW_SECS: i64 = 60;
/// Longest accepted `tool_key`.
const MAX_TOOL_KEY_CHARS: usize = 64;
/// The turn origins an approval may state (WS10-S7 consumes them).
const TURN_ORIGINS: &[&str] = &["direct", "ambient", "automation"];
/// The typed refusal of a decision scope an approval's turn origin does not allow (WS10-S7).
pub(crate) const SCOPE_NOT_PERMITTED: &str = "approval_scope_not_permitted";

/// Whether a turn origin is a person's own turn. A missing or empty origin reads as `direct`: every
/// run that predates the claim was started by a person (WS10-S7).
pub(crate) fn origin_is_direct(turn_origin: Option<&str>) -> bool {
    match turn_origin.map(str::trim) {
        None | Some("") => true,
        Some(origin) => origin == mp_contracts::space_mentions::ORIGIN_DIRECT,
    }
}

/// The origin an approval carries: the run's own Control-signed claim when it is not `direct`
/// (a creator can never present an `ambient` or `automation` turn as `direct`), else what the
/// creator stated. Empty stays empty (read as `direct`).
pub(crate) fn effective_turn_origin(run_origin: Option<&str>, stated: Option<String>) -> Option<String> {
    if !origin_is_direct(run_origin) {
        return run_origin.map(|origin| origin.trim().to_owned());
    }
    stated
}

/// The run's turn origin claim (`runs.turn_origin`, migration 0075); `None` for a run without one.
pub(crate) async fn run_turn_origin(pool: &Pool, run_id: &str, org_id: &str) -> Result<Option<String>, Status> {
    sqlx::query_scalar::<_, Option<String>>("SELECT turn_origin FROM runs WHERE id = $1 AND org_id = $2")
        .bind(run_id)
        .bind(org_id)
        .fetch_optional(pool)
        .await
        .map(Option::flatten)
        .map_err(|error| {
            tracing::error!(%error, "run turn origin lookup failed");
            Status::unavailable("run turn origin unavailable")
        })
}

/// A decision on an approval of an `ambient` or `automation` turn is `once` only: no person was
/// watching that turn, so a wider scope (a thread or run grant) would let nobody's judgement
/// stand for later calls. Refused with a typed code; a `direct` or unknown approval passes.
pub(crate) async fn refuse_wide_scope_for_origin(
    pool: &Pool,
    approval_id: &str,
    org_id: &str,
    scope: &str,
) -> Result<(), Status> {
    if scope == "once" {
        return Ok(());
    }
    let origin: Option<Option<String>> =
        sqlx::query_scalar("SELECT turn_origin FROM approvals WHERE id = $1 AND org_id = $2")
            .bind(approval_id)
            .bind(org_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "approval turn origin lookup failed");
                Status::unavailable("approval turn origin unavailable")
            })?;
    if origin_is_direct(origin.flatten().as_deref()) {
        return Ok(());
    }
    Err(crate::space::mentions::coded_status(
        Status::permission_denied(SCOPE_NOT_PERMITTED),
        SCOPE_NOT_PERMITTED,
    ))
}

/// Claims of an `approval-grant-use-v1` token, exactly Control's `ApprovalGrantUse`.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct GrantUseClaims {
    pub token_ref: String,
    pub rule_id: String,
    pub scope: String,
    pub run_id: String,
    pub org_id: String,
    pub space_ref: String,
    pub subject_id: String,
    pub agent_subject_id: String,
    pub tool_key: String,
    pub action_key: String,
    pub action_schema_hash: String,
    pub authority_revision: i64,
    pub audience: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub nonce: String,
}

/// The validated gate context of a create request.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct GateContext {
    pub tool_key: Option<String>,
    pub action_digest: Option<String>,
    pub turn_origin: Option<String>,
}

fn is_plain_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOOL_KEY_CHARS
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-' | b'/'))
}

/// Validate the gate fields of a create request. `zdr` drops the digest: a zero-retention run
/// keeps no per-call fingerprint, so it can never be once-claimed.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_gate_context(
    tool_key: &str,
    action_digest: &str,
    turn_origin: &str,
    zdr: bool,
) -> Result<GateContext, Status> {
    let mut context = GateContext::default();
    if !tool_key.is_empty() {
        if !is_plain_identifier(tool_key) {
            return Err(Status::invalid_argument("tool_key is invalid"));
        }
        context.tool_key = Some(tool_key.to_owned());
    }
    if !action_digest.is_empty() {
        if action_digest.len() != 64 || !action_digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Status::invalid_argument("action_digest is invalid"));
        }
        if !zdr {
            context.action_digest = Some(action_digest.to_ascii_lowercase());
        }
    }
    if !turn_origin.is_empty() {
        if !TURN_ORIGINS.contains(&turn_origin) {
            return Err(Status::invalid_argument("turn_origin is invalid"));
        }
        context.turn_origin = Some(turn_origin.to_owned());
    }
    Ok(context)
}

/// Verify a standing-rule proof against the request it is presented with. Every claim is
/// compared with the server's own view (verified caller, run, the run's Space); a mismatch of
/// any kind is a refusal.
#[allow(clippy::result_large_err)]
pub(crate) fn verify_grant_use_token(
    token: &str,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    run_space_id: &str,
    tool_key: &str,
    now: DateTime<Utc>,
) -> Result<GrantUseClaims, Status> {
    let claims = open_signed_claims(token, crate::grpc::configured_control_space_decision_keys)?;
    check_claims(&claims, org_id, user_id, run_id, run_space_id, tool_key, now)?;
    Ok(claims)
}

/// The envelope, key and signature half of [`verify_grant_use_token`]. The trusted key set comes
/// from `load_keys`, which runs only once the envelope and key id have parsed (the order the
/// refusals were always given in), so a test can supply keys without the process environment.
#[allow(clippy::result_large_err)]
fn open_signed_claims(
    token: &str,
    load_keys: impl FnOnce() -> Result<BTreeMap<String, VerifyingKey>, Status>,
) -> Result<GrantUseClaims, Status> {
    if token.len() > MAX_TOKEN_BYTES {
        return Err(Status::invalid_argument("approval grant proof is too large"));
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 4 || parts[0] != APPROVAL_GRANT_USE_VERSION {
        return Err(Status::permission_denied("invalid approval grant proof envelope"));
    }
    let key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let keys = load_keys()?;
    let key = crate::decision_keys::lookup_key(&keys, &key_id)?;
    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| Status::permission_denied("invalid approval grant proof payload"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(parts[3])
        .ok()
        .and_then(|raw| Signature::from_slice(&raw).ok())
        .ok_or_else(|| Status::permission_denied("invalid approval grant proof signature"))?;
    key.verify(
        format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
        &signature,
    )
    .map_err(|_| Status::permission_denied("invalid approval grant proof signature"))?;
    serde_json::from_slice(&payload)
        .map_err(|_| Status::permission_denied("invalid approval grant proof claims"))
}

/// The claim checks, separate from the signature so they can be exercised without keys.
#[allow(clippy::result_large_err)]
fn check_claims(
    claims: &GrantUseClaims,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    run_space_id: &str,
    tool_key: &str,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    let present = [
        &claims.token_ref,
        &claims.rule_id,
        &claims.space_ref,
        &claims.agent_subject_id,
        &claims.action_key,
        &claims.action_schema_hash,
        &claims.nonce,
    ]
    .iter()
    .all(|value| !value.trim().is_empty());
    let bound = claims.audience == APPROVAL_GRANT_USE_AUDIENCE
        && claims.scope == APPROVAL_GRANT_USE_SCOPE
        && claims.org_id == org_id
        && claims.subject_id == user_id
        && claims.run_id == run_id
        && !run_space_id.is_empty()
        && claims.space_ref == run_space_id
        && claims.tool_key == tool_key
        && tool_key.starts_with(STANDING_RULE_TOOL_PREFIX)
        && claims.authority_revision > 0;
    let lifetime = claims.expires_at - claims.issued_at;
    let timely = claims.expires_at > now
        && claims.issued_at <= now + Duration::seconds(MAX_ISSUED_SKEW_SECS)
        && lifetime > Duration::zero()
        && lifetime <= Duration::seconds(MAX_LIFETIME_SECS);
    if present && bound && timely {
        Ok(())
    } else {
        Err(Status::permission_denied("approval grant proof does not match this call"))
    }
}

/// The decision record of an approval a standing rule passed. Scope is `once`: the rule waived
/// this one prompt. The rule id is provenance for the audit chain.
pub(crate) fn standing_rule_decision(rule_id: &str) -> JsonValue {
    json!({ "source": "standing_grant", "scope": "once", "rule_id": rule_id })
}

/// Everything the already-granted insert needs.
pub(crate) struct GrantedApproval<'a> {
    pub id: &'a str,
    pub run_id: &'a str,
    pub kind: &'a str,
    pub requested_by: &'a str,
    pub org_id: &'a str,
    pub user_id: &'a str,
    pub idempotency_key: &'a str,
    pub metadata: &'a JsonValue,
    pub expires_at: Option<DateTime<Utc>>,
    pub context: &'a GateContext,
    pub claims: &'a GrantUseClaims,
}

/// Record an approval already granted by a standing rule. `ON CONFLICT DO NOTHING` on the
/// `(org, user, idempotency key)` guard: when the step already has an approval (for example a
/// pending one from an earlier pause) nothing is written and `None` comes back, so the gate
/// reads the existing row's state and never treats it as passed. No delivery row is made: the
/// pass carries no descriptor, so no worker runs anything for it.
pub(crate) async fn insert_granted_by_rule(
    pool: &Pool,
    approval: &GrantedApproval<'_>,
) -> anyhow::Result<Option<String>> {
    let decision_reason = format!("standing rule {}", approval.claims.rule_id);
    let decided_by = format!("grant:{}", approval.claims.rule_id);
    let row: Option<(String,)> = sqlx::query_as(
        "INSERT INTO approvals (id, run_id, plan_id, kind, status, requested_by, decided_by, \
         decision_reason, org_id, user_id, idempotency_key, metadata, decided_at, expires_at, \
         tool_key, action_digest, turn_origin, grant_ref, grant_use_token_ref) \
         VALUES ($1, $2, NULL, $3, 'granted', $4, $5, $6, $7, $8, $9, $10, now(), $11, $12, $13, $14, $15, $16) \
         ON CONFLICT (org_id, user_id, idempotency_key) WHERE idempotency_key <> '' DO NOTHING \
         RETURNING id",
    )
    .bind(approval.id)
    .bind(approval.run_id)
    .bind(approval.kind)
    .bind(approval.requested_by)
    .bind(&decided_by)
    .bind(&decision_reason)
    .bind(approval.org_id)
    .bind(approval.user_id)
    .bind(approval.idempotency_key)
    .bind(approval.metadata)
    .bind(approval.expires_at)
    .bind(approval.context.tool_key.as_deref())
    .bind(approval.context.action_digest.as_deref())
    .bind(approval.context.turn_origin.as_deref())
    .bind(&approval.claims.rule_id)
    .bind(approval.claims.token_ref.trim())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
}

/// The unique index that makes a standing-rule proof single-use (migration 0136).
const TOKEN_REF_INDEX: &str = "approvals_grant_use_token_ref_key";

/// Whether an insert failed because this proof already passed another step: its `token_ref` is
/// stored on an earlier approval (migration 0136). A retry of the same step never gets here, the
/// idempotency arbiter answers it first.
pub(crate) fn is_grant_proof_replay(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<sqlx::Error>()
        .and_then(|error| error.as_database_error())
        .is_some_and(|db| db.is_unique_violation() && db.constraint() == Some(TOKEN_REF_INDEX))
}

/// The refusal of a proof that was already spent.
pub(crate) fn grant_proof_replayed() -> Status {
    Status::permission_denied("approval grant proof was already used")
}

/// Store the gate context on an approval this request just created (not on one an idempotent
/// retry resolved to). A no-op when the request stated none.
pub(crate) async fn record_gate_context(
    pool: &Pool,
    approval_id: &str,
    context: &GateContext,
) -> anyhow::Result<()> {
    if context.tool_key.is_none() && context.action_digest.is_none() && context.turn_origin.is_none() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE approvals SET tool_key = COALESCE($2, tool_key), \
         action_digest = COALESCE($3, action_digest), turn_origin = COALESCE($4, turn_origin) \
         WHERE id = $1",
    )
    .bind(approval_id)
    .bind(context.tool_key.as_deref())
    .bind(context.action_digest.as_deref())
    .bind(context.turn_origin.as_deref())
    .execute(pool)
    .await?;
    Ok(())
}

/// Spend the once-approval for one exact action, atomically.
///
/// Matches a granted, unspent, unexpired approval of the same person for the same tool and
/// digest, raised on the same thread as the retrying run, that the delivery worker does not run
/// (its card says the retry is the continuation: `resume_class = none`). `FOR UPDATE SKIP LOCKED`
/// makes concurrent claims safe: the loser sees no row and is refused. Returns the spent
/// approval id.
pub(crate) async fn claim_once(
    pool: &Pool,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    tool_key: &str,
    action_digest: &str,
) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "UPDATE approvals SET consumed_at = now() \
         WHERE id = ( \
             SELECT a.id FROM approvals a \
             JOIN runs ar ON ar.id = a.run_id \
             WHERE a.org_id = $1 AND a.user_id = $2 \
               AND a.tool_key = $3 AND a.action_digest = $4 \
               AND a.status = 'granted' AND a.consumed_at IS NULL \
               AND a.kind <> 'untrusted_content' \
               AND (a.expires_at IS NULL OR a.expires_at > now()) \
               AND a.continuation_mode = '' \
               AND COALESCE(a.metadata->'card'->>'resume_class', '') = 'none' \
               AND COALESCE(a.turn_origin, 'direct') = 'direct' \
               AND COALESCE(ar.turn_origin, 'direct') = 'direct' \
               AND ar.thread_id = (SELECT thread_id FROM runs WHERE id = $5 AND org_id = $1 AND user_id = $2) \
             ORDER BY a.decided_at DESC NULLS LAST \
             LIMIT 1 \
             FOR UPDATE OF a SKIP LOCKED \
         ) AND consumed_at IS NULL \
         RETURNING id",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(tool_key)
    .bind(action_digest.to_ascii_lowercase())
    .bind(run_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_phase::ERROR_CODE_TRAILER;
    use ed25519_dalek::{Signer, SigningKey};

    fn claims() -> GrantUseClaims {
        let now = Utc::now();
        GrantUseClaims {
            token_ref: "tok".into(),
            rule_id: "rule-1".into(),
            scope: "always".into(),
            run_id: "run-1".into(),
            org_id: "org-1".into(),
            space_ref: "space-1".into(),
            subject_id: "user-1".into(),
            agent_subject_id: "agent-1".into(),
            tool_key: "mcp__srv__tool".into(),
            action_key: "mcp__srv__tool".into(),
            action_schema_hash: "a".repeat(64),
            authority_revision: 3,
            audience: "model-plane".into(),
            issued_at: now,
            expires_at: now + Duration::seconds(120),
            nonce: "n".into(),
        }
    }

    #[test]
    fn claims_bind_org_user_run_space_and_tool() {
        let now = Utc::now();
        let ok = |c: &GrantUseClaims| {
            check_claims(c, "org-1", "user-1", "run-1", "space-1", "mcp__srv__tool", now)
        };
        assert!(ok(&claims()).is_ok());
        for mutate in [
            |c: &mut GrantUseClaims| c.org_id = "org-2".into(),
            |c: &mut GrantUseClaims| c.subject_id = "user-2".into(),
            |c: &mut GrantUseClaims| c.run_id = "run-2".into(),
            |c: &mut GrantUseClaims| c.space_ref = "space-2".into(),
            |c: &mut GrantUseClaims| c.tool_key = "mcp__other__tool".into(),
            |c: &mut GrantUseClaims| c.scope = "thread".into(),
            |c: &mut GrantUseClaims| c.audience = "other".into(),
            |c: &mut GrantUseClaims| c.expires_at = c.issued_at + Duration::seconds(121),
        ] {
            let mut c = claims();
            mutate(&mut c);
            assert!(ok(&c).is_err());
        }
    }

    #[test]
    fn a_standing_rule_never_covers_a_non_mcp_tool() {
        let now = Utc::now();
        let mut c = claims();
        c.tool_key = "execute_provider_action".into();
        assert!(check_claims(&c, "org-1", "user-1", "run-1", "space-1", "execute_provider_action", now).is_err());
    }

    #[test]
    fn gate_context_validates_and_zdr_drops_the_digest() {
        let digest = "ab".repeat(32);
        let context = validate_gate_context("shell", &digest, "direct", false).expect("valid");
        assert_eq!(context.action_digest.as_deref(), Some(digest.as_str()));
        let zdr = validate_gate_context("shell", &digest, "", true).expect("valid");
        assert!(zdr.action_digest.is_none());
        assert!(validate_gate_context("bad key!", "", "", false).is_err());
        assert!(validate_gate_context("shell", "short", "", false).is_err());
        assert!(validate_gate_context("shell", "", "weird", false).is_err());
    }

    #[test]
    fn origin_is_direct_reads_a_missing_origin_as_a_persons_own_turn() {
        for origin in [None, Some(""), Some("  "), Some("direct"), Some(" direct ")] {
            assert!(origin_is_direct(origin), "{origin:?}");
        }
        for origin in [Some("ambient"), Some("automation"), Some(" automation "), Some("Direct"), Some("weird")] {
            assert!(!origin_is_direct(origin), "{origin:?} is not a person's own turn");
        }
    }

    #[test]
    fn a_runs_own_non_direct_origin_wins_over_the_one_the_creator_states() {
        let stated = |origin: &str| Some(origin.to_owned());
        // A creator can never present an ambient or automation turn as direct.
        assert_eq!(effective_turn_origin(Some("ambient"), stated("direct")), stated("ambient"));
        assert_eq!(effective_turn_origin(Some("automation"), None), stated("automation"));
        assert_eq!(effective_turn_origin(Some("automation"), stated("ambient")), stated("automation"));
        assert_eq!(
            effective_turn_origin(Some(" ambient "), stated("direct")),
            stated("ambient"),
            "the run's claim is trimmed"
        );
        // A direct run, or one with no claim, keeps what the creator stated (even a lesser origin).
        assert_eq!(effective_turn_origin(Some("direct"), stated("ambient")), stated("ambient"));
        assert_eq!(effective_turn_origin(None, stated("automation")), stated("automation"));
        assert_eq!(effective_turn_origin(Some(""), stated("direct")), stated("direct"));
        assert_eq!(effective_turn_origin(Some("direct"), None), None);
        assert_eq!(effective_turn_origin(None, None), None);
    }

    #[test]
    fn a_standing_rule_decision_names_the_rule_and_waives_one_prompt() {
        assert_eq!(
            standing_rule_decision("rule-7"),
            json!({ "source": "standing_grant", "scope": "once", "rule_id": "rule-7" })
        );
    }

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-17T12:00:00Z")
            .expect("fixed time")
            .with_timezone(&Utc)
    }

    fn live_claims(now: DateTime<Utc>) -> GrantUseClaims {
        GrantUseClaims {
            issued_at: now,
            expires_at: now + Duration::seconds(120),
            ..claims()
        }
    }

    #[test]
    fn a_proof_is_timely_only_inside_its_lifetime_and_the_issue_skew() {
        let now = fixed_now();
        let secs = Duration::seconds;
        let check = |issued: Duration, expires: Duration| {
            let claims = GrantUseClaims {
                issued_at: now + issued,
                expires_at: now + expires,
                ..claims()
            };
            check_claims(&claims, "org-1", "user-1", "run-1", "space-1", "mcp__srv__tool", now)
        };
        // (issued, expires, both as offsets from now; accepted; why)
        for (issued, expires, accepted, name) in [
            (secs(0), secs(120), true, "issued now for the full 120 s"),
            (secs(-100), secs(20), true, "issued earlier and still live"),
            (secs(-120), secs(0), false, "expires exactly now"),
            (secs(-100), secs(-1), false, "expired a second ago"),
            (secs(60), secs(180), true, "issued at the 60 s skew"),
            (secs(61), secs(181), false, "issued beyond the skew"),
            (secs(10), secs(10), false, "zero lifetime"),
            (secs(10), secs(5), false, "expires before it was issued"),
            (secs(0), secs(121), false, "121 s lifetime"),
        ] {
            assert_eq!(check(issued, expires).is_ok(), accepted, "{name}");
        }
    }

    #[test]
    fn a_proof_missing_a_field_or_an_authority_revision_is_refused() {
        let now = fixed_now();
        let check = |claims: &GrantUseClaims| {
            check_claims(claims, "org-1", "user-1", "run-1", "space-1", "mcp__srv__tool", now)
        };
        assert!(check(&live_claims(now)).is_ok());
        for edit in [
            |c: &mut GrantUseClaims| c.token_ref.clear(),
            |c: &mut GrantUseClaims| c.rule_id = "  ".into(),
            |c: &mut GrantUseClaims| c.space_ref.clear(),
            |c: &mut GrantUseClaims| c.agent_subject_id.clear(),
            |c: &mut GrantUseClaims| c.action_key.clear(),
            |c: &mut GrantUseClaims| c.action_schema_hash.clear(),
            |c: &mut GrantUseClaims| c.nonce.clear(),
            |c: &mut GrantUseClaims| c.authority_revision = 0,
            |c: &mut GrantUseClaims| c.authority_revision = -1,
        ] {
            let mut claims = live_claims(now);
            edit(&mut claims);
            assert!(check(&claims).is_err());
        }
        // A run outside any Space has no Space a standing rule could belong to.
        assert!(
            check_claims(&live_claims(now), "org-1", "user-1", "run-1", "", "mcp__srv__tool", now).is_err()
        );
    }

    const KEY_ID: &str = "grant-test-key";

    fn key_set(signing: &SigningKey) -> BTreeMap<String, VerifyingKey> {
        BTreeMap::from([(KEY_ID.to_owned(), signing.verifying_key())])
    }

    fn payload(now: DateTime<Utc>) -> JsonValue {
        json!({
            "token_ref": "tok",
            "rule_id": "rule-1",
            "scope": "always",
            "run_id": "run-1",
            "org_id": "org-1",
            "space_ref": "space-1",
            "subject_id": "user-1",
            "agent_subject_id": "agent-1",
            "tool_key": "mcp__srv__tool",
            "action_key": "mcp__srv__tool",
            "action_schema_hash": "a".repeat(64),
            "authority_revision": 3,
            "audience": "model-plane",
            "issued_at": now,
            "expires_at": now + Duration::seconds(120),
            "nonce": "n",
        })
    }

    /// A proof as Control issues it: version, key id, claims, then the signature over the first
    /// three parts.
    fn sign(signing: &SigningKey, key_id: &str, payload: &JsonValue) -> String {
        let signed = format!(
            "{APPROVAL_GRANT_USE_VERSION}.{}.{}",
            URL_SAFE_NO_PAD.encode(key_id),
            URL_SAFE_NO_PAD.encode(payload.to_string())
        );
        let signature = signing.sign(signed.as_bytes());
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    fn open(token: &str, keys: BTreeMap<String, VerifyingKey>) -> Result<GrantUseClaims, Status> {
        open_signed_claims(token, || Ok(keys))
    }

    fn assert_denied(result: Result<GrantUseClaims, Status>, message: &str, case: &str) {
        let status = result.expect_err(case);
        assert_eq!(status.code(), tonic::Code::PermissionDenied, "{case}");
        assert_eq!(status.message(), message, "{case}");
    }

    #[test]
    fn a_proof_signed_by_the_trusted_key_opens_and_every_other_form_is_refused() {
        let trusted = SigningKey::from_bytes(&[41_u8; 32]);
        let forger = SigningKey::from_bytes(&[42_u8; 32]);
        let now = fixed_now();
        let good = sign(&trusted, KEY_ID, &payload(now));
        let opened = open(&good, key_set(&trusted)).expect("a proof signed by the trusted key opens");
        assert_eq!((opened.rule_id.as_str(), opened.run_id.as_str()), ("rule-1", "run-1"));

        // Forged: another key signs under the trusted key id.
        assert_denied(
            open(&sign(&forger, KEY_ID, &payload(now)), key_set(&trusted)),
            "invalid approval grant proof signature",
            "forged signature",
        );
        // Tampered: the claims change after they were signed.
        let parts: Vec<String> = good.split('.').map(str::to_owned).collect();
        let mut other_claims = payload(now);
        other_claims["tool_key"] = json!("mcp__other__tool");
        let mut tampered = parts.clone();
        tampered[2] = URL_SAFE_NO_PAD.encode(other_claims.to_string());
        assert_denied(
            open(&tampered.join("."), key_set(&trusted)),
            "invalid approval grant proof signature",
            "tampered claims",
        );
        let mut truncated = parts.clone();
        truncated[3] = URL_SAFE_NO_PAD.encode([0_u8; 8]);
        assert_denied(
            open(&truncated.join("."), key_set(&trusted)),
            "invalid approval grant proof signature",
            "a signature of the wrong length",
        );

        // A key id this service does not trust, or one that cannot be read at all.
        let untrusted = "untrusted Control Space decision key";
        assert_denied(
            open(&sign(&trusted, "rotated-away", &payload(now)), key_set(&trusted)),
            untrusted,
            "unknown key id",
        );
        for (name, key_part) in [("undecodable", "!!!".to_owned()), ("blank", URL_SAFE_NO_PAD.encode("  "))] {
            let mut broken = parts.clone();
            broken[1] = key_part;
            assert_denied(open(&broken.join("."), key_set(&trusted)), untrusted, name);
        }

        // The envelope: only this version, in four parts.
        let envelope = "invalid approval grant proof envelope";
        for (name, token) in [
            (
                "a thread decision's version word",
                good.replacen(APPROVAL_GRANT_USE_VERSION, "v1", 1),
            ),
            ("too few parts", parts[..3].join(".")),
            ("too many parts", format!("{good}.extra")),
            ("empty", String::new()),
        ] {
            assert_denied(open(&token, key_set(&trusted)), envelope, name);
        }
        let too_large = open(&"x".repeat(MAX_TOKEN_BYTES + 1), key_set(&trusted)).expect_err("oversize");
        assert_eq!(too_large.code(), tonic::Code::InvalidArgument);

        // Claims Control does not issue: an unknown claim, a missing one, or not claims at all.
        let claims_refusal = "invalid approval grant proof claims";
        let mut with_extra = payload(now);
        with_extra["extra"] = json!(true);
        let mut without_nonce = payload(now);
        without_nonce.as_object_mut().expect("object").remove("nonce");
        for (name, claims) in [
            ("an unknown claim", with_extra),
            ("a missing claim", without_nonce),
            ("not claims", json!("not claims")),
        ] {
            assert_denied(open(&sign(&trusted, KEY_ID, &claims), key_set(&trusted)), claims_refusal, name);
        }
    }

    #[test]
    fn the_key_set_is_loaded_only_after_the_envelope_parses() {
        // A malformed proof is refused as malformed even when no key is configured.
        let malformed = open_signed_claims("not a proof", || Err(Status::failed_precondition("keys unavailable")))
            .expect_err("envelope");
        assert_eq!(malformed.code(), tonic::Code::PermissionDenied);
        // A well-formed one reaches the key load and reports that failure.
        let trusted = SigningKey::from_bytes(&[41_u8; 32]);
        let good = sign(&trusted, KEY_ID, &payload(fixed_now()));
        let unconfigured = open_signed_claims(&good, || Err(Status::failed_precondition("keys unavailable")))
            .expect_err("keys");
        assert_eq!(unconfigured.code(), tonic::Code::FailedPrecondition);
    }

    #[test]
    fn a_signed_proof_for_another_run_tool_or_time_is_refused() {
        let trusted = SigningKey::from_bytes(&[41_u8; 32]);
        let now = fixed_now();
        // The two halves `verify_grant_use_token` runs, with the trusted key passed in.
        let verify = |token: &str, run_id: &str, tool_key: &str, at: DateTime<Utc>| {
            let claims = open_signed_claims(token, || Ok(key_set(&trusted)))?;
            check_claims(&claims, "org-1", "user-1", run_id, "space-1", tool_key, at)?;
            Ok::<_, Status>(claims)
        };
        let token = sign(&trusted, KEY_ID, &payload(now));
        verify(&token, "run-1", "mcp__srv__tool", now).expect("the call the rule was issued for");
        for (name, run_id, tool_key, at) in [
            ("another run", "run-2", "mcp__srv__tool", now),
            ("another tool", "run-1", "mcp__other__tool", now),
            ("a tool that is not an MCP tool", "run-1", "shell", now),
            ("after it expired", "run-1", "mcp__srv__tool", now + Duration::seconds(121)),
            ("before it was issued, beyond the skew", "run-1", "mcp__srv__tool", now - Duration::seconds(61)),
        ] {
            let status = verify(&token, run_id, tool_key, at).expect_err(name);
            assert_eq!(status.code(), tonic::Code::PermissionDenied, "{name}");
            assert_eq!(status.message(), "approval grant proof does not match this call", "{name}");
        }
    }

    fn lazy_pool() -> Pool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool")
    }

    #[tokio::test]
    async fn a_once_decision_is_never_refused_and_never_reads_the_row() {
        // The pool reaches nothing, so the call would fail if it queried at all.
        refuse_wide_scope_for_origin(&lazy_pool(), "appr_1", "org-1", "once")
            .await
            .expect("a once decision passes on any origin");
    }

    const TOOL_KEY: &str = "shell";

    fn call_digest() -> String {
        "ab".repeat(32)
    }

    async fn seed_thread(pool: &Pool, org: &str, user: &str, thread: &str) {
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
            .bind(thread)
            .bind(org)
            .bind(user)
            .execute(pool)
            .await
            .expect("seed thread");
    }

    async fn seed_plain_run(pool: &Pool, org: &str, user: &str, thread: &str, run: &str) {
        sqlx::query("INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'goal', $3, $4)")
            .bind(run)
            .bind(thread)
            .bind(org)
            .bind(user)
            .execute(pool)
            .await
            .expect("seed run");
    }

    async fn drop_org_rows(pool: &Pool, org: &str) {
        for sql in [
            "DELETE FROM approvals WHERE org_id = $1",
            "DELETE FROM runs WHERE org_id = $1",
            "DELETE FROM threads WHERE org_id = $1",
        ] {
            sqlx::query(sql).bind(org).execute(pool).await.ok();
        }
    }

    async fn try_claim(pool: &Pool, org: &str, user: &str, run: &str, tool: &str, digest: &str) -> Option<String> {
        claim_once(pool, org, user, run, tool, digest).await.expect("claim")
    }

    /// One org with a thread, the run that raised an approval, the run that retries the call, and
    /// the granted once-approval of user-1 for that exact call: the row `claim_once` is meant to
    /// find (card `resume_class` none, no continuation mode, a person's own turn).
    struct Gate {
        org: String,
        thread: String,
        retry_run: String,
        approval: String,
    }

    impl Gate {
        async fn seed(pool: &Pool, tag: &str) -> Self {
            let sfx = mp_ids::new_ulid();
            let gate = Self {
                org: format!("org-gate-{tag}-{sfx}"),
                thread: format!("thr-gate-{tag}-{sfx}"),
                retry_run: format!("run-retry-{tag}-{sfx}"),
                approval: format!("appr-gate-{tag}-{sfx}"),
            };
            let raised_by = format!("run-raised-{tag}-{sfx}");
            seed_thread(pool, &gate.org, "user-1", &gate.thread).await;
            seed_plain_run(pool, &gate.org, "user-1", &gate.thread, &raised_by).await;
            seed_plain_run(pool, &gate.org, "user-1", &gate.thread, &gate.retry_run).await;
            sqlx::query(
                "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id, metadata, decided_at, \
                 tool_key, action_digest) \
                 VALUES ($1, $2, 'tool_call', 'granted', $3, 'user-1', $4, now(), $5, $6)",
            )
            .bind(&gate.approval)
            .bind(&raised_by)
            .bind(&gate.org)
            .bind(json!({ "card": { "resume_class": "none" } }))
            .bind(TOOL_KEY)
            .bind(call_digest())
            .execute(pool)
            .await
            .expect("seed approval");
            gate
        }

        async fn claim(&self, pool: &Pool) -> Option<String> {
            try_claim(pool, &self.org, "user-1", &self.retry_run, TOOL_KEY, &call_digest()).await
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn approval_scope_two_concurrent_claim_once_calls_have_one_winner() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let gate = Gate::seed(&pool, "race").await;
        let claims: Vec<_> = (0..8)
            .map(|_| {
                let (pool, org, run) = (pool.clone(), gate.org.clone(), gate.retry_run.clone());
                tokio::spawn(async move { claim_once(&pool, &org, "user-1", &run, TOOL_KEY, &call_digest()).await })
            })
            .collect();
        let mut winners = Vec::new();
        for claim in claims {
            if let Some(id) = claim.await.expect("task").expect("claim") {
                winners.push(id);
            }
        }
        assert_eq!(winners, [gate.approval.clone()], "exactly one retry spends the grant");
        assert_eq!(gate.claim(&pool).await, None, "a spent grant is not found again");
        let spent: bool = sqlx::query_scalar("SELECT consumed_at IS NOT NULL FROM approvals WHERE id = $1")
            .bind(&gate.approval)
            .fetch_one(&pool)
            .await
            .expect("row");
        assert!(spent);
        drop_org_rows(&pool, &gate.org).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn approval_scope_claim_once_needs_the_same_thread_person_tool_digest_and_org() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let gate = Gate::seed(&pool, "match").await;
        let sfx = mp_ids::new_ulid();
        // Another thread: a run of the same person on a thread of its own.
        let (other_thread, other_run) = (format!("thr-other-{sfx}"), format!("run-other-{sfx}"));
        seed_thread(&pool, &gate.org, "user-1", &other_thread).await;
        seed_plain_run(&pool, &gate.org, "user-1", &other_thread, &other_run).await;
        // Another person: their own run on the approval's thread.
        let stranger_run = format!("run-stranger-{sfx}");
        seed_plain_run(&pool, &gate.org, "user-2", &gate.thread, &stranger_run).await;

        let digest = call_digest();
        let org = gate.org.as_str();
        let retry = gate.retry_run.as_str();
        assert_eq!(try_claim(&pool, org, "user-1", &other_run, TOOL_KEY, &digest).await, None, "another thread");
        assert_eq!(try_claim(&pool, org, "user-2", &stranger_run, TOOL_KEY, &digest).await, None, "another person");
        assert_eq!(try_claim(&pool, org, "user-1", retry, TOOL_KEY, &"cd".repeat(32)).await, None, "another digest");
        assert_eq!(try_claim(&pool, org, "user-1", retry, "other_tool", &digest).await, None, "another tool");
        assert_eq!(try_claim(&pool, "another-org", "user-1", retry, TOOL_KEY, &digest).await, None, "another org");
        assert_eq!(try_claim(&pool, org, "user-1", "run-missing", TOOL_KEY, &digest).await, None, "an unknown run");
        // None of those spent it: the exact call does, whatever case the digest is stated in.
        assert_eq!(
            try_claim(&pool, org, "user-1", retry, TOOL_KEY, &digest.to_ascii_uppercase()).await,
            Some(gate.approval.clone())
        );
        assert_eq!(gate.claim(&pool).await, None);
        drop_org_rows(&pool, &gate.org).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn approval_scope_claim_once_never_spends_a_row_it_may_not() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        // Each edit takes the baseline approval out of the retry's reach; `$1` is the approval.
        let edits: [(&str, &str); 11] = [
            ("expired", "UPDATE approvals SET expires_at = now() - interval '1 minute' WHERE id = $1"),
            ("already spent", "UPDATE approvals SET consumed_at = now() WHERE id = $1"),
            ("requested, not granted", "UPDATE approvals SET status = 'requested' WHERE id = $1"),
            ("denied", "UPDATE approvals SET status = 'denied' WHERE id = $1"),
            ("untrusted content", "UPDATE approvals SET kind = 'untrusted_content' WHERE id = $1"),
            (
                "a worker continuation",
                r#"UPDATE approvals SET metadata = '{"card":{"resume_class":"action_continuation"}}'::jsonb WHERE id = $1"#,
            ),
            (
                "a live poll",
                r#"UPDATE approvals SET metadata = '{"card":{"resume_class":"live_poll"}}'::jsonb WHERE id = $1"#,
            ),
            ("no card at all", "UPDATE approvals SET metadata = '{}'::jsonb WHERE id = $1"),
            ("a loop continuation", "UPDATE approvals SET continuation_mode = 'loop' WHERE id = $1"),
            ("an ambient turn", "UPDATE approvals SET turn_origin = 'ambient' WHERE id = $1"),
            (
                "an automation run",
                "UPDATE runs SET turn_origin = 'automation' WHERE id = (SELECT run_id FROM approvals WHERE id = $1)",
            ),
        ];
        for (name, edit) in edits {
            let gate = Gate::seed(&pool, "edit").await;
            sqlx::query(edit)
                .bind(&gate.approval)
                .execute(&pool)
                .await
                .expect("edit the approval");
            assert_eq!(gate.claim(&pool).await, None, "{name}");
            drop_org_rows(&pool, &gate.org).await;
        }
        // The unedited baseline is spent, so the refusals above are the edits and not the setup.
        let control = Gate::seed(&pool, "control").await;
        assert_eq!(control.claim(&pool).await, Some(control.approval.clone()));
        drop_org_rows(&pool, &control.org).await;
    }

    #[derive(sqlx::FromRow)]
    struct StoredGrant {
        status: String,
        decided_by: String,
        decision_reason: String,
        requested_by: String,
        tool_key: Option<String>,
        action_digest: Option<String>,
        turn_origin: Option<String>,
        grant_ref: Option<String>,
        decided: bool,
        unspent: bool,
        decision_source: Option<String>,
        decision_scope: Option<String>,
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn approval_scope_a_standing_rule_pass_is_recorded_granted_with_no_delivery() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let sfx = mp_ids::new_ulid();
        let (org, thread, run) = (
            format!("org-rule-{sfx}"),
            format!("thr-rule-{sfx}"),
            format!("run-rule-{sfx}"),
        );
        seed_thread(&pool, &org, "user-1", &thread).await;
        seed_plain_run(&pool, &org, "user-1", &thread, &run).await;
        let claims = GrantUseClaims {
            rule_id: "rule-7".into(),
            run_id: run.clone(),
            org_id: org.clone(),
            // Unique per run of the test: the index of migration 0136 is global.
            token_ref: format!("tok-rule-{sfx}"),
            ..claims()
        };
        let context = validate_gate_context("mcp__srv__tool", &call_digest(), "direct", false).expect("context");
        let metadata = json!({ "decision": standing_rule_decision(&claims.rule_id) });
        let first_id = format!("appr-rule-{sfx}");
        let approval = GrantedApproval {
            id: &first_id,
            run_id: &run,
            kind: "tool_call",
            requested_by: "agent-1",
            org_id: &org,
            user_id: "user-1",
            idempotency_key: "idem-1",
            metadata: &metadata,
            expires_at: None,
            context: &context,
            claims: &claims,
        };
        let created = insert_granted_by_rule(&pool, &approval).await.expect("insert");
        assert_eq!(created.as_deref(), Some(first_id.as_str()));

        let stored: StoredGrant = sqlx::query_as(
            "SELECT status, decided_by, decision_reason, requested_by, tool_key, action_digest, \
             turn_origin, grant_ref, (decided_at IS NOT NULL) AS decided, (consumed_at IS NULL) AS unspent, \
             metadata #>> '{decision,source}' AS decision_source, \
             metadata #>> '{decision,scope}' AS decision_scope \
             FROM approvals WHERE id = $1",
        )
        .bind(&first_id)
        .fetch_one(&pool)
        .await
        .expect("the approval row");
        assert_eq!(stored.status, "granted");
        assert_eq!(stored.decided_by, "grant:rule-7");
        assert_eq!(stored.decision_reason, "standing rule rule-7");
        assert_eq!(stored.requested_by, "agent-1");
        assert_eq!(stored.grant_ref.as_deref(), Some("rule-7"));
        assert_eq!(stored.tool_key.as_deref(), Some("mcp__srv__tool"));
        assert_eq!(stored.action_digest.as_deref(), Some(call_digest().as_str()));
        assert_eq!(stored.turn_origin.as_deref(), Some("direct"));
        assert!(stored.decided && stored.unspent);
        assert_eq!(stored.decision_source.as_deref(), Some("standing_grant"));
        assert_eq!(stored.decision_scope.as_deref(), Some("once"));
        let deliveries: i64 = sqlx::query_scalar("SELECT count(*) FROM approval_delivery_outbox WHERE approval_id = $1")
            .bind(&first_id)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(deliveries, 0, "a pass carries no descriptor, so no worker is queued");

        // The same step again (same idempotency key) writes nothing and says so.
        let second_id = format!("appr-rule-again-{sfx}");
        let again = GrantedApproval { id: &second_id, ..approval };
        assert_eq!(insert_granted_by_rule(&pool, &again).await.expect("insert"), None);

        // The same proof on ANOTHER step is a replay (migration 0136): refused, nothing written.
        let third_id = format!("appr-rule-replay-{sfx}");
        let replay = GrantedApproval { id: &third_id, idempotency_key: "idem-2", ..approval };
        let error = insert_granted_by_rule(&pool, &replay).await.expect_err("a spent proof is refused");
        assert!(is_grant_proof_replay(&error), "{error:#}");
        // A fresh proof for that step passes.
        let fresh_claims = GrantUseClaims { token_ref: format!("tok-rule-fresh-{sfx}"), ..claims.clone() };
        let fresh = GrantedApproval { id: &third_id, idempotency_key: "idem-2", claims: &fresh_claims, ..approval };
        assert_eq!(insert_granted_by_rule(&pool, &fresh).await.expect("insert").as_deref(), Some(third_id.as_str()));
        let stored_ref: Option<String> =
            sqlx::query_scalar("SELECT grant_use_token_ref FROM approvals WHERE id = $1")
                .bind(&first_id)
                .fetch_one(&pool)
                .await
                .expect("token ref");
        assert_eq!(stored_ref, Some(format!("tok-rule-{sfx}")));
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM approvals WHERE org_id = $1")
            .bind(&org)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(rows, 2);
        drop_org_rows(&pool, &org).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn approval_scope_wide_decisions_are_refused_only_on_turns_nobody_watched() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let sfx = mp_ids::new_ulid();
        let (org, thread, run) = (
            format!("org-origin-{sfx}"),
            format!("thr-origin-{sfx}"),
            format!("run-origin-{sfx}"),
        );
        seed_thread(&pool, &org, "user-1", &thread).await;
        seed_plain_run(&pool, &org, "user-1", &thread, &run).await;
        let id = |name: &str| format!("appr-origin-{name}-{sfx}");
        // (name, stored turn origin, a wide scope is refused)
        let origins = [
            ("ambient", Some("ambient"), true),
            ("automation", Some("automation"), true),
            ("direct", Some("direct"), false),
            ("legacy", None, false),
        ];
        for (name, origin, _) in origins {
            sqlx::query(
                "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id, turn_origin) \
                 VALUES ($1, $2, 'tool_call', 'requested', $3, 'user-1', $4)",
            )
            .bind(id(name))
            .bind(&run)
            .bind(&org)
            .bind(origin)
            .execute(&pool)
            .await
            .expect("seed approval");
        }
        for scope in ["thread", "run"] {
            for (name, _, refused) in origins {
                let result = refuse_wide_scope_for_origin(&pool, &id(name), &org, scope).await;
                if refused {
                    let status = result.expect_err("a wide scope on a turn nobody watched");
                    assert_eq!(status.code(), tonic::Code::PermissionDenied, "{name} {scope}");
                    assert_eq!(status.message(), SCOPE_NOT_PERMITTED, "{name} {scope}");
                    assert_eq!(
                        status
                            .metadata()
                            .get(ERROR_CODE_TRAILER)
                            .and_then(|value| value.to_str().ok()),
                        Some(SCOPE_NOT_PERMITTED),
                        "{name} {scope}"
                    );
                } else {
                    result.unwrap_or_else(|status| panic!("{name} {scope} passes: {status:?}"));
                }
            }
        }
        // A once decision passes on every origin, and an approval that is not found passes too.
        for (name, _, _) in origins {
            refuse_wide_scope_for_origin(&pool, &id(name), &org, "once")
                .await
                .expect("once passes");
        }
        refuse_wide_scope_for_origin(&pool, "appr-unknown", &org, "thread")
            .await
            .expect("an unknown approval passes");
        drop_org_rows(&pool, &org).await;
    }
}
