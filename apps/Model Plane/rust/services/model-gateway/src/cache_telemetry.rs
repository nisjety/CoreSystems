//! Advisory prompt-cache miss classification (build plan WS02-S2).
//!
//! The provider tells the gateway how many input tokens it served from its
//! prompt cache, never why a turn missed. This module guesses the reason from
//! what the gateway itself saw on the previous turn of the same thread, so the
//! Insights view can show "why is my hit rate low" next to the hit rate.
//!
//! It is advisory and lossy on purpose:
//! - State is per replica, in memory. Behind a load balancer a thread that moves
//!   replica reads as `no_baseline`; a restart forgets everything. Nothing here
//!   is persisted or shared.
//! - Only a 16-byte blake3 digest of the leading system messages is kept, never
//!   text, and never for a ZDR turn (the caller skips those).
//! - The result rides the usage envelope as one closed-set token that cost-core
//!   stores beside the cache token counts. A wrong guess changes a label, not a
//!   bill or an answer.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::ChatMessage;

/// Threads remembered per replica. The oldest-touched entry is evicted first.
const CAPACITY: usize = 4096;

/// How long a provider keeps an idle prefix. Anthropic's default cache lifetime
/// is five minutes; OpenAI evicts after five to ten idle minutes. The shorter
/// bound is used so a gap beyond it is called `ttl_expired`, not `unexplained`.
const PREFIX_TTL: Duration = Duration::from_secs(300);

/// Why a turn that could have read from the cache did not. The wire tokens are
/// the closed set cost-core validates (`ledger.CacheMissReasons`); keep both in
/// step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissReason {
    /// The prompt was shorter than the provider's minimum cacheable prefix.
    BelowMinimum,
    /// The leading system messages differ from the previous turn's.
    PrefixChanged,
    /// Same prefix as the previous turn, but the previous turn was too long ago.
    TtlExpired,
    /// This replica has not seen the thread (first turn, restart, other replica).
    NoBaseline,
    /// Same prefix, recent baseline, still no read.
    Unexplained,
}

impl MissReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BelowMinimum => "below_minimum",
            Self::PrefixChanged => "prefix_changed",
            Self::TtlExpired => "ttl_expired",
            Self::NoBaseline => "no_baseline",
            Self::Unexplained => "unexplained",
        }
    }
}

/// Digest of the part of a request a provider caches: the leading run of system
/// messages. Role and length are mixed in so two messages cannot be re-split
/// into the same bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixDigest([u8; 16]);

impl PrefixDigest {
    pub fn of(messages: &[ChatMessage]) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"verevon.cache-telemetry.v1");
        for message in messages.iter().take_while(|m| m.role == "system") {
            hasher.update(&(message.content.len() as u64).to_le_bytes());
            hasher.update(message.content.as_bytes());
        }
        let mut out = [0u8; 16];
        out.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        Self(out)
    }
}

/// What the gateway knows about one finished turn.
pub struct Observation<'a> {
    pub org_id: &'a str,
    pub thread_id: &'a str,
    pub model: &'a str,
    pub input_tokens: u32,
    pub cache_read_tokens: u32,
    pub prefix: PrefixDigest,
}

/// Smallest prefix the provider will cache for this model, mirroring the
/// adapter's own table in inference-core (`min_cacheable_tokens`). A copy rather
/// than a call: the gateway reaches inference-core over gRPC only.
fn min_cacheable_tokens(model: &str) -> u32 {
    let model = model.to_ascii_lowercase();
    if model.contains("haiku") {
        2_048
    } else if model.contains("opus-4-5")
        || model.contains("opus-4.5")
        || model.contains("opus-4-6")
        || model.contains("opus-4.6")
    {
        4_096
    } else if model.contains("opus-4-7")
        || model.contains("opus-4.7")
        || model.contains("mythos-preview")
    {
        2_048
    } else {
        1_024
    }
}

struct Baseline {
    prefix: PrefixDigest,
    seen_at: Instant,
    touched: u64,
}

#[derive(Default)]
struct State {
    baselines: HashMap<[u8; 16], Baseline>,
    tick: u64,
}

/// Per-replica store of the last prefix seen per (org, thread, model).
pub struct CacheTelemetry {
    state: Mutex<State>,
    capacity: usize,
}

