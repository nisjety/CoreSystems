//! Limits and the small shared stores behind `POST /v1/rerank` (QB-T05a).
//!
//! The numbers are the contract Quarry codes against. Changing one is a wire change and
//! must move the goldens under `tests/fixtures/contracts/rerank-v1/` with it.

use std::time::Duration;

use super::UnitBudget;

/// The sub-router's own body limit, applied on top of the 10 MiB engine-wide limit.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_DOCUMENTS: usize = 100;
/// Longer candidate texts are cut at this many characters, never mid-character, and the
/// document id is listed in the response's `truncated_ids`.
pub const MAX_TEXT_CHARS: usize = 4_000;
pub const MAX_QUERY_CHARS: usize = 1_024;
pub const MAX_DOCUMENT_ID_BYTES: usize = 256;
pub const REQUEST_ID_MIN_CHARS: usize = 8;
pub const REQUEST_ID_MAX_CHARS: usize = 64;
pub const DEADLINE_MIN_MS: u64 = 100;
pub const DEADLINE_MAX_MS: u64 = 8_000;
pub const DEADLINE_DEFAULT_MS: u64 = 2_000;
/// Replay window of the per-replica cache. Known gap: the cache is not shared, so a replay
/// that lands on another replica, or after this window, makes a second provider call. The
/// ledger still records that call once, because cost-core's unique index keys on the same
/// idempotency key, but the provider charges for it. A shared replay store is a follow-up.
pub const REPLAY_TTL: Duration = Duration::from_secs(120);
pub const REPLAY_CAPACITY: u64 = 10_000;
/// The cost ledger's idempotency key limit (the Go verifier refuses a longer key).
pub const IDEMPOTENCY_KEY_MAX_BYTES: usize = 256;

/// Cut `text` to at most `max` characters. Returns the kept prefix and whether it was cut.
#[must_use]
pub fn truncate_chars(text: &str, max: usize) -> (&str, bool) {
    match text.char_indices().nth(max) {
        Some((byte_index, _)) => (&text[..byte_index], true),
        None => (text, false),
    }
}

/// `8..=64` characters from `[A-Za-z0-9._:-]`. The id reaches cost-core and the logs, so
/// nothing that could carry text or a separator is accepted.
#[must_use]
pub fn valid_request_id(id: &str) -> bool {
    let chars = id.chars().count();
    (REQUEST_ID_MIN_CHARS..=REQUEST_ID_MAX_CHARS).contains(&chars)
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

/// One to 256 bytes, no control characters.
#[must_use]
pub fn valid_document_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_DOCUMENT_ID_BYTES && !id.chars().any(char::is_control)
}

/// The cost idempotency key. The composed form is `rerank:{org}:{request_id}:{digest}`; a
/// pathological org id that would exceed the ledger's limit is replaced by a hash of the
/// whole key, which keeps the key deterministic and unique per (org, request, content).
#[must_use]
pub fn idempotency_key(org_id: &str, request_id: &str, digest: &str) -> String {
    let key = format!("rerank:{org_id}:{request_id}:{digest}");
    if key.len() <= IDEMPOTENCY_KEY_MAX_BYTES {
        key
    } else {
        format!("rerank:{}", blake3::hash(key.as_bytes()).to_hex())
    }
}

/// The shared daily counter of billed web-rerank units per org. Alert-only: the caller
/// compares the total to the budget and records an alert, it never stops a call.
/// Dragonfly-backed, so every replica counts into the same bucket. Fails open: an
/// unreachable backend answers `None`, which the caller treats as "not counted".
pub struct DailyUnitBudget {
    redis_url: String,
    conn: tokio::sync::OnceCell<redis::aio::ConnectionManager>,
}

/// Two days: a day's bucket outlives its day by enough to be read by a late report.
const BUDGET_KEY_TTL_SECONDS: i64 = 2 * 24 * 60 * 60;
const BUDGET_BACKEND_TIMEOUT: Duration = Duration::from_millis(250);

impl DailyUnitBudget {
    #[must_use]
    pub fn new(redis_url: String) -> Self {
        Self {
            redis_url,
            conn: tokio::sync::OnceCell::new(),
        }
    }

    async fn connection(&self) -> anyhow::Result<redis::aio::ConnectionManager> {
        let conn = self
            .conn
            .get_or_try_init(|| async {
                let client = redis::Client::open(self.redis_url.as_str())?;
                redis::aio::ConnectionManager::new(client).await
            })
            .await?;
        Ok(conn.clone())
    }
}

#[async_trait::async_trait]
impl UnitBudget for DailyUnitBudget {
    async fn add_units(&self, org_id: &str, day: &str, units: u64) -> Option<u64> {
        let key = format!("dpv2:webrerank:units:{org_id}:{day}");
        let work = async {
            let mut conn = self.connection().await?;
            let total: u64 = redis::cmd("INCRBY")
                .arg(&key)
                .arg(units)
                .query_async(&mut conn)
                .await?;
            let _: i64 = redis::cmd("EXPIRE")
                .arg(&key)
                .arg(BUDGET_KEY_TTL_SECONDS)
                .query_async(&mut conn)
                .await?;
            Ok::<u64, anyhow::Error>(total)
        };
        match tokio::time::timeout(BUDGET_BACKEND_TIMEOUT, work).await {
            Ok(Ok(total)) => Some(total),
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "web rerank unit budget unavailable; not counted");
                None
            }
            Err(_) => {
                tracing::warn!("web rerank unit budget timed out; not counted");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_cuts_at_a_character_boundary() {
        let text = "é".repeat(4_001);
        let (kept, cut) = truncate_chars(&text, MAX_TEXT_CHARS);
        assert!(cut);
        assert_eq!(kept.chars().count(), 4_000);
        assert_eq!(truncate_chars("short", MAX_TEXT_CHARS), ("short", false));
        assert_eq!(truncate_chars("exact", 5), ("exact", false));
    }

    #[test]
    fn request_ids_are_eight_to_sixty_four_safe_characters() {
        assert!(valid_request_id("abcdefgh"));
        assert!(valid_request_id(&"a".repeat(64)));
        assert!(valid_request_id("quarry:run.2026-10-11_0001"));
        assert!(!valid_request_id("abcdefg"));
        assert!(!valid_request_id(&"a".repeat(65)));
        assert!(!valid_request_id("abc def/ghi"));
        assert!(!valid_request_id("abcdefgh\n"));
        assert!(!valid_request_id("café-ids"));
    }

    #[test]
    fn document_ids_are_bounded_and_printable() {
        assert!(valid_document_id("doc-1"));
        assert!(valid_document_id(&"d".repeat(256)));
        assert!(!valid_document_id(""));
        assert!(!valid_document_id(&"d".repeat(257)));
        assert!(!valid_document_id("doc\n1"));
    }

    #[test]
    fn the_idempotency_key_is_composed_and_bounded() {
        let digest = "a".repeat(64);
        assert_eq!(
            idempotency_key("org-1", "rr-req-00001", &digest),
            format!("rerank:org-1:rr-req-00001:{digest}")
        );
        let huge_org = "o".repeat(300);
        let key = idempotency_key(&huge_org, "rr-req-00001", &digest);
        assert!(key.len() <= IDEMPOTENCY_KEY_MAX_BYTES, "{}", key.len());
        assert_eq!(
            key,
            idempotency_key(&huge_org, "rr-req-00001", &digest),
            "deterministic"
        );
    }
}
