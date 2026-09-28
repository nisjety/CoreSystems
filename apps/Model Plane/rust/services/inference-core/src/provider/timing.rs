//! Where inference-core's share of one call's wall time went.
//!
//! The fallback chain measures from its own entry ([`CallClock`]); the gRPC
//! handler then re-anchors the result on its own entry, before authorization
//! ([`HandlerClock`]), and projects it onto the wire `ProviderTiming`. The
//! proto message documents every field and which origin it is measured from.
//!
//! Durations and counts only. Nothing here ever sees prompt text, content,
//! tool arguments or tool results, so the timing is safe to log and to
//! forward on zero-data-retention and temporary turns.
//!
//! Measured on [`tokio::time::Instant`]: the monotonic std clock in
//! production, and tokio's paused clock under `start_paused` tests, which is
//! what lets the timing tests assert exact milliseconds instead of sleeping
//! and hoping.

use std::collections::BTreeMap;
use std::time::Duration;

use tokio::time::Instant;

use super::{InferChunk, InferResponse};

/// Internal counterpart of the wire `ProviderTiming`.
///
/// `handler` and `routing` are on the handler origin; `provider`, `accept`
/// and `first_token` are on the origin of the attempt that served the call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderTiming {
    pub handler: Duration,
    pub routing: Duration,
    pub provider: Duration,
    pub accept: Duration,
    /// `None` until a non-empty content or reasoning delta arrived, and
    /// always on unary calls. Reaches the wire as 0.
    pub first_token: Option<Duration>,
    pub attempts: u32,
    pub failed_attempts: Duration,
    pub retry_wait: Duration,
    pub response_cache_hit: bool,
    pub usage_reported: bool,
    /// Phase durations an upstream reported about itself, or that an adapter
    /// measured about its upstream (the Codex subscription adapter fills it
    /// with the broker's phases and `broker_ready`). The chain carries
    /// forward whatever an adapter puts here.
    pub upstream: BTreeMap<String, Duration>,
}

impl ProviderTiming {
    /// Whether a provider reported token usage at all.
    ///
    /// Both counts at 0 is how an adapter that receives no usage reports it
    /// (the Codex subscription broker when Codex sent none, or a broker that
    /// predates usage reporting); a real call always consumes input tokens,
    /// so this is the "unknown, not free" signal. An adapter that knows
    /// usage was reported says so on its own timing, and the chain keeps it.
    #[must_use]
    pub fn usage_reported(input_tokens: i32, output_tokens: i32) -> bool {
        input_tokens > 0 || output_tokens > 0
    }
}

/// The gRPC handler's clock: its own entry, and the moment it handed the
/// request to the chain.
#[derive(Debug, Clone, Copy)]
pub struct HandlerClock {
    entered: Instant,
    chain_entered: Instant,
}

impl HandlerClock {
    /// Read at the handler's entry, before authorization.
    #[must_use]
    pub fn start() -> Self {
        let now = Instant::now();
        Self {
            entered: now,
            chain_entered: now,
        }
    }

    /// Read immediately before the request is handed to the chain.
    pub fn enter_chain(&mut self) {
        self.chain_entered = Instant::now();
    }

    /// Move chain-origin timing onto the handler origin and close `handler`
    /// now: authorization and request conversion count as routing.
    pub fn stamp(&self, timing: &mut ProviderTiming) {
        timing.routing += self.chain_entered.saturating_duration_since(self.entered);
        timing.handler = self.entered.elapsed();
    }
}

/// Wall-clock bookkeeping for one chain call, measured from the chain's entry.
#[derive(Debug)]
pub(crate) struct CallClock {
    entered: Instant,
    first_attempt: Option<Duration>,
    failed_attempts: Duration,
    retry_wait: Duration,
}

impl CallClock {
    pub(crate) fn start() -> Self {
        Self {
            entered: Instant::now(),
            first_attempt: None,
            failed_attempts: Duration::ZERO,
            retry_wait: Duration::ZERO,
        }
    }

    /// Mark the start of a provider attempt and return its origin.
    pub(crate) fn begin_attempt(&mut self) -> Instant {
        let now = Instant::now();
        let entered = self.entered;
        self.first_attempt
            .get_or_insert_with(|| now.saturating_duration_since(entered));
        now
    }

    /// Charge an attempt that did not serve the call.
    pub(crate) fn attempt_failed(&mut self, began: Instant) {
        self.failed_attempts += began.elapsed();
    }

    /// Charge a retry-after wait that began at `began` and has just ended.
    pub(crate) fn waited(&mut self, began: Instant) {
        self.retry_wait += began.elapsed();
    }

    /// Timing for a response served from the completed-response cache: no
    /// attempt started, so everything so far is routing.
    pub(crate) fn cache_hit(&self, response: &InferResponse) -> ProviderTiming {
        ProviderTiming {
            routing: self.entered.elapsed(),
            response_cache_hit: true,
            usage_reported: ProviderTiming::usage_reported(
                response.input_tokens,
                response.output_tokens,
            ),
            ..ProviderTiming::default()
        }
    }

