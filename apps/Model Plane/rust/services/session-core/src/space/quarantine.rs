//! Held tool output and its single-use release (build plan WS10-S8).
//!
//! A Space run's external tool result that matches a prompt-injection marker is, when the Space
//! holds quarantine in `enforce`, kept here instead of reaching the model:
//!
//! - [`hold`]: the content is encrypted with the continuation cipher (AAD names the approval, org
//!   and person) into `quarantined_outputs` (migration 0082), against an `UNTRUSTED_CONTENT`
//!   approval the caller raised first. Never under zero data retention, never outside a Space.
//! - [`claim_release`]: after a person grants the approval, the retry of the exact call (same tool,
//!   same action digest, same thread, same person) is handed the content once. One transaction
//!   with `FOR UPDATE SKIP LOCKED`, so two concurrent retries have one winner. The ciphertext is
//!   blanked in the same transaction.
//! - [`preview`]: an inert plain-text excerpt (at most 1 KiB) for the person deciding, and no one
//!   else. Never markup, never links, control and bidi characters removed.
//! - Deny, timeout, release and expiry blank the ciphertext (`PURGE_FOR_APPROVAL_SQL` runs inside
//!   the decide transaction; [`purge_expired`] runs before every claim and hold).
//!
//! Nothing here is authority: the approval decides, and the claim only checks that the approval
//! the person granted is the one the retry names.

use chrono::{Duration, Utc};
use tonic::Status;

use crate::continuation_crypto::DescriptorCipher;
use crate::store::Pool;

/// Longest content that can be held. Over it, the caller withholds without holding.
pub(crate) const MAX_HELD_BYTES: usize = 1024 * 1024;
/// A hold lives at most this long (the approval's own expiry can shorten it).
const HOLD_LIFETIME_HOURS: i64 = 24;
/// Longest preview handed to the deciding person.
pub(crate) const PREVIEW_MAX_BYTES: usize = 1024;
/// The approval kind that gates a hold, as stored in `approvals.kind`.
pub(crate) const KIND: &str = "untrusted_content";

/// Blank the ciphertext of one approval's hold. Used inside the decide transaction when the
/// decision is `denied` or `timed_out`.
pub(crate) const PURGE_FOR_APPROVAL_SQL: &str = "UPDATE quarantined_outputs \
     SET ciphertext = '', purged_at = now() \
     WHERE approval_id = $1 AND org_id = $2 AND purged_at IS NULL";

fn aad(approval_id: &str, org_id: &str, user_id: &str) -> String {
    format!("quarantine:{approval_id}:org:{org_id}:user:{user_id}")
}

fn is_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_plain_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-' | b'/'))
}

fn internal(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

/// Blank every hold whose own expiry has passed. Cheap (partial index) and idempotent.
pub(crate) async fn purge_expired(pool: &Pool) -> Result<(), Status> {
    sqlx::query(
        "UPDATE quarantined_outputs SET ciphertext = '', purged_at = now() \
         WHERE purged_at IS NULL AND expires_at <= now()",
    )
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(internal)
}

/// Everything a hold needs; the caller has already been authenticated and bound to the run.
pub(crate) struct HoldInput<'a> {
    pub approval_id: &'a str,
    pub run_id: &'a str,
    pub org_id: &'a str,
    pub user_id: &'a str,
    pub tool_key: &'a str,
    pub action_digest: &'a str,
    pub trust_class: &'a str,
    pub content_hash: &'a str,
    pub content: &'a str,
}

