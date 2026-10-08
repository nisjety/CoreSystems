//! What one chunk's graph extraction came to, as a closed set, and what the chunk's durable
//! state becomes because of it (design task CR-T12).
//!
//! The first version of this service had three answers to "what happened to this chunk":
//! it has `graph_text_units` rows, it has none, or the process remembers having tried. Nothing
//! told a failing chunk from an empty one, so the reconciler re-paid for both on every tick
//! (measured: 63 provider calls per tick for nine documents with no entities, and one chunk
//! retried at least ten times at six-minute intervals), and a restart forgot even that.
//!
//! Every outcome here says what happened, which `graph_extraction_state.status` it moves a
//! chunk to, which content-free `error_class` goes with it, whether the attempt counts toward
//! the poison cap, and when to look again. [`Outcome::settle`] is the one place that turns
//! an outcome into a [`Settlement`]; it reads no message text and does no I/O, so the table of
//! what becomes of each failure is a test, not a convention.
//!
//! ## Where the mapping comes from
//!
//! A failed call or mint is decoded by the shared client (`inference_client_rs::InferenceError`)
//! from the gRPC code and the `x-mp-provider-error` trailer. [`outcome_for_failure`] turns that
//! decoding into an outcome. The table in the tests is driven from the pinned copy of the
//! status contract inference-core generates and fails when that contract gains a case nobody
//! decided about. Failures that never reached inference-core (the client's own deadline, an
//! answer that is not JSON) are typed in `extractor` ([`ExtractionTimeout`],
//! [`InvalidModelOutput`]) so no decision reads prose.
//!
//! ## The vocabulary is the database's
//!
//! [`STATUS_NAMES`] and [`ERROR_CLASS_NAMES`] are the values the CHECK constraints of
//! migration `20261003150000_graph_extraction_state.sql` accept. A Go contract test reads
//! this file and compares both lists with the migration, so neither can drift from the schema
//! without a test failing.
//!
//! ## The ladder and the poison cap
//!
//! A failed attempt of the chunk's own (the provider failed, the answer was unusable, the
//! call timed out) counts. After [`DEFAULT_MAX_ATTEMPTS`] of them the chunk is
//! `failed_permanent` and nothing retries it until an operator puts it back to `pending`.
//! Between attempts it waits [`LADDER`]: five minutes, then fifteen, forty-five, two and a
//! quarter hours and six hours. A cause that is not the chunk's (the credential, the
//! organization's retention posture, an organization that is zero-retention, an exhausted
//! budget) never counts: it hands the attempt back, so a deployment problem cannot walk a
//! thousand healthy chunks to `failed_permanent`.

use std::time::Duration;

use inference_client_rs::{
    Failure, InferenceError, Stage, PENDING_RETRY_AFTER, SKIP_RECHECK_AFTER,
};

use crate::extractor::{EmptyModelOutput, ExtractionTimeout, InvalidModelOutput, ZdrOrganization};

/// The version of the extraction prompt (`extractor::build_prompt` and its system prompt). A
/// fingerprint test in `extractor` fails when the prompt text changes without this changing.
/// An `empty` chunk is attempted again when this differs from the version it was recorded under.
pub const PROMPT_VERSION: &str = "graph-v1";

/// The stamp the migration's backfill gives chunks that were extracted before provenance was
/// recorded: no model, no real prompt version. The service never writes it; it is the contract
/// with the migration (tests/contract/postgres reads this line), and the Rust tests pin it.
#[allow(dead_code)]
pub const LEGACY_PROMPT_VERSION: &str = "legacy";

/// Attempts after which a chunk is given up on (the design's poison cap).
pub const DEFAULT_MAX_ATTEMPTS: i32 = 6;

/// The largest cap `GRAPH_EXTRACTION_MAX_ATTEMPTS` may name: past this a cap is not a cap.
pub const MAX_ATTEMPTS_CEILING: i32 = 50;

/// Backoff after the first, second, ... failed attempt of a chunk. The last step repeats.
pub const LADDER: [Duration; 5] = [
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
    Duration::from_secs(45 * 60),
    Duration::from_secs(135 * 60),
    Duration::from_secs(6 * 60 * 60),
];

