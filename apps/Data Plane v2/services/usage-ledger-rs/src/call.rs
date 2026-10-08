//! What one provider call used, in the terms the ledger needs.
//!
//! A producer holds the provider's own answer (an inference-core `InferResponse`, or the
//! vectors of an embedding call that report no usage at all) and turns it into a
//! [`CallUsage`] here. Every rule about what a call is worth lives in this file, so a
//! second producer cannot read the same response differently:
//!
//! * the counts are copied exactly as the provider reported them. Nothing is added to
//!   them: inference-core already folds the cache reads and writes into `input_tokens`
//!   (`total_input_tokens`), so adding `cache_read_input_tokens` would count them twice;
//! * a call answered from inference-core's completed-response cache called no provider
//!   and spent nothing, so it contributes nothing at all;
//! * a call whose provider reported no usage still happened. It is counted, its counts
//!   mean "unknown", and the event it lands in says so (`estimated`), because zero tokens
//!   on a real call would otherwise read as free.

/// How far the counts of one call can be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallBasis {
    /// The provider reported them: exact.
    Reported,
    /// The provider reported no usage. The counts mean unknown, not free.
    Unreported,
    /// This service estimated them. An embedding call returns vectors only, so its token
    /// count is the length of the text it sent divided by four.
    Estimated,
    /// Answered from inference-core's response cache. No provider was called.
    CacheHit,
}

/// The usage fields of one `InferResponse`, as they are on the wire.
///
/// A plain copy of the fields rather than the generated message, because each producer
/// compiles the Model Plane proto into its own crate and two crates' generated types
/// cannot be passed to one another.
#[derive(Clone, Copy, Debug)]
pub struct WireUsage<'a> {
    pub request_id: &'a str,
    pub model_used: &'a str,
    pub provider_used: &'a str,
    /// Prompt tokens. Already includes the cache reads and writes.
    pub input_tokens: i32,
    pub output_tokens: i32,
    /// Tokens served from the provider's prompt cache: detail of `input_tokens`, not an
    /// addition to it.
    pub cache_read_input_tokens: i32,
    /// `provider_timing`; `None` from a server that predates the field.
    pub timing: Option<WireTiming>,
}

/// The two flags of `InferResponse.provider_timing` that decide whether a call is metered.
#[derive(Clone, Copy, Debug)]
pub struct WireTiming {
    pub usage_reported: bool,
    pub response_cache_hit: bool,
}

/// One provider call, ready to be aggregated into an event by [`crate::UsageBuilder`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallUsage {
    pub(crate) request_id: String,
    pub(crate) model_used: String,
    pub(crate) provider_used: String,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cache_read_tokens: u64,
    /// How many items the call covered: one for a chat completion, the number of texts
    /// for an embedding call.
    pub(crate) items: u64,
    /// Billed units that are not tokens (a reranker's search units).
    pub(crate) units: Option<u64>,
    pub(crate) basis: CallBasis,
}

impl CallUsage {
    /// A call as `InferResponse` reports it: negative counts (a server's bug) are zero, and
    /// a server that predates `provider_timing` has usage reported exactly when it sent a
    /// count.
    pub fn from_wire(wire: WireUsage<'_>) -> Self {
        let count = |value: i32| u64::try_from(value.max(0)).unwrap_or(0);
        let (input_tokens, output_tokens) = (count(wire.input_tokens), count(wire.output_tokens));
        let usage_reported = wire
            .timing
            .map_or(input_tokens > 0 || output_tokens > 0, |timing| {
                timing.usage_reported
            });
        Self::from_counts(
            wire.request_id,
            wire.model_used,
            wire.provider_used,
            input_tokens,
            output_tokens,
            count(wire.cache_read_input_tokens),
            usage_reported,
            wire.timing.is_some_and(|timing| timing.response_cache_hit),
        )
    }

    /// A call whose counts a producer already holds as numbers (the contextualizer reads
    /// the response once and keeps these).
    #[allow(clippy::too_many_arguments)]
    pub fn from_counts(
        request_id: &str,
        model_used: &str,
        provider_used: &str,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        usage_reported: bool,
        response_cache_hit: bool,
    ) -> Self {
        let basis = if response_cache_hit {
            CallBasis::CacheHit
        } else if usage_reported {
            CallBasis::Reported
        } else {
            CallBasis::Unreported
        };
        Self {
            request_id: request_id.trim().to_owned(),
            model_used: model_used.to_owned(),
            provider_used: provider_used.to_owned(),
            input_tokens,
            output_tokens,
            cache_read_tokens,
            items: 1,
            units: None,
            basis,
        }
    }

