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
use ed25519_dalek::{Signature, Verifier};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
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
    let keys = crate::grpc::configured_control_space_decision_keys()?;
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
    let claims: GrantUseClaims = serde_json::from_slice(&payload)
        .map_err(|_| Status::permission_denied("invalid approval grant proof claims"))?;
    check_claims(&claims, org_id, user_id, run_id, run_space_id, tool_key, now)?;
    Ok(claims)
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
         tool_key, action_digest, turn_origin, grant_ref) \
         VALUES ($1, $2, NULL, $3, 'granted', $4, $5, $6, $7, $8, $9, $10, now(), $11, $12, $13, $14, $15) \
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
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
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
}