/// Keep `content` against its `UNTRUSTED_CONTENT` approval. Idempotent per approval: a repeat
/// keeps the first content. Refused (typed) when the approval is not a requested quarantine
/// approval of this person on this run, when the run is not in a Space, or when the content is too
/// large to hold.
pub(crate) async fn hold(pool: &Pool, input: &HoldInput<'_>) -> Result<(), Status> {
    if !is_plain_identifier(input.tool_key)
        || !is_digest(input.action_digest)
        || !is_plain_identifier(input.trust_class)
        || input.content_hash.is_empty()
        || input.content_hash.len() > 128
    {
        return Err(Status::invalid_argument("quarantine hold fields are invalid"));
    }
    if input.content.is_empty() || input.content.len() > MAX_HELD_BYTES {
        return Err(Status::invalid_argument("quarantine_content_size_unsupported"));
    }
    let approval: Option<(String, String, String, String, String, Option<chrono::DateTime<Utc>>)> =
        sqlx::query_as(
            "SELECT kind, status, run_id, org_id, user_id, expires_at FROM approvals WHERE id = $1",
        )
        .bind(input.approval_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    let Some((kind, status, run_id, org_id, user_id, approval_expires)) = approval else {
        return Err(Status::not_found("approval not found"));
    };
    if kind != KIND
        || status != "requested"
        || run_id != input.run_id
        || org_id != input.org_id
        || user_id != input.user_id
    {
        return Err(Status::failed_precondition(
            "approval is not a requested quarantine approval of this run",
        ));
    }
    let run_space: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(space_id, '') FROM runs WHERE id = $1 AND org_id = $2 AND user_id = $3",
    )
    .bind(input.run_id)
    .bind(input.org_id)
    .bind(input.user_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    if run_space.as_deref().unwrap_or_default().is_empty() {
        return Err(Status::failed_precondition(
            "quarantine holds exist only for runs in a Space",
        ));
    }
    purge_expired(pool).await?;
    let mut expires_at = Utc::now() + Duration::hours(HOLD_LIFETIME_HOURS);
    if let Some(approval_expires) = approval_expires {
        expires_at = expires_at.min(approval_expires);
    }
    let ciphertext = DescriptorCipher::from_env()
        .and_then(|cipher| {
            cipher.seal(
                &aad(input.approval_id, input.org_id, input.user_id),
                input.content,
            )
        })
        .map_err(|error| {
            tracing::error!(%error, "quarantine hold: could not seal the content");
            Status::unavailable("quarantine_cipher_unavailable")
        })?;
    sqlx::query(
        "INSERT INTO quarantined_outputs \
         (approval_id, run_id, org_id, user_id, tool_key, action_digest, trust_class, \
          content_hash, ciphertext, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (approval_id) DO NOTHING",
    )
    .bind(input.approval_id)
    .bind(input.run_id)
    .bind(input.org_id)
    .bind(input.user_id)
    .bind(input.tool_key)
    .bind(input.action_digest)
    .bind(input.trust_class)
    .bind(input.content_hash)
    .bind(&ciphertext)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(())
}

/// A released hold.
pub(crate) struct Released {
    pub approval_id: String,
    pub content: String,
    pub content_hash: String,
}

/// Hand the content of a granted, unspent, unexpired hold to the retry of the exact call, once.
///
/// Matches the same person, tool and action digest on the same thread as the retrying run. The
/// transaction locks the row (`SKIP LOCKED`, so a concurrent claim sees nothing), opens the
/// ciphertext, marks the hold consumed and blanks the ciphertext before it commits.
pub(crate) async fn claim_release(
    pool: &Pool,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    tool_key: &str,
    action_digest: &str,
) -> Result<Option<Released>, Status> {
    purge_expired(pool).await?;
    let mut transaction = pool.begin().await.map_err(internal)?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT q.approval_id, q.ciphertext, q.content_hash \
         FROM quarantined_outputs q \
         JOIN approvals a ON a.id = q.approval_id \
         JOIN runs hr ON hr.id = q.run_id \
         WHERE q.org_id = $1 AND q.user_id = $2 AND q.tool_key = $3 AND q.action_digest = $4 \
           AND a.kind = 'untrusted_content' AND a.status = 'granted' \
           AND q.consumed_at IS NULL AND q.purged_at IS NULL AND q.expires_at > now() \
           AND hr.thread_id = (SELECT thread_id FROM runs WHERE id = $5 AND org_id = $1 AND user_id = $2) \
         ORDER BY a.decided_at DESC NULLS LAST \
         LIMIT 1 \
         FOR UPDATE OF q SKIP LOCKED",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(tool_key)
    .bind(action_digest.to_ascii_lowercase())
    .bind(run_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(internal)?;
    let Some((approval_id, ciphertext, content_hash)) = row else {
        return Ok(None);
    };
    let content = DescriptorCipher::from_env()
        .and_then(|cipher| cipher.open(&aad(&approval_id, org_id, user_id), &ciphertext))
        .map_err(|error| {
            tracing::error!(%error, approval_id = %approval_id, "quarantine release: could not open the content");
            Status::unavailable("quarantine_cipher_unavailable")
        })?;
    sqlx::query(
        "UPDATE quarantined_outputs \
         SET consumed_at = now(), released_at = now(), purged_at = now(), ciphertext = '' \
         WHERE approval_id = $1",
    )
    .bind(&approval_id)
    .execute(&mut *transaction)
    .await
    .map_err(internal)?;
    transaction.commit().await.map_err(internal)?;
    Ok(Some(Released { approval_id, content, content_hash }))
}

/// The inert preview of a hold.
pub(crate) struct Preview {
    pub text: String,
    pub truncated: bool,
    pub tool_key: String,
    pub trust_class: String,
}

/// Characters that never reach a preview: control characters other than newline and tab, and the
/// invisible and bidirectional-override characters that can reorder or hide what is shown.
fn is_inert_hostile(c: char) -> bool {
    (c.is_control() && c != '\n' && c != '\t')
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// Plain text of at most [`PREVIEW_MAX_BYTES`], cut on a character boundary. The bool is true when
/// something was cut.
pub(crate) fn inert_preview(content: &str) -> (String, bool) {
    let mut text = String::with_capacity(PREVIEW_MAX_BYTES);
    let mut truncated = false;
    for c in content.chars().filter(|c| !is_inert_hostile(*c)) {
        if text.len() + c.len_utf8() > PREVIEW_MAX_BYTES {
            truncated = true;
            break;
        }
        text.push(c);
    }
    (text, truncated)
}

/// The preview for the person deciding. Only the run's owner, and only while the approval is
/// addressed to the requester (the default), requested or granted-and-unspent, and the hold is
/// live. Every other case is the same `not_found`, so the answer never says which part failed.
pub(crate) async fn preview(
    pool: &Pool,
    org_id: &str,
    user_id: &str,
    approval_id: &str,
) -> Result<Preview, Status> {
    purge_expired(pool).await?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT q.ciphertext, q.tool_key, q.trust_class \
         FROM quarantined_outputs q \
         JOIN approvals a ON a.id = q.approval_id \
         WHERE q.approval_id = $1 AND q.org_id = $2 AND q.user_id = $3 \
           AND a.kind = 'untrusted_content' AND a.status IN ('requested', 'granted') \
           AND a.addressee_kind = 'requester' \
           AND q.consumed_at IS NULL AND q.purged_at IS NULL AND q.expires_at > now()",
    )
    .bind(approval_id)
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some((ciphertext, tool_key, trust_class)) = row else {
        return Err(Status::not_found("quarantine_preview_unavailable"));
    };
    let content = DescriptorCipher::from_env()
        .and_then(|cipher| cipher.open(&aad(approval_id, org_id, user_id), &ciphertext))
        .map_err(|error| {
            tracing::error!(%error, approval_id, "quarantine preview: could not open the content");
            Status::unavailable("quarantine_cipher_unavailable")
        })?;
    let (text, truncated) = inert_preview(&content);
    Ok(Preview { text, truncated, tool_key, trust_class })
}

/// Validate the fields every claim shares.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_claim(tool_key: &str, action_digest: &str) -> Result<(), Status> {
    if is_plain_identifier(tool_key) && is_digest(&action_digest.to_ascii_lowercase()) {
        Ok(())
    } else {
        Err(Status::invalid_argument("tool_key and action_digest are required"))
    }
}
