//! Prometheus counters for inference-core, rendered on `:8082/metrics`.
//!
//! Two counters answer "who is calling the Model Plane, for which model, and
//! what came of it":
//!
//! * `inference_requests_total{rpc,service,model,provider,outcome}`: one per
//!   finished call, whatever its result.
//! * `inference_tokens_total{service,model,provider,direction}`: tokens the
//!   serving provider reported, `direction` being `input` or `output`.
//!
//! # Outcomes
//!
//! `outcome` is `ok`, the typed `x-mp-provider-error` kind when the failure
//! carries one (`zdr_unavailable`, `residency_unavailable`, `tier_unavailable`,
//! `too_long`, `rate_limited`, `subscription_*`), or the lower-case gRPC code
//! otherwise (`invalid_argument`, `permission_denied`, `unavailable`, ...). A
//! request refused before any provider was called therefore still moves a
//! counter, under the outcome that says why, and the `ok` series does not move.
//!
//! # Cardinality
//!
//! No org and no user is ever a label. `service` is the verified service id of a
//! service principal (`service:<id>`, which Auth Core mints from its registry),
//! the constant `user` for an end-user token and `unauthenticated` when no
//! credential was verified. `model` and `provider` are what the response names
//! (`model_used`, `provider_used`), never the caller-controlled request field;
//! they are `unknown` when the call failed before a provider answered and `none`
//! for a call that serves no model (a catalogue read). Every label that is
//! derived from data a caller can influence goes through a [`LabelSet`]: a value
//! that is not a plain identifier becomes `invalid` and a value past the label's
//! cap becomes `other`, so a hostile or buggy caller cannot grow the series set.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock, PoisonError};

use metrics::{counter, describe_counter, describe_gauge, gauge};
use metrics_exporter_prometheus::{BuildError, PrometheusBuilder, PrometheusHandle};
use tonic::{Code, Status};

use crate::auth::AuthenticatedPrincipal;
use crate::grpc::PROVIDER_ERROR_KIND_TRAILER;

/// Counter: finished calls by rpc, caller, serving model and provider, outcome.
pub const REQUESTS_TOTAL: &str = "inference_requests_total";

/// Counter: provider-reported tokens by caller, model, provider and direction.
pub const TOKENS_TOTAL: &str = "inference_tokens_total";

/// Gauge fixed at 1: the version of the running binary. It also makes the
/// scrape non-empty from the first second, before any request has arrived.
pub const BUILD_INFO: &str = "inference_core_build_info";

/// Label value for "the call failed before a provider answered, so no model or
/// provider is known".
pub const UNKNOWN: &str = "unknown";

/// Label value for "this call serves no model" (a catalogue read).
pub const NONE: &str = "none";

/// `service` label value for a call that carried no verified credential.
pub const UNAUTHENTICATED: &str = "unauthenticated";

/// `service` label value for an end-user token. Users are never labelled
/// individually.
pub const END_USER: &str = "user";

/// Where a label value past its cap goes.
const OTHER: &str = "other";

/// Where a value that is not a plain identifier goes.
const INVALID: &str = "invalid";

/// Longest label value kept as it is.
const MAX_LABEL_LEN: usize = 80;

/// How many distinct service ids, models, providers and outcomes are kept
/// before the rest collapse into `other`. Far above anything the platform
/// configures today, far below anything that would hurt a Prometheus.
const MAX_SERVICES: usize = 64;
const MAX_MODELS: usize = 128;
const MAX_PROVIDERS: usize = 32;
const MAX_OUTCOMES: usize = 64;

/// Install the Prometheus recorder as the process-global recorder and return the
/// handle `/metrics` renders from.
///
/// # Errors
///
/// Returns an error when a global recorder is already installed. The binary
/// treats that as fatal at boot: a second recorder would leave `/metrics` empty
/// while every counter silently went to the first.
pub fn install_recorder() -> Result<PrometheusHandle, BuildError> {
    let handle = PrometheusBuilder::new().install_recorder()?;
    describe();
    gauge!(BUILD_INFO, "version" => env!("CARGO_PKG_VERSION")).set(1.0);
    Ok(handle)
}

/// Register the help text of every metric this service emits.
pub fn describe() {
    describe_counter!(
        REQUESTS_TOTAL,
        "Finished inference-core calls by rpc, calling service, serving model and provider, and outcome (ok, a typed refusal kind such as zdr_unavailable, or the gRPC code)."
    );
    describe_counter!(
        TOKENS_TOTAL,
        "Tokens reported by the serving provider, by calling service, model, provider and direction (input already includes prompt-cache tokens)."
    );
    describe_gauge!(BUILD_INFO, "Version of the running inference-core binary.");
}

/// A bounded set of label values: the first `cap` distinct plain identifiers
/// are kept, everything after them becomes `other`.
pub struct LabelSet {
    cap: usize,
    seen: Mutex<HashSet<String>>,
}