/// The longest a chunk waits for anything: the top of the ladder, and the most a provider's own
/// retry-after is honoured for.
pub const MAX_RETRY_WAIT: Duration = Duration::from_secs(6 * 60 * 60);

/// Document classifications whose content may be sent to the extraction model. Anything else, or
/// a missing classification, fails closed. Mirrors `embedding-engine-rs`'s caption rule, which is
/// the established idiom for this decision in the plane, and the set `v_graph_extraction_coverage`
/// and `v_context_coverage` use.
pub const EGRESS_ALLOWED_CLASSIFICATIONS: [&str; 3] = ["internal", "public", "sensitive"];

pub fn egress_allowed(classification: Option<&str>) -> bool {
    match classification {
        Some(c) => EGRESS_ALLOWED_CLASSIFICATIONS.contains(&c.trim().to_ascii_lowercase().as_str()),
        // Fail closed: a document with no classification is not assumed safe to send to the
        // extraction model.
        None => false,
    }
}

/// A claim on a chunk: the attempts it made, this one included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    pub attempts: i32,
}

/// What the chunks of a document may be used for, as the `documents` row says right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    /// `visibility = 'org'`: the whole organization may see it. Graph entities are
    /// organization-shared, so nothing else is extracted.
    pub visible: bool,
    pub classification: String,
}

/// The `graph_extraction_state.status` vocabulary, in the order of [`Status::ALL`]. Read by
/// tests/contract/postgres, which compares it with the migration's CHECK.
#[allow(dead_code)]
pub const STATUS_NAMES: [&str; 9] = [
    "pending",
    "ready",
    "empty",
    "skipped_zdr",
    "skipped_policy",
    "skipped_visibility",
    "skipped_budget",
    "failed_transient",
    "failed_permanent",
];

/// The `graph_extraction_state.error_class` vocabulary, in the order of [`ErrorClass::ALL`].
/// Read by tests/contract/postgres, like [`STATUS_NAMES`].
#[allow(dead_code)]
pub const ERROR_CLASS_NAMES: [&str; 12] = [
    "rate_limited",
    "provider_unavailable",
    "timeout",
    "auth",
    "zdr_unavailable",
    "zdr_org",
    "zdr_document",
    "residency_unavailable",
    "posture_unavailable",
    "invalid_output",
    "too_long",
    "internal",
];

/// The status of a chunk's extraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Pending,
    Ready,
    Empty,
    SkippedZdr,
    SkippedPolicy,
    SkippedVisibility,
    SkippedBudget,
    FailedTransient,
    FailedPermanent,
}

impl Status {
    #[cfg(test)]
    pub const ALL: [Status; 9] = [
        Status::Pending,
        Status::Ready,
        Status::Empty,
        Status::SkippedZdr,
        Status::SkippedPolicy,
        Status::SkippedVisibility,
        Status::SkippedBudget,
        Status::FailedTransient,
        Status::FailedPermanent,
    ];

    /// The value stored in `graph_extraction_state.status`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Empty => "empty",
            Self::SkippedZdr => "skipped_zdr",
            Self::SkippedPolicy => "skipped_policy",
            Self::SkippedVisibility => "skipped_visibility",
            Self::SkippedBudget => "skipped_budget",
            Self::FailedTransient => "failed_transient",
            Self::FailedPermanent => "failed_permanent",
        }
    }

    #[cfg(test)]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }

    /// Whether the status can never be due again on its own: `ready` and `failed_permanent`
    /// (and `empty` until the prompt version changes, which is the store's rule, not a
    /// property of the status).
    #[cfg(test)]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Ready | Self::FailedPermanent)
    }
}

/// The statuses a worker may pick up when they are due: everything that is not final and not
/// `empty` (whose rule is the prompt version's). The partial index `idx_ges_work` covers
/// exactly these, and a Go contract test compares the two.
pub const WORK_STATUSES: [Status; 6] = [
    Status::Pending,
    Status::FailedTransient,
    Status::SkippedZdr,
    Status::SkippedPolicy,
    Status::SkippedVisibility,
    Status::SkippedBudget,
];