    /// Timing for a unary attempt that began at `began` and has just served
    /// `response`, after `attempts` provider calls in total. Keeps any
    /// upstream phases and usage signal the adapter reported.
    pub(crate) fn served(
        &self,
        began: Instant,
        attempts: u32,
        response: &mut InferResponse,
    ) -> ProviderTiming {
        let mut timing = self.before_serving(attempts);
        timing.provider = began.elapsed();
        timing.usage_reported =
            ProviderTiming::usage_reported(response.input_tokens, response.output_tokens);
        if let Some(reported) = response.provider_timing.take() {
            keep_adapter_report(&mut timing, reported);
        }
        timing
    }

    /// Timing for a stream a provider has just accepted, finished chunk by
    /// chunk by [`StreamClock::observe`].
    pub(crate) fn accepted(&self, began: Instant, attempts: u32) -> StreamClock {
        let mut timing = self.before_serving(attempts);
        timing.accept = began.elapsed();
        StreamClock { began, timing }
    }

    fn before_serving(&self, attempts: u32) -> ProviderTiming {
        ProviderTiming {
            routing: self.first_attempt.unwrap_or_default(),
            attempts,
            failed_attempts: self.failed_attempts,
            retry_wait: self.retry_wait,
            ..ProviderTiming::default()
        }
    }
}

/// The serving attempt's clock for one accepted stream.
#[derive(Debug)]
pub(crate) struct StreamClock {
    began: Instant,
    timing: ProviderTiming,
}

impl StreamClock {
    /// Look at one chunk on its way out: the first non-empty content or
    /// reasoning delta marks `first_token`, and the final chunk closes
    /// `provider` and carries the timing. Non-final chunks carry none.
    pub(crate) fn observe(&mut self, chunk: &mut InferChunk) {
        if self.timing.first_token.is_none()
            && (!chunk.delta.is_empty() || !chunk.reasoning_delta.is_empty())
        {
            self.timing.first_token = Some(self.began.elapsed());
        }
        if !chunk.done {
            chunk.provider_timing = None;
            return;
        }
        let mut timing = std::mem::take(&mut self.timing);
        timing.provider = self.began.elapsed();
        timing.usage_reported =
            ProviderTiming::usage_reported(chunk.input_tokens, chunk.output_tokens);
        if let Some(reported) = chunk.provider_timing.take() {
            keep_adapter_report(&mut timing, reported);
        }
        chunk.provider_timing = Some(timing);
    }
}