impl CacheTelemetry {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(State::default()),
            capacity: capacity.max(1),
        }
    }

    /// Classify a finished turn and remember its prefix as the next baseline.
    /// `None` when the turn read from the cache or reported no input at all.
    pub fn classify(&self, obs: &Observation<'_>, now: Instant) -> Option<MissReason> {
        let key = baseline_key(obs);
        let Ok(mut state) = self.state.lock() else {
            // A poisoned lock only costs this advisory label.
            return None;
        };
        state.tick += 1;
        let tick = state.tick;
        let previous = state
            .baselines
            .insert(
                key,
                Baseline {
                    prefix: obs.prefix,
                    seen_at: now,
                    touched: tick,
                },
            )
            .map(|b| (b.prefix, b.seen_at));
        if previous.is_none() && state.baselines.len() > self.capacity {
            if let Some(oldest) = state
                .baselines
                .iter()
                .min_by_key(|(_, b)| b.touched)
                .map(|(k, _)| *k)
            {
                state.baselines.remove(&oldest);
            }
        }
        if obs.input_tokens == 0 || obs.cache_read_tokens > 0 {
            return None;
        }
        if obs.input_tokens < min_cacheable_tokens(obs.model) {
            return Some(MissReason::BelowMinimum);
        }
        Some(match previous {
            None => MissReason::NoBaseline,
            Some((prefix, _)) if prefix != obs.prefix => MissReason::PrefixChanged,
            Some((_, seen_at)) if now.saturating_duration_since(seen_at) > PREFIX_TTL => {
                MissReason::TtlExpired
            }
            Some(_) => MissReason::Unexplained,
        })
    }
}

fn baseline_key(obs: &Observation<'_>) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    for part in [obs.org_id, obs.thread_id, obs.model] {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    out
}

/// The turn's miss reason for the usage envelope, or `None`. `prefix` is `None`
/// for a turn that must not be observed (ZDR, or a subscription route whose
/// tokens are not cache spend), which then leaves no digest behind either.
pub fn miss_reason_for_turn(
    prefix: Option<PrefixDigest>,
    org_id: &str,
    thread_id: &str,
    model: &str,
    input_tokens: u32,
    cache_read_tokens: u32,
) -> Option<&'static str> {
    let prefix = prefix?;
    global()
        .classify(
            &Observation {
                org_id,
                thread_id,
                model,
                input_tokens,
                cache_read_tokens,
                prefix,
            },
            Instant::now(),
        )
        .map(MissReason::as_str)
}