/// The closed set of content-free labels a chunk's `error_class` takes. Free text is where
/// document content leaks into a column or a log line, so the set is closed and equals the
/// migration's CHECK.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    RateLimited,
    ProviderUnavailable,
    Timeout,
    Auth,
    /// No attested zero-retention route exists (inference-core refused before any provider was
    /// called).
    ZdrUnavailable,
    /// The ORGANIZATION is zero-retention.
    ZdrOrg,
    /// The DOCUMENT is classified restricted.
    ZdrDocument,
    ResidencyUnavailable,
    /// Auth Core could not resolve the organization's retention posture, so the content was
    /// neither sent nor skipped.
    PostureUnavailable,
    /// The model answered and the answer was not an extraction.
    InvalidOutput,
    TooLong,
    Internal,
}

impl ErrorClass {
    #[cfg(test)]
    pub const ALL: [ErrorClass; 12] = [
        ErrorClass::RateLimited,
        ErrorClass::ProviderUnavailable,
        ErrorClass::Timeout,
        ErrorClass::Auth,
        ErrorClass::ZdrUnavailable,
        ErrorClass::ZdrOrg,
        ErrorClass::ZdrDocument,
        ErrorClass::ResidencyUnavailable,
        ErrorClass::PostureUnavailable,
        ErrorClass::InvalidOutput,
        ErrorClass::TooLong,
        ErrorClass::Internal,
    ];

    /// The value stored in `graph_extraction_state.error_class`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RateLimited => "rate_limited",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::Timeout => "timeout",
            Self::Auth => "auth",
            Self::ZdrUnavailable => "zdr_unavailable",
            Self::ZdrOrg => "zdr_org",
            Self::ZdrDocument => "zdr_document",
            Self::ResidencyUnavailable => "residency_unavailable",
            Self::PostureUnavailable => "posture_unavailable",
            Self::InvalidOutput => "invalid_output",
            Self::TooLong => "too_long",
            Self::Internal => "internal",
        }
    }
}

/// Why content was withheld for zero retention. Three reasons, kept distinct so a restricted
/// document and a zero-retention organization can be told apart in the coverage view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZdrReason {
    /// The organization's retention posture is zero-retention (the bearer's `zdr` claim).
    /// Nothing is sent: no call is made at all.
    Org,
    /// The document is classified restricted. Nothing is sent.
    Document,
    /// inference-core refused because no attested zero-retention route exists.
    NoAttestedRoute,
}

impl ZdrReason {
    pub const fn error_class(self) -> ErrorClass {
        match self {
            Self::Org => ErrorClass::ZdrOrg,
            Self::Document => ErrorClass::ZdrDocument,
            Self::NoAttestedRoute => ErrorClass::ZdrUnavailable,
        }
    }
}

/// What a completed extraction is stamped with: which prompt and model produced it and what it
/// stored. Written with the status `ready` or `empty`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub prompt_version: String,
    /// `InferResponse.model_used`: what served the call, never what was asked for.
    pub model: String,
    /// `InferResponse.provider_used`.
    pub provider: String,
    pub entities: i32,
    pub relationships: i32,
    pub claims: i32,
}

impl Stamp {
    pub fn total(&self) -> i64 {
        i64::from(self.entities) + i64::from(self.relationships) + i64::from(self.claims)
    }
}

/// What one chunk's attempt came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The model answered and the answer was stored (or there was nothing to store).
    Extracted(Stamp),
    /// Withheld for zero retention. Zero provider calls were made, except for
    /// `ZdrReason::NoAttestedRoute`, which is inference-core's own refusal.
    SkippedZdr(ZdrReason),
    /// The organization's processing policy turns graph extraction off.
    SkippedPolicy,
    /// The chunk's document is not visible to the whole organization.
    SkippedVisibility,
    /// The organization's budget mode is `stop` and the budget is used up.
    SkippedBudget,
    /// Waiting for Auth Core to say what the organization's retention posture is. Never a send,
    /// never a skip for good.
    PostureUnavailable { retry_after: Duration },
    /// No deployment meets the residency floor. Loud on purpose: a misconfigured region would
    /// otherwise refuse every call.
    ResidencyUnavailable,
    /// Failed, retryable. `retry_after` is what the provider asked for, if it said.
    Transient {
        class: ErrorClass,
        retry_after: Option<Duration>,
    },
    /// Failed for good: retrying the same chunk cannot help.
    Permanent { class: ErrorClass },
}

