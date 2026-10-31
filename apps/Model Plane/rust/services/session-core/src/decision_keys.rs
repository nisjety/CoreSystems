//! Operability around the Control Space decision verification keys (F10/F11).
//!
//! In the W1 live proof a stale `CONTROL_SPACE_DECISION_KEY_ID` / public key in the Model Plane
//! deploy env overrode the Control ledger, so every Control-signed decision named a key id this
//! service did not trust and was refused with a bare `PermissionDenied`. The refusal is correct
//! and stays exactly as it was; what was missing is any signal that explained it. This module
//! adds that signal and nothing else:
//!
//! * [`boot_check`] logs the configured key ids with a short public-key fingerprint at start-up
//!   (never the key itself) and fails start-up when a key set is configured but unparseable.
//! * [`lookup_key`] is the single place a verifier resolves a token's key id. An unknown id gets
//!   the same refusal as before, plus an error-level log naming the token's id and the
//!   configured ids (once per minute per id) and a `mp_session_space_decision_unknown_key_total`
//!   counter.

use std::{
    collections::{BTreeMap, HashMap},
    env,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};
use tonic::Status;
use tracing::{error, info, warn};

/// At most one unknown-key log line per key id in this window.
const UNKNOWN_KEY_LOG_INTERVAL: Duration = Duration::from_secs(60);

/// Distinct unknown ids remembered by the limiter. Token key ids are caller-controlled, so the
/// map must not grow with them; at the cap, entries older than the window are dropped and, if
/// none are, the new id is not logged (the counter still counts it).
const UNKNOWN_KEY_LIMITER_CAPACITY: usize = 64;

/// Longest key id echoed into a log line.
const LOGGED_KEY_ID_MAX_CHARS: usize = 64;

/// The refusal for a token whose key id is not in the configured set. Unchanged by F11.
const UNTRUSTED_KEY_MESSAGE: &str = "untrusted Control Space decision key";

/// The env vars that configure the key set (see `configured_control_space_decision_keys`).
const KEY_ENV_VARS: [&str; 3] = [
    "CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON",
    "CONTROL_SPACE_DECISION_KEY_ID",
    "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64",
];

/// A short, stable, non-reversible tag for a public key: the first 8 bytes of
/// `sha256(raw public key)` as lowercase hex. Enough to compare two deployments' keys by eye.
pub(crate) fn fingerprint(key: &VerifyingKey) -> String {
    let digest = Sha256::digest(key.to_bytes());
    digest[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `id=fingerprint` pairs, comma separated, in key-id order.
pub(crate) fn format_key_set(keys: &BTreeMap<String, VerifyingKey>) -> String {
    keys.iter()
        .map(|(id, key)| format!("{id}={}", fingerprint(key)))
        .collect::<Vec<_>>()
        .join(",")
}

/// Just the configured ids, comma separated.
fn format_key_ids(keys: &BTreeMap<String, VerifyingKey>) -> String {
    keys.keys().cloned().collect::<Vec<_>>().join(",")
}

/// A caller-controlled key id made safe to log: control characters become `?` and the length
/// is capped.
pub(crate) fn sanitize_key_id(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .take(LOGGED_KEY_ID_MAX_CHARS)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    if raw.chars().count() > LOGGED_KEY_ID_MAX_CHARS {
        out.push_str("...");
    }
    out
}

/// Once-per-window gate, keyed by key id.
pub(crate) struct UnknownKeyLimiter {
    window: Duration,
    capacity: usize,
    last_logged: Mutex<HashMap<String, Instant>>,
}

impl UnknownKeyLimiter {
    pub(crate) fn new(window: Duration, capacity: usize) -> Self {
        Self {
            window,
            capacity,
            last_logged: Mutex::new(HashMap::new()),
        }
    }

    /// Whether a log line for `key_id` may be emitted at `now`; records it when it may.
    pub(crate) fn should_log(&self, key_id: &str, now: Instant) -> bool {
        let mut map = match self.last_logged.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(last) = map.get_mut(key_id) {
            if now.saturating_duration_since(*last) < self.window {
                return false;
            }
            *last = now;
            return true;
        }
        if map.len() >= self.capacity {
            map.retain(|_, last| now.saturating_duration_since(*last) < self.window);
            if map.len() >= self.capacity {
                return false;
            }
        }
        map.insert(key_id.to_owned(), now);
        true
    }
}

fn limiter() -> &'static UnknownKeyLimiter {
    static LIMITER: OnceLock<UnknownKeyLimiter> = OnceLock::new();
    LIMITER.get_or_init(|| {
        UnknownKeyLimiter::new(UNKNOWN_KEY_LOG_INTERVAL, UNKNOWN_KEY_LIMITER_CAPACITY)
    })
}

/// Resolve a decision token's key id against the configured set. Unknown ids are refused with
/// the unchanged `PermissionDenied`, and made visible (see the module docs).
pub(crate) fn lookup_key<'a>(
    keys: &'a BTreeMap<String, VerifyingKey>,
    token_key_id: &str,
) -> Result<&'a VerifyingKey, Status> {
    lookup_key_with(keys, token_key_id, limiter(), Instant::now())
}