/// The replica's shared instance. Not on `AppState`: it is a process-local,
/// advisory cache and nothing else needs to reach it.
pub fn global() -> &'static CacheTelemetry {
    static GLOBAL: OnceLock<CacheTelemetry> = OnceLock::new();
    GLOBAL.get_or_init(|| CacheTelemetry::new(CAPACITY))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(text: &str) -> ChatMessage {
        ChatMessage {
            role: "system".to_owned(),
            content: text.to_owned(),
            ..Default::default()
        }
    }

    fn user(text: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_owned(),
            content: text.to_owned(),
            ..Default::default()
        }
    }

    fn turn<'a>(thread: &'a str, input: u32, read: u32, prefix: PrefixDigest) -> Observation<'a> {
        Observation {
            org_id: "org",
            thread_id: thread,
            model: "claude-sonnet-4-5",
            input_tokens: input,
            cache_read_tokens: read,
            prefix,
        }
    }

    #[test]
    fn the_wire_tokens_are_the_closed_set_cost_core_accepts() {
        let all = [
            MissReason::BelowMinimum,
            MissReason::PrefixChanged,
            MissReason::TtlExpired,
            MissReason::NoBaseline,
            MissReason::Unexplained,
        ];
        let tokens: Vec<&str> = all.iter().map(|r| r.as_str()).collect();
        assert_eq!(
            tokens,
            [
                "below_minimum",
                "prefix_changed",
                "ttl_expired",
                "no_baseline",
                "unexplained"
            ]
        );
    }

    #[test]
    fn a_read_is_a_hit_and_carries_no_reason() {
        let t = CacheTelemetry::new(8);
        let p = PrefixDigest::of(&[system("a")]);
        assert_eq!(t.classify(&turn("th", 5000, 4000, p), Instant::now()), None);
    }

    #[test]
    fn a_first_long_turn_is_no_baseline() {
        let t = CacheTelemetry::new(8);
        let p = PrefixDigest::of(&[system("a")]);
        assert_eq!(
            t.classify(&turn("th", 5000, 0, p), Instant::now()),
            Some(MissReason::NoBaseline)
        );
    }

    #[test]
    fn a_short_prompt_is_below_minimum_even_on_the_first_turn() {
        let t = CacheTelemetry::new(8);
        let p = PrefixDigest::of(&[system("a")]);
        assert_eq!(
            t.classify(&turn("th", 300, 0, p), Instant::now()),
            Some(MissReason::BelowMinimum)
        );
    }

    #[test]
    fn the_minimum_follows_the_model_family() {
        let t = CacheTelemetry::new(8);
        let p = PrefixDigest::of(&[system("a")]);
        let mut obs = turn("th", 1500, 0, p);
        obs.model = "claude-haiku-4-5";
        assert_eq!(t.classify(&obs, Instant::now()), Some(MissReason::BelowMinimum));
        obs.model = "claude-sonnet-4-5";
        assert_eq!(t.classify(&obs, Instant::now()), Some(MissReason::NoBaseline));
    }

    #[test]
    fn a_changed_system_prefix_is_prefix_changed() {
        let t = CacheTelemetry::new(8);
        let now = Instant::now();
        t.classify(&turn("th", 5000, 0, PrefixDigest::of(&[system("memory v1")])), now);
        assert_eq!(
            t.classify(
                &turn("th", 5100, 0, PrefixDigest::of(&[system("memory v2")])),
                now + Duration::from_secs(10)
            ),
            Some(MissReason::PrefixChanged)
        );
    }

    #[test]
    fn the_same_prefix_after_the_ttl_is_ttl_expired() {
        let t = CacheTelemetry::new(8);
        let now = Instant::now();
        let p = PrefixDigest::of(&[system("a")]);
        t.classify(&turn("th", 5000, 0, p), now);
        assert_eq!(
            t.classify(&turn("th", 5100, 0, p), now + PREFIX_TTL + Duration::from_secs(1)),
            Some(MissReason::TtlExpired)
        );
    }

    #[test]
    fn the_same_prefix_soon_after_with_no_read_is_unexplained() {
        let t = CacheTelemetry::new(8);
        let now = Instant::now();
        let p = PrefixDigest::of(&[system("a")]);
        t.classify(&turn("th", 5000, 0, p), now);
        assert_eq!(
            t.classify(&turn("th", 5100, 0, p), now + Duration::from_secs(20)),
            Some(MissReason::Unexplained)
        );
    }

    #[test]
    fn a_hit_refreshes_the_baseline() {
        let t = CacheTelemetry::new(8);
        let now = Instant::now();
        let p = PrefixDigest::of(&[system("a")]);
        t.classify(&turn("th", 5000, 0, p), now);
        t.classify(&turn("th", 5100, 4000, p), now + Duration::from_secs(250));
        assert_eq!(
            t.classify(&turn("th", 5200, 0, p), now + Duration::from_secs(500)),
            Some(MissReason::Unexplained)
        );
    }

    #[test]
    fn threads_do_not_share_a_baseline() {
        let t = CacheTelemetry::new(8);
        let now = Instant::now();
        let p = PrefixDigest::of(&[system("a")]);
        t.classify(&turn("one", 5000, 0, p), now);
        assert_eq!(t.classify(&turn("two", 5000, 0, p), now), Some(MissReason::NoBaseline));
    }

    #[test]
    fn the_oldest_touched_thread_is_evicted_at_capacity() {
        let t = CacheTelemetry::new(2);
        let now = Instant::now();
        let p = PrefixDigest::of(&[system("a")]);
        t.classify(&turn("one", 5000, 0, p), now);
        t.classify(&turn("two", 5000, 0, p), now);
        t.classify(&turn("one", 5000, 0, p), now);
        t.classify(&turn("three", 5000, 0, p), now);
        assert_eq!(t.classify(&turn("two", 5000, 0, p), now), Some(MissReason::NoBaseline));
        assert_eq!(t.classify(&turn("three", 5000, 0, p), now), Some(MissReason::Unexplained));
    }

    #[test]
    fn a_turn_with_no_reported_input_says_nothing() {
        let t = CacheTelemetry::new(8);
        let p = PrefixDigest::of(&[system("a")]);
        assert_eq!(t.classify(&turn("th", 0, 0, p), Instant::now()), None);
    }

    #[test]
    fn only_the_leading_system_run_is_digested() {
        let with_history = PrefixDigest::of(&[system("a"), user("hello"), system("late")]);
        let without = PrefixDigest::of(&[system("a")]);
        assert_eq!(with_history, without);
        assert_ne!(PrefixDigest::of(&[system("a"), system("b")]), PrefixDigest::of(&[system("ab")]));
    }
}