/// What a chunk's row becomes. Written in one statement by the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub status: Status,
    pub error_class: Option<ErrorClass>,
    /// When the chunk is due again, measured from now. `None`: due whenever the gate or the
    /// scan lets it through (an org-level skip is lifted by the organization's own setting, not
    /// by the clock), or never (a final status).
    pub retry_in: Option<Duration>,
    /// Hand the attempt this claim counted back: the cause was not the chunk's.
    pub refund_attempt: bool,
    pub stamp: Option<Stamp>,
}

/// The wait after attempt number `attempt` (1-based) has failed.
pub fn backoff(attempt: i32) -> Duration {
    let index = usize::try_from(attempt.saturating_sub(1)).unwrap_or(0);
    LADDER[index.min(LADDER.len() - 1)]
}

impl Outcome {
    /// Whether a claim made for this outcome keeps its attempt. A posture, credential,
    /// zero-retention or budget cause is not the chunk's fault and must not walk it toward
    /// `failed_permanent`: only a failure of the work itself counts.
    pub const fn counts_as_attempt(&self) -> bool {
        match self {
            Self::Extracted(_) => true,
            Self::Transient { class, .. } => !matches!(class, ErrorClass::Auth),
            Self::Permanent { .. } => true,
            Self::SkippedZdr(_)
            | Self::SkippedPolicy
            | Self::SkippedVisibility
            | Self::SkippedBudget
            | Self::PostureUnavailable { .. }
            | Self::ResidencyUnavailable => false,
        }
    }

    /// Whether this outcome says something about the whole deployment or organization rather
    /// than about this chunk, so the remaining chunks should not ask: asking again only costs a
    /// call each (and, for a refusal nobody can lift, a refusal each).
    pub const fn is_systemic(&self) -> bool {
        match self {
            Self::SkippedZdr(ZdrReason::Org | ZdrReason::NoAttestedRoute)
            | Self::SkippedPolicy
            | Self::SkippedBudget
            | Self::PostureUnavailable { .. }
            | Self::ResidencyUnavailable
            | Self::Transient {
                class: ErrorClass::Auth | ErrorClass::RateLimited | ErrorClass::ProviderUnavailable,
                ..
            } => true,
            Self::Extracted(_)
            | Self::SkippedZdr(ZdrReason::Document)
            | Self::SkippedVisibility
            | Self::Transient { .. }
            | Self::Permanent { .. } => false,
        }
    }

    /// Whether the outcome is a failure of the attempt (for the pass's consecutive-failure
    /// breaker): a skip is a decision, not a failure.
    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::Transient { .. } | Self::Permanent { .. })
    }

    /// What the chunk's row becomes, given the attempts its claim made (the claim already
    /// counted this one) and the cap.
    pub fn settle(&self, attempts: i32, max_attempts: i32) -> Settlement {
        let refund_attempt = !self.counts_as_attempt();
        let settlement = |status, error_class, retry_in| Settlement {
            status,
            error_class,
            retry_in,
            refund_attempt,
            stamp: None,
        };
        match self {
            Self::Extracted(stamp) => Settlement {
                status: if stamp.total() > 0 {
                    Status::Ready
                } else {
                    Status::Empty
                },
                error_class: None,
                retry_in: None,
                refund_attempt: false,
                stamp: Some(stamp.clone()),
            },
            Self::SkippedZdr(reason) => settlement(
                Status::SkippedZdr,
                Some(reason.error_class()),
                match reason {
                    // The organization's or the document's own setting lifts these: the gate
                    // and the scan look at them on every pass.
                    ZdrReason::Org | ZdrReason::Document => None,
                    // inference-core's refusal has no cheap pre-check: ask again in an hour.
                    ZdrReason::NoAttestedRoute => Some(SKIP_RECHECK_AFTER),
                },
            ),
            Self::SkippedPolicy => settlement(Status::SkippedPolicy, None, None),
            Self::SkippedVisibility => settlement(Status::SkippedVisibility, None, None),
            Self::SkippedBudget => settlement(Status::SkippedBudget, None, None),
            Self::PostureUnavailable { retry_after } => settlement(
                Status::Pending,
                Some(ErrorClass::PostureUnavailable),
                Some(*retry_after),
            ),
            Self::ResidencyUnavailable => settlement(
                Status::FailedTransient,
                Some(ErrorClass::ResidencyUnavailable),
                Some(SKIP_RECHECK_AFTER),
            ),
            Self::Transient { class, retry_after } => {
                if matches!(class, ErrorClass::Auth) {
                    // A deployment problem to fix, not a property of the chunk: wait five
                    // minutes, never count it.
                    return settlement(
                        Status::FailedTransient,
                        Some(*class),
                        Some(PENDING_RETRY_AFTER),
                    );
                }
                if attempts >= max_attempts {
                    return settlement(Status::FailedPermanent, Some(*class), None);
                }
                // The provider's own wish wins when it is longer than the ladder; a shorter
                // one is subsumed by the ladder's floor (the reconciler ticks every five
                // minutes anyway).
                let wait = retry_after
                    .map(|after| after.min(MAX_RETRY_WAIT))
                    .unwrap_or_default()
                    .max(backoff(attempts));
                settlement(Status::FailedTransient, Some(*class), Some(wait))
            }
            Self::Permanent { class } => settlement(Status::FailedPermanent, Some(*class), None),
        }
    }
}