    /// A call whose token count this service estimated: an embedding request returns
    /// vectors and no usage. `call_id` is an id the caller generated for this one call (a
    /// UUID): there is no provider request id to use, and the id is what makes the event
    /// idempotent under a publish retry and distinct from a later re-embed of the same text.
    pub fn estimated(
        call_id: &str,
        model_used: &str,
        provider_used: &str,
        items: u64,
        input_tokens: u64,
    ) -> Self {
        Self {
            request_id: call_id.trim().to_owned(),
            model_used: model_used.to_owned(),
            provider_used: provider_used.to_owned(),
            input_tokens,
            output_tokens: 0,
            cache_read_tokens: 0,
            items,
            units: None,
            basis: CallBasis::Estimated,
        }
    }

    /// Billed units that are not tokens (a reranker's search units).
    #[must_use]
    pub fn with_units(mut self, units: u64) -> Self {
        self.units = Some(units);
        self
    }

    pub fn basis(&self) -> CallBasis {
        self.basis
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Whether this call belongs in the ledger at all.
    pub fn is_metered(&self) -> bool {
        self.basis != CallBasis::CacheHit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fields of a real Azure OpenAI answer as inference-core returns them: 612
    /// prompt tokens of which 512 were served from the prompt cache, 48 completion tokens.
    fn wire<'a>(timing: Option<WireTiming>) -> WireUsage<'a> {
        WireUsage {
            request_id: "9d0c7a3e-6f5b-4a3c-9f1e-0d4b2c8a7e11",
            model_used: "gpt-4o-mini-2024-07-18",
            provider_used: "azure-openai",
            input_tokens: 612,
            output_tokens: 48,
            cache_read_input_tokens: 512,
            timing,
        }
    }

    #[test]
    fn the_counts_are_the_providers_exactly_and_the_cache_is_not_added_again() {
        let call = CallUsage::from_wire(wire(Some(WireTiming {
            usage_reported: true,
            response_cache_hit: false,
        })));
        assert_eq!(
            call.input_tokens, 612,
            "input already includes the cache reads"
        );
        assert_eq!(call.output_tokens, 48);
        assert_eq!(
            call.cache_read_tokens, 512,
            "cache reads are detail, never an addition"
        );
        assert_eq!(call.basis(), CallBasis::Reported);
        assert!(call.is_metered());
        assert_eq!(call.items, 1);
    }

    #[test]
    fn a_response_cache_hit_is_not_metered() {
        let call = CallUsage::from_wire(wire(Some(WireTiming {
            usage_reported: true,
            response_cache_hit: true,
        })));
        assert_eq!(call.basis(), CallBasis::CacheHit);
        assert!(!call.is_metered());
    }

    #[test]
    fn a_provider_that_reported_no_usage_is_a_call_with_unknown_counts() {
        let call = CallUsage::from_wire(wire(Some(WireTiming {
            usage_reported: false,
            response_cache_hit: false,
        })));
        assert_eq!(call.basis(), CallBasis::Unreported);
        assert!(
            call.is_metered(),
            "the call happened: it is counted, not dropped"
        );
    }

    #[test]
    fn a_server_without_timing_reports_usage_exactly_when_it_sent_a_count() {
        let with_count = CallUsage::from_wire(wire(None));
        assert_eq!(with_count.basis(), CallBasis::Reported);

        let mut empty = wire(None);
        empty.input_tokens = 0;
        empty.output_tokens = 0;
        assert_eq!(CallUsage::from_wire(empty).basis(), CallBasis::Unreported);
        // And it cannot have been answered from a cache it does not know about.
        assert!(CallUsage::from_wire(wire(None)).is_metered());
    }

    #[test]
    fn negative_wire_counts_are_zero() {
        let mut broken = wire(None);
        broken.input_tokens = -5;
        broken.output_tokens = 7;
        broken.cache_read_input_tokens = -1;
        let call = CallUsage::from_wire(broken);
        assert_eq!(
            (
                call.input_tokens,
                call.output_tokens,
                call.cache_read_tokens
            ),
            (0, 7, 0)
        );
    }

    #[test]
    fn an_estimated_call_says_so_and_carries_its_item_count() {
        let call = CallUsage::estimated("call-1", "embed-v-4-0", "azure-cohere", 24, 900);
        assert_eq!(call.basis(), CallBasis::Estimated);
        assert_eq!(
            (call.items, call.input_tokens, call.output_tokens),
            (24, 900, 0)
        );
        assert!(call.is_metered());
    }
}