impl LabelSet {
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            seen: Mutex::new(HashSet::new()),
        }
    }

    /// The label value to record for `raw`.
    ///
    /// An empty value is `unknown`; a value that is not a plain identifier
    /// (too long, or carrying a character outside letters, digits and
    /// `- _ . : / @ +`) is `invalid`; a new value past the cap is `other`. The
    /// fixed sentinels pass through untouched and never use up the cap.
    #[must_use]
    pub fn admit(&self, raw: &str) -> String {
        let value = raw.trim();
        if value.is_empty() {
            return UNKNOWN.to_owned();
        }
        if matches!(
            value,
            UNKNOWN | NONE | UNAUTHENTICATED | END_USER | OTHER | INVALID
        ) {
            return value.to_owned();
        }
        if value.len() > MAX_LABEL_LEN || !value.chars().all(is_label_char) {
            return INVALID.to_owned();
        }
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        if seen.contains(value) {
            return value.to_owned();
        }
        if seen.len() >= self.cap {
            return OTHER.to_owned();
        }
        seen.insert(value.to_owned());
        value.to_owned()
    }
}

fn is_label_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/' | '@' | '+')
}

struct Labels {
    services: LabelSet,
    models: LabelSet,
    providers: LabelSet,
    outcomes: LabelSet,
}

fn labels() -> &'static Labels {
    static LABELS: OnceLock<Labels> = OnceLock::new();
    LABELS.get_or_init(|| Labels {
        services: LabelSet::new(MAX_SERVICES),
        models: LabelSet::new(MAX_MODELS),
        providers: LabelSet::new(MAX_PROVIDERS),
        outcomes: LabelSet::new(MAX_OUTCOMES),
    })
}

/// The `service` label for a verified principal: the service id of a service
/// principal, `user` for an end-user token. Never the org or the user id.
#[must_use]
pub fn caller_label(principal: &AuthenticatedPrincipal) -> String {
    if principal.user_id.is_none() && principal.subject.starts_with("service:") {
        labels().services.admit(&principal.subject)
    } else {
        END_USER.to_owned()
    }
}

/// The `outcome` label for a failed call: the typed provider-error kind when the
/// status carries one, else the lower-case gRPC code.
#[must_use]
pub fn outcome_of(status: &Status) -> String {
    let kind = status
        .metadata()
        .get(PROVIDER_ERROR_KIND_TRAILER)
        .and_then(|value| value.to_str().ok())
        .filter(|kind| !kind.trim().is_empty());
    match kind {
        Some(kind) => labels().outcomes.admit(kind),
        None => code_name(status.code()).to_owned(),
    }
}

fn code_name(code: Code) -> &'static str {
    match code {
        Code::Ok => "ok",
        Code::Cancelled => "cancelled",
        Code::Unknown => "unknown",
        Code::InvalidArgument => "invalid_argument",
        Code::DeadlineExceeded => "deadline_exceeded",
        Code::NotFound => "not_found",
        Code::AlreadyExists => "already_exists",
        Code::PermissionDenied => "permission_denied",
        Code::ResourceExhausted => "resource_exhausted",
        Code::FailedPrecondition => "failed_precondition",
        Code::Aborted => "aborted",
        Code::OutOfRange => "out_of_range",
        Code::Unimplemented => "unimplemented",
        Code::Internal => "internal",
        Code::Unavailable => "unavailable",
        Code::DataLoss => "data_loss",
        Code::Unauthenticated => "unauthenticated",
    }
}

/// Count one finished call.
///
/// `service` is [`caller_label`]'s value (or [`UNAUTHENTICATED`]); `model` and
/// `provider` are what the response named, or [`UNKNOWN`] / [`NONE`]; `outcome`
/// is `ok` or [`outcome_of`]'s value. Every one is bounded here, so a caller
/// cannot pass a raw value around the cap.
pub fn record_request(
    rpc: &'static str,
    service: &str,
    model: &str,
    provider: &str,
    outcome: &str,
) {
    let labels = labels();
    counter!(
        REQUESTS_TOTAL,
        "rpc" => rpc,
        "service" => labels.services.admit(service),
        "model" => labels.models.admit(model),
        "provider" => labels.providers.admit(provider),
        "outcome" => labels.outcomes.admit(outcome),
    )
    .increment(1);
}