/// Turn a decoded failure into the outcome of the chunk it interrupted.
///
/// By stage:
///
/// **Minting the bearer** (Auth Core said no, or could not say). Never counts as an attempt,
/// and never a send:
///
/// * could not resolve the posture, could not verify the organization, was unreachable or
///   failed: `pending` for five minutes (`posture_unavailable`).
/// * refused the credential, the principal or the organization, or issued a token that does
///   not match the deployment: `failed_transient` / `auth`, five minutes.
///
/// **Calling inference-core:**
///
/// * zero-retention refusal: `skipped_zdr` / `zdr_unavailable`, asked again in an hour.
/// * residency refusal: `failed_transient` / `residency_unavailable`, hourly.
/// * tier refusal (this client never asks for a tier, so the server changed its contract):
///   `failed_transient` / `internal`, hourly.
/// * throttled: `failed_transient` / `rate_limited`, after the provider's own wait.
/// * unavailable, deadline, cancelled: `failed_transient` / `timeout` or `provider_unavailable`.
/// * internal: `failed_transient` / `internal`.
/// * input too long: `failed_permanent` / `too_long`.
/// * credential refused by inference-core: `failed_transient` / `auth`, five minutes.
/// * invalid request, or a status this client does not know: `failed_permanent` / `internal`.
///   Retrying a request that cannot succeed only re-sends the chunk.
pub fn outcome_for_failure(error: &InferenceError) -> Outcome {
    let failure = error.failure();
    match error.stage() {
        Stage::Mint => match failure {
            Failure::Auth | Failure::InvalidArgument | Failure::TooLong | Failure::Other => {
                Outcome::Transient {
                    class: ErrorClass::Auth,
                    retry_after: Some(PENDING_RETRY_AFTER),
                }
            }
            // Every other way a mint fails is a way of "Auth Core could not tell us": a wait.
            Failure::PostureUnavailable
            | Failure::DirectoryUnavailable
            | Failure::Transient { .. }
            | Failure::Internal
            | Failure::RateLimited { .. }
            | Failure::ZdrUnavailable
            | Failure::ResidencyUnavailable
            | Failure::TierUnavailable => Outcome::PostureUnavailable {
                retry_after: PENDING_RETRY_AFTER,
            },
        },
        Stage::Call => match failure {
            Failure::ZdrUnavailable => Outcome::SkippedZdr(ZdrReason::NoAttestedRoute),
            Failure::ResidencyUnavailable => Outcome::ResidencyUnavailable,
            Failure::TierUnavailable => Outcome::Transient {
                class: ErrorClass::Internal,
                retry_after: Some(SKIP_RECHECK_AFTER),
            },
            Failure::RateLimited { retry_after } => Outcome::Transient {
                class: ErrorClass::RateLimited,
                retry_after: *retry_after,
            },
            Failure::Transient { retry_after } => {
                let class = if matches!(
                    error.code(),
                    Some(tonic::Code::DeadlineExceeded | tonic::Code::Cancelled)
                ) {
                    ErrorClass::Timeout
                } else {
                    ErrorClass::ProviderUnavailable
                };
                Outcome::Transient {
                    class,
                    retry_after: *retry_after,
                }
            }
            Failure::Internal => Outcome::Transient {
                class: ErrorClass::Internal,
                retry_after: None,
            },
            Failure::TooLong => Outcome::Permanent {
                class: ErrorClass::TooLong,
            },
            Failure::Auth => Outcome::Transient {
                class: ErrorClass::Auth,
                retry_after: Some(PENDING_RETRY_AFTER),
            },
            Failure::InvalidArgument | Failure::Other => Outcome::Permanent {
                class: ErrorClass::Internal,
            },
            // The two Auth Core waits cannot arrive on a call; if a future decoder routes them
            // here they are still a wait, never a skip.
            Failure::PostureUnavailable | Failure::DirectoryUnavailable => {
                Outcome::PostureUnavailable {
                    retry_after: PENDING_RETRY_AFTER,
                }
            }
        },
    }
}