fn lookup_key_with<'a>(
    keys: &'a BTreeMap<String, VerifyingKey>,
    token_key_id: &str,
    limiter: &UnknownKeyLimiter,
    now: Instant,
) -> Result<&'a VerifyingKey, Status> {
    if let Some(key) = keys.get(token_key_id) {
        return Ok(key);
    }
    // No id label: the id is caller-controlled and would make the series unbounded.
    metrics::counter!("mp_session_space_decision_unknown_key_total").increment(1);
    if limiter.should_log(token_key_id, now) {
        error!(
            token_key_id = %sanitize_key_id(token_key_id),
            configured_key_ids = %format_key_ids(keys),
            "Control Space decision signed with a key id this service does not trust; the \
             decision is refused. The deploy env (CONTROL_SPACE_DECISION_KEY_ID / \
             CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 / CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON) \
             probably carries a stale key that differs from the Control ledger"
        );
    }
    Err(Status::permission_denied(UNTRUSTED_KEY_MESSAGE))
}

fn key_env_is_set() -> bool {
    KEY_ENV_VARS
        .iter()
        .any(|name| env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

/// What start-up learned about the decision key configuration.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BootOutcome {
    /// Nothing is configured: Space decisions will be refused per request.
    NotConfigured,
    /// A valid set, summarised as `id=fingerprint,...`.
    Configured(String),
}

/// The pure half of [`boot_check`]: `configured` is whether any key env var carries a value,
/// `loaded` is what the loader returned for them. A configured-but-unparseable set is an error.
pub(crate) fn evaluate_boot(
    configured: bool,
    loaded: Result<BTreeMap<String, VerifyingKey>, Status>,
) -> Result<BootOutcome, String> {
    if !configured {
        return Ok(BootOutcome::NotConfigured);
    }
    match loaded {
        Ok(keys) => Ok(BootOutcome::Configured(format_key_set(&keys))),
        Err(status) => Err(format!(
            "Control Space decision key configuration is unusable: {}",
            status.message()
        )),
    }
}

/// Start-up check: log the configured key ids with fingerprints, warn when none are configured,
/// and fail start-up when a key set is configured but cannot be parsed.
pub(crate) fn boot_check() -> anyhow::Result<()> {
    let configured = key_env_is_set();
    let loaded = if configured {
        crate::grpc::configured_control_space_decision_keys()
    } else {
        Ok(BTreeMap::new())
    };
    match evaluate_boot(configured, loaded) {
        Ok(BootOutcome::Configured(summary)) => {
            info!(
                decision_keys = %summary,
                "Control Space decision verification keys configured (id=public-key fingerprint)"
            );
            Ok(())
        }
        Ok(BootOutcome::NotConfigured) => {
            warn!(
                "no Control Space decision verification key is configured; every Space decision \
                 will be refused"
            );
            Ok(())
        }
        Err(message) => {
            error!(%message, "refusing to start");
            Err(anyhow::anyhow!(message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn key(seed: u8) -> VerifyingKey {
        SigningKey::from_bytes(&[seed; 32]).verifying_key()
    }

    #[test]
    fn fingerprint_is_short_stable_and_not_the_key() {
        let a = key(1);
        assert_eq!(fingerprint(&a), fingerprint(&a));
        assert_ne!(fingerprint(&a), fingerprint(&key(2)));
        assert_eq!(fingerprint(&a).len(), 16);
        assert!(fingerprint(&a).chars().all(|c| c.is_ascii_hexdigit()));
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            a.to_bytes(),
        );
        assert!(!encoded.contains(&fingerprint(&a)));
    }

    #[test]
    fn key_set_is_listed_in_id_order_with_fingerprints() {
        let keys = BTreeMap::from([("b-old".to_owned(), key(2)), ("a-new".to_owned(), key(1))]);
        assert_eq!(
            format_key_set(&keys),
            format!("a-new={},b-old={}", fingerprint(&key(1)), fingerprint(&key(2)))
        );
        assert_eq!(format_key_ids(&keys), "a-new,b-old");
        assert_eq!(format_key_set(&BTreeMap::new()), "");
    }

    #[test]
    fn logged_key_ids_are_capped_and_stripped_of_control_characters() {
        assert_eq!(sanitize_key_id("control-key-1"), "control-key-1");
        assert_eq!(sanitize_key_id("a\nb\u{1b}c"), "a?b?c");
        let long = "x".repeat(200);
        let shown = sanitize_key_id(&long);
        assert_eq!(shown.chars().count(), LOGGED_KEY_ID_MAX_CHARS + 3);
        assert!(shown.ends_with("..."));
    }

    #[test]
    fn limiter_allows_one_line_per_id_per_window() {
        let limiter = UnknownKeyLimiter::new(Duration::from_secs(60), 8);
        let t0 = Instant::now();
        assert!(limiter.should_log("stale", t0));
        assert!(!limiter.should_log("stale", t0 + Duration::from_secs(1)));
        assert!(!limiter.should_log("stale", t0 + Duration::from_secs(59)));
        // A different id has its own window.
        assert!(limiter.should_log("other", t0 + Duration::from_secs(2)));
        // After the window the first id may log again, and the window restarts.
        assert!(limiter.should_log("stale", t0 + Duration::from_secs(61)));
        assert!(!limiter.should_log("stale", t0 + Duration::from_secs(62)));
    }

    #[test]
    fn limiter_memory_is_bounded_by_capacity() {
        let limiter = UnknownKeyLimiter::new(Duration::from_secs(60), 2);
        let t0 = Instant::now();
        assert!(limiter.should_log("a", t0));
        assert!(limiter.should_log("b", t0));
        // Full and nothing has aged out: a third distinct id is not logged, and not stored.
        assert!(!limiter.should_log("c", t0 + Duration::from_secs(1)));
        assert_eq!(limiter.last_logged.lock().expect("lock").len(), 2);
        // Once the old entries age out the new id gets a slot.
        assert!(limiter.should_log("c", t0 + Duration::from_secs(61)));
    }

    #[test]
    fn lookup_returns_a_configured_key_and_refuses_an_unknown_id_unchanged() {
        let keys = BTreeMap::from([("control-1".to_owned(), key(1))]);
        let limiter = UnknownKeyLimiter::new(Duration::from_secs(60), 8);
        let now = Instant::now();
        let found = lookup_key_with(&keys, "control-1", &limiter, now).expect("known id resolves");
        assert_eq!(found.to_bytes(), key(1).to_bytes());
        // A known id never touches the limiter.
        assert!(limiter.last_logged.lock().expect("lock").is_empty());

        let refusal = lookup_key_with(&keys, "stale-id", &limiter, now).expect_err("unknown id");
        assert_eq!(refusal.code(), tonic::Code::PermissionDenied);
        assert_eq!(refusal.message(), "untrusted Control Space decision key");
        // The refusal logged (consumed the window) for that id only.
        assert!(!limiter.should_log("stale-id", now));
        assert!(limiter.should_log("another", now));
    }

    #[test]
    fn boot_reports_unconfigured_configured_and_unparseable_sets() {
        assert_eq!(
            evaluate_boot(false, Ok(BTreeMap::new())),
            Ok(BootOutcome::NotConfigured)
        );
        let keys = BTreeMap::from([("control-1".to_owned(), key(1))]);
        assert_eq!(
            evaluate_boot(true, Ok(keys)),
            Ok(BootOutcome::Configured(format!(
                "control-1={}",
                fingerprint(&key(1))
            )))
        );
        let failure = evaluate_boot(
            true,
            Err(Status::failed_precondition(
                "Control Space decision public key is invalid",
            )),
        )
        .expect_err("a configured but unparseable key set must fail boot");
        assert!(failure.contains("Control Space decision public key is invalid"));
        // A failure to load is irrelevant when nothing was configured.
        assert_eq!(
            evaluate_boot(false, Err(Status::failed_precondition("x"))),
            Ok(BootOutcome::NotConfigured)
        );
    }
}