/// Count the tokens a provider reported for one call.
///
/// `input` already includes the prompt-cache read and write legs (the provider
/// adapters fold them in), so they are not added again. A leg of zero is not
/// recorded: a route that reports no usage creates no series.
pub fn record_tokens(service: &str, model: &str, provider: &str, input: u64, output: u64) {
    let labels = labels();
    for (direction, tokens) in [("input", input), ("output", output)] {
        if tokens == 0 {
            continue;
        }
        counter!(
            TOKENS_TOTAL,
            "service" => labels.services.admit(service),
            "model" => labels.models.admit(model),
            "provider" => labels.providers.admit(provider),
            "direction" => direction,
        )
        .increment(tokens);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(body: impl FnOnce()) -> String {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        metrics::with_local_recorder(&recorder, body);
        handle.render()
    }

    #[test]
    fn a_label_set_keeps_its_first_values_and_folds_the_rest_into_other() {
        let set = LabelSet::new(2);
        assert_eq!(set.admit("gpt-4o-mini"), "gpt-4o-mini");
        assert_eq!(set.admit("model-router"), "model-router");
        assert_eq!(set.admit("a-third-model"), "other", "past the cap");
        assert_eq!(
            set.admit("gpt-4o-mini"),
            "gpt-4o-mini",
            "a value already admitted keeps its own series"
        );
    }

    #[test]
    fn a_label_value_that_is_not_a_plain_identifier_is_not_passed_on() {
        let set = LabelSet::new(8);
        assert_eq!(set.admit(""), "unknown");
        assert_eq!(set.admit("   "), "unknown");
        for hostile in [
            "has space",
            "quote\"inside",
            "new\nline",
            "back\\slash",
            "brace}",
            "non-ascii-\u{e6}\u{f8}\u{e5}",
            &"x".repeat(MAX_LABEL_LEN + 1),
        ] {
            assert_eq!(set.admit(hostile), "invalid", "{hostile:?}");
        }
        assert_eq!(set.admit("service:model-gateway"), "service:model-gateway");
        assert_eq!(set.admit("gpt-5.6-terra"), "gpt-5.6-terra");
    }

    #[test]
    fn the_fixed_sentinels_never_use_up_the_cap() {
        let set = LabelSet::new(1);
        for sentinel in [UNKNOWN, NONE, UNAUTHENTICATED, END_USER] {
            assert_eq!(set.admit(sentinel), sentinel);
        }
        assert_eq!(set.admit("only-real-value"), "only-real-value");
    }

    #[test]
    fn an_outcome_is_the_typed_kind_when_there_is_one_else_the_grpc_code() {
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert(
            PROVIDER_ERROR_KIND_TRAILER,
            "zdr_unavailable".parse().unwrap(),
        );
        let typed = Status::with_metadata(Code::FailedPrecondition, "refused", metadata);
        assert_eq!(outcome_of(&typed), "zdr_unavailable");

        assert_eq!(outcome_of(&Status::unavailable("down")), "unavailable");
        assert_eq!(
            outcome_of(&Status::permission_denied("no")),
            "permission_denied"
        );
        assert_eq!(
            outcome_of(&Status::unauthenticated("no")),
            "unauthenticated"
        );
        assert_eq!(
            outcome_of(&Status::failed_precondition("untyped")),
            "failed_precondition",
            "a precondition failure with no kind is named by its code"
        );
    }

    #[test]
    fn a_service_principal_is_labelled_by_its_service_id_and_a_user_is_not_labelled_at_all() {
        let service = AuthenticatedPrincipal::for_test("org-secret", None, false);
        assert_eq!(caller_label(&service), "service:test");
        let user = AuthenticatedPrincipal::for_test("org-secret", Some("user-secret"), false);
        assert_eq!(caller_label(&user), "user");
    }

    #[test]
    fn requests_and_tokens_render_with_their_labels_and_no_org_or_user() {
        let text = render(|| {
            record_request(
                "infer",
                "service:data-orchestrator",
                "gpt-4o-mini",
                "azure-openai",
                "ok",
            );
            record_request(
                "infer",
                "service:data-orchestrator",
                "gpt-4o-mini",
                "azure-openai",
                "ok",
            );
            record_request(
                "infer",
                "service:data-orchestrator",
                UNKNOWN,
                UNKNOWN,
                "zdr_unavailable",
            );
            record_tokens(
                "service:data-orchestrator",
                "gpt-4o-mini",
                "azure-openai",
                2000,
                5,
            );
        });

        let line = |name: &str, wanted: &[&str], value: &str| {
            text.lines().any(|line| {
                line.starts_with(name)
                    && wanted.iter().all(|label| line.contains(label))
                    && line.ends_with(&format!(" {value}"))
            })
        };
        assert!(
            line(
                REQUESTS_TOTAL,
                &[
                    "rpc=\"infer\"",
                    "service=\"service:data-orchestrator\"",
                    "model=\"gpt-4o-mini\"",
                    "provider=\"azure-openai\"",
                    "outcome=\"ok\"",
                ],
                "2"
            ),
            "two ok calls: {text}"
        );
        assert!(
            line(
                REQUESTS_TOTAL,
                &[
                    "model=\"unknown\"",
                    "provider=\"unknown\"",
                    "outcome=\"zdr_unavailable\""
                ],
                "1"
            ),
            "the refusal is its own series: {text}"
        );
        assert!(
            line(
                TOKENS_TOTAL,
                &["direction=\"input\"", "model=\"gpt-4o-mini\""],
                "2000"
            ),
            "{text}"
        );
        assert!(line(TOKENS_TOTAL, &["direction=\"output\""], "5"), "{text}");
        assert!(!text.contains("org"), "no org label: {text}");
    }

    #[test]
    fn a_call_that_reports_no_usage_creates_no_token_series() {
        let text = render(|| {
            record_request("infer", "service:x", "m", "p", "ok");
            record_tokens("service:x", "m", "p", 0, 0);
        });
        assert!(text.contains(REQUESTS_TOTAL), "{text}");
        assert!(!text.contains(TOKENS_TOTAL), "{text}");
    }
}