/// The outcome of a failure of any kind the extractor can return. Typed errors are decoded by
/// type; anything else is `internal` and transient (an unknown failure is never retried forever,
/// because it counts toward the cap, and never turned into a send).
pub fn outcome_for_error(error: &anyhow::Error) -> Outcome {
    if let Some(failure) = error.downcast_ref::<InferenceError>() {
        return outcome_for_failure(failure);
    }
    if error.downcast_ref::<ZdrOrganization>().is_some() {
        return Outcome::SkippedZdr(ZdrReason::Org);
    }
    if error.downcast_ref::<ExtractionTimeout>().is_some() {
        return Outcome::Transient {
            class: ErrorClass::Timeout,
            retry_after: None,
        };
    }
    if error.downcast_ref::<InvalidModelOutput>().is_some()
        || error.downcast_ref::<EmptyModelOutput>().is_some()
    {
        return Outcome::Transient {
            class: ErrorClass::InvalidOutput,
            retry_after: None,
        };
    }
    Outcome::Transient {
        class: ErrorClass::Internal,
        retry_after: None,
    }
}

/// The fields of a failure a log line carries as fields, not buried in a message: the decoded
/// kind, the gRPC code and the provider's own kind. Before this existed graph-index logged
/// `%e` of an error wrapped in a bare context, and eight live "model-plane inference failed"
/// lines said nothing else.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FailureFields {
    /// `Failure::kind()` of a decoded failure, or the typed local failure's name.
    pub kind: &'static str,
    /// `mint` or `call`, for an inference failure.
    pub stage: Option<&'static str>,
    /// The gRPC code (`Unavailable`), for a failure that came back from inference-core.
    pub grpc_code: Option<String>,
    /// The `x-mp-provider-error` kind, or the `error` Auth Core named in a 503.
    pub provider_error: Option<String>,
}

pub fn failure_fields(error: &anyhow::Error) -> FailureFields {
    if let Some(failure) = error.downcast_ref::<InferenceError>() {
        return FailureFields {
            kind: failure.failure().kind(),
            stage: Some(match failure.stage() {
                Stage::Mint => "mint",
                Stage::Call => "call",
            }),
            grpc_code: failure.code().map(|code| format!("{code:?}")),
            provider_error: failure.wire_kind().map(str::to_owned),
        };
    }
    let kind = if error.downcast_ref::<ZdrOrganization>().is_some() {
        "zdr_organization"
    } else if error.downcast_ref::<ExtractionTimeout>().is_some() {
        "extraction_timeout"
    } else if error.downcast_ref::<InvalidModelOutput>().is_some() {
        "invalid_model_output"
    } else if error.downcast_ref::<EmptyModelOutput>().is_some() {
        "empty_model_output"
    } else {
        "unclassified"
    };
    FailureFields {
        kind,
        ..FailureFields::default()
    }
}

#[cfg(test)]
mod tests;