/// The parts of an adapter's own timing the chain keeps: upstream phases,
/// and a usage report the token counts alone cannot show. Everything else
/// is the chain's to measure.
fn keep_adapter_report(timing: &mut ProviderTiming, reported: ProviderTiming) {
    timing.upstream = reported.upstream;
    timing.usage_reported |= reported.usage_reported;
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Transport projection, shared by the unary and streaming gRPC surfaces so
/// the timing is mapped in exactly one place.
impl From<ProviderTiming> for mp_contracts::model_plane::v1::ProviderTiming {
    fn from(value: ProviderTiming) -> Self {
        Self {
            handler_ms: millis(value.handler),
            routing_ms: millis(value.routing),
            provider_ms: millis(value.provider),
            accept_ms: millis(value.accept),
            first_token_ms: value.first_token.map_or(0, millis),
            attempts: value.attempts,
            failed_attempts_ms: millis(value.failed_attempts),
            retry_wait_ms: millis(value.retry_wait),
            response_cache_hit: value.response_cache_hit,
            usage_reported: value.usage_reported,
            upstream_ms: value
                .upstream
                .into_iter()
                .map(|(phase, duration)| (phase, millis(duration)))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_projection_copies_every_field_in_whole_milliseconds() {
        let timing = ProviderTiming {
            handler: Duration::from_micros(41_999),
            routing: Duration::from_millis(3),
            provider: Duration::from_millis(30),
            accept: Duration::from_millis(2),
            first_token: Some(Duration::from_millis(20)),
            attempts: 2,
            failed_attempts: Duration::from_millis(5),
            retry_wait: Duration::from_secs(1),
            response_cache_hit: false,
            usage_reported: true,
            upstream: BTreeMap::from([("broker_ready".to_owned(), Duration::from_millis(7))]),
        };

        let wire = mp_contracts::model_plane::v1::ProviderTiming::from(timing);

        assert_eq!(wire.handler_ms, 41, "milliseconds truncate, never round up");
        assert_eq!(wire.routing_ms, 3);
        assert_eq!(wire.provider_ms, 30);
        assert_eq!(wire.accept_ms, 2);
        assert_eq!(wire.first_token_ms, 20);
        assert_eq!(wire.attempts, 2);
        assert_eq!(wire.failed_attempts_ms, 5);
        assert_eq!(wire.retry_wait_ms, 1_000);
        assert!(!wire.response_cache_hit);
        assert!(wire.usage_reported);
        assert_eq!(wire.upstream_ms.get("broker_ready"), Some(&7));
    }

    #[test]
    fn a_missing_first_token_reaches_the_wire_as_zero() {
        let wire = mp_contracts::model_plane::v1::ProviderTiming::from(ProviderTiming::default());
        assert_eq!(wire.first_token_ms, 0);
        assert!(wire.upstream_ms.is_empty());
    }

    #[test]
    fn usage_is_reported_only_when_a_count_is_nonzero() {
        assert!(!ProviderTiming::usage_reported(0, 0));
        assert!(ProviderTiming::usage_reported(12, 0));
        assert!(ProviderTiming::usage_reported(0, 3));
    }

    #[tokio::test(start_paused = true)]
    async fn handler_clock_moves_routing_onto_the_handler_origin() {
        let mut clock = HandlerClock::start();
        // Authorization and request conversion.
        tokio::time::advance(Duration::from_millis(7)).await;
        clock.enter_chain();
        // The chain's own routing, then the provider.
        tokio::time::advance(Duration::from_millis(40)).await;
        let mut timing = ProviderTiming {
            routing: Duration::from_millis(2),
            provider: Duration::from_millis(38),
            ..ProviderTiming::default()
        };

        clock.stamp(&mut timing);

        assert_eq!(timing.routing, Duration::from_millis(9));
        assert_eq!(timing.handler, Duration::from_millis(47));
        assert_eq!(
            timing.provider,
            Duration::from_millis(38),
            "attempt-origin fields are untouched"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn stream_clock_stamps_only_the_final_chunk() {
        let mut calls = CallClock::start();
        let began = calls.begin_attempt();
        tokio::time::advance(Duration::from_millis(4)).await;
        let mut stream = calls.accepted(began, 1);
        let chunk = |delta: &str, reasoning: &str, done: bool| InferChunk {
            compaction_summary: String::new(),
            request_id: "r".to_owned(),
            delta: delta.to_owned(),
            done,
            model_used: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            stop_reason: String::new(),
            reasoning_delta: reasoning.to_owned(),
            provider_used: String::new(),
            residency: String::new(),
            token_confidence: None,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            provider_timing: None,
        };

        tokio::time::advance(Duration::from_millis(6)).await;
        let mut empty = chunk("", "", false);
        stream.observe(&mut empty);
        tokio::time::advance(Duration::from_millis(5)).await;
        // Reasoning counts as the first token: it is the model producing output.
        let mut thinking = chunk("", "hmm", false);
        stream.observe(&mut thinking);
        tokio::time::advance(Duration::from_millis(5)).await;
        let mut answer = chunk("hei", "", false);
        stream.observe(&mut answer);
        tokio::time::advance(Duration::from_millis(10)).await;
        let mut done = chunk("", "", true);
        stream.observe(&mut done);

        assert!(empty.provider_timing.is_none());
        assert!(thinking.provider_timing.is_none());
        assert!(answer.provider_timing.is_none());
        let timing = done
            .provider_timing
            .expect("the final chunk carries the timing");
        assert_eq!(timing.accept, Duration::from_millis(4));
        assert_eq!(timing.first_token, Some(Duration::from_millis(15)));
        assert_eq!(timing.provider, Duration::from_millis(30));
        assert_eq!(timing.attempts, 1);
        assert!(
            !timing.usage_reported,
            "a stream that reported no tokens is unknown usage"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_chain_keeps_an_adapters_upstream_phases_and_usage_report() {
        let reported = |usage_reported: bool| ProviderTiming {
            usage_reported,
            upstream: BTreeMap::from([("turn_total".to_owned(), Duration::from_millis(900))]),
            // The chain measures this itself; an adapter's value is ignored.
            provider: Duration::from_secs(99),
            ..ProviderTiming::default()
        };
        let mut calls = CallClock::start();
        let began = calls.begin_attempt();
        tokio::time::advance(Duration::from_millis(12)).await;

        let mut response = InferResponse {
            provider_timing: Some(reported(true)),
            ..InferResponse::default()
        };
        let unary = calls.served(began, 1, &mut response);
        assert!(unary.usage_reported, "the adapter saw usage reported");
        assert_eq!(unary.provider, Duration::from_millis(12));
        assert_eq!(
            unary.upstream.get("turn_total"),
            Some(&Duration::from_millis(900))
        );

        let mut unreported = InferResponse {
            provider_timing: Some(reported(false)),
            ..InferResponse::default()
        };
        assert!(
            !calls.served(began, 1, &mut unreported).usage_reported,
            "no report and no tokens stays unknown"
        );

        let mut stream = calls.accepted(began, 1);
        let mut done = InferChunk {
            compaction_summary: String::new(),
            request_id: "r".to_owned(),
            delta: String::new(),
            done: true,
            model_used: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            stop_reason: String::new(),
            reasoning_delta: String::new(),
            provider_used: String::new(),
            residency: String::new(),
            token_confidence: None,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            provider_timing: Some(reported(true)),
        };
        stream.observe(&mut done);
        let streamed = done
            .provider_timing
            .expect("the final chunk carries the timing");
        assert!(streamed.usage_reported);
        assert_eq!(
            streamed.upstream.get("turn_total"),
            Some(&Duration::from_millis(900))
        );
        assert_eq!(streamed.provider, Duration::from_millis(12));
    }
}
