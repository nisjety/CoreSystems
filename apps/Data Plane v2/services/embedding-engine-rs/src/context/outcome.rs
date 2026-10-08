//! What generating one chunk's situating context came to, as a closed set.
//!
//! The first version of this code answered `Option<String>`: `Some(context)` or
//! `None`. `None` meant eleven different things (the document is restricted, the
//! organization is zero-retention, the provider throttled, the answer was empty,
//! the credential was refused, ...) and nothing downstream could tell them apart,
//! so an operator looking at a unit without a context could not say why it had
//! none, and a refusal that must never be retried looked the same as a blip that
//! should be. Every outcome here says what happened, what the unit's
//! `context_status` becomes, which content-free `context_error_class` goes with
//! it, and when to look again.
//!
//! ## Where the mapping comes from
//!
//! A failed call or mint is decoded by the shared client
//! (`inference_client_rs::InferenceError`) from the gRPC code and the
//! `x-mp-provider-error` trailer. [`outcome_for_failure`] is the one place that
//! turns that decoding into an outcome. It reads no message text. The table in the
//! tests is driven from the pinned copy of the status contract inference-core
//! generates, and fails when that contract gains a case nobody decided about.
//!
//! ## The vocabulary is the database's
//!
//! [`Status`] and [`ErrorClass`] are the values the CHECK constraints of migration
//! `20261003130000_context_provenance.sql` accept. A test reads the migration and
//! compares both sets, so neither can drift from the schema without a test
//! failing.
//!
//! ## What is not here
//!
//! Writing any of this to the database is the batch pipeline's job (CR-T8) and the
//! reconciler's (CR-T9a). Metering is only *captured* here ([`Usage`]); emitting
//! it is CR-T10.

use std::time::Duration;

use inference_client_rs::{
    Failure, InferenceError, Stage, PENDING_RETRY_AFTER, SKIP_RECHECK_AFTER,
};

use super::prompt::PromptVersion;

/// The longest a provider's `retry-after` is honoured when scheduling the next
/// attempt. The decoder carries up to a day (a subscription usage limit resets in
/// hours); a unit waiting that long would sit outside every backlog query.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(5 * 60);

/// The closed set of content-free labels a unit's `context_error_class` takes.
///
/// Free text is where document content leaks into a database column or a log, so
/// the set is closed and equals the migration's CHECK.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    RateLimited,
    ProviderUnavailable,
    Timeout,
    Auth,
    /// No attested zero-retention route exists (inference-core refused before any
    /// provider was called).
    ZdrUnavailable,
    /// The ORGANIZATION is zero-retention.
    ZdrOrg,
    /// The DOCUMENT is classified restricted.
    ZdrDocument,
    ResidencyUnavailable,
    /// Auth Core could not resolve the organization's retention posture, so the
    /// content was neither sent nor skipped.
    PostureUnavailable,
    ContentFilter,
    InvalidOutput,
    DocMissing,
    TooLong,
    Internal,
}

impl ErrorClass {
    pub const ALL: [ErrorClass; 14] = [
        ErrorClass::RateLimited,
        ErrorClass::ProviderUnavailable,
        ErrorClass::Timeout,
        ErrorClass::Auth,
        ErrorClass::ZdrUnavailable,
        ErrorClass::ZdrOrg,
        ErrorClass::ZdrDocument,
        ErrorClass::ResidencyUnavailable,
        ErrorClass::PostureUnavailable,
        ErrorClass::ContentFilter,
        ErrorClass::InvalidOutput,
        ErrorClass::DocMissing,
        ErrorClass::TooLong,
        ErrorClass::Internal,
    ];

    /// The value stored in `knowledge_units.context_error_class`.
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
            Self::ContentFilter => "content_filter",
            Self::InvalidOutput => "invalid_output",
            Self::DocMissing => "doc_missing",
            Self::TooLong => "too_long",
            Self::Internal => "internal",
        }
    }
}

/// The `context_status` an outcome moves a unit to.
///
/// `legacy` is deliberately not here: it is a state the migration gives rows whose
/// provenance was never recorded, never the result of generating anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Pending,
    Ready,
    SkippedZdr,
    SkippedPolicy,
    SkippedBudget,
    FailedTransient,
    FailedPermanent,
}

impl Status {
    pub const ALL: [Status; 7] = [
        Status::Pending,
        Status::Ready,
        Status::SkippedZdr,
        Status::SkippedPolicy,
        Status::SkippedBudget,
        Status::FailedTransient,
        Status::FailedPermanent,
    ];

    /// The value stored in `knowledge_units.context_status`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::SkippedZdr => "skipped_zdr",
            Self::SkippedPolicy => "skipped_policy",
            Self::SkippedBudget => "skipped_budget",
            Self::FailedTransient => "failed_transient",
            Self::FailedPermanent => "failed_permanent",
        }
    }
}

/// Why content was withheld for zero retention. Three reasons, kept distinct so a
/// restricted document and a zero-retention organization can be told apart in the
/// coverage view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZdrReason {
    /// The organization's retention posture is zero-retention (the bearer's `zdr`
    /// claim). Nothing is sent: no call is made at all.
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

/// Why a unit is waiting rather than failed or skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingReason {
    /// The inline time box ran out. The unit is embedded raw now and the
    /// reconciler fills the context in.
    DeadlineExceeded,
    /// Auth Core could not resolve the organization's retention posture (or could
    /// not be reached to resolve it) and issued no token. Never a send, never a
    /// skip for good.
    PostureUnavailable,
    /// The organization is over its background budget and its budget mode is
    /// `throttle` (CR-T9b): it is worked one chunk at a time, and this chunk is not the one
    /// that was let through. Due at once: the reconciler takes the next one at its own pace,
    /// and the pace is the throttle. A deferral, never a skip, and no error class: nothing
    /// went wrong.
    BudgetThrottled,
}

/// What a provider call used. Captured only: emitting it is CR-T10.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Prompt tokens. inference-core already folds the cache reads and writes in,
    /// so the two cache fields below are detail, not additions.
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Tokens served from the provider's prompt cache. A measurement, not a gate:
    /// Azure's automatic prefix cache needs a 1,024-token prefix and is best
    /// effort, so `0` is not a failure.
    pub cache_read_input_tokens: u32,
    pub cache_creation_input_tokens: u32,
    /// False when the provider reported no usage at all; the counts then mean
    /// "unknown", not "free".
    pub usage_reported: bool,
    /// True when inference-core answered from its completed-response cache and no
    /// provider was called: the call cost nothing and must not be metered.
    pub response_cache_hit: bool,
}

impl Usage {
    /// Whether a metering event should be emitted for this call.
    pub const fn is_billable(&self) -> bool {
        self.usage_reported && !self.response_cache_hit
    }
}

/// A context that was generated, validated and may be stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generated {
    /// The situating sentences, normalized, 12-600 characters.
    pub text: String,
    pub usage: Usage,
    /// The model that actually served the call (`InferResponse.model_used`).
    pub model_used: String,
    /// The registry id of the provider that served it (`provider_used`).
    pub provider_used: String,
    /// The residency label of the serving deployment, as disclosed.
    pub residency: String,
    /// The id of the request, for correlating with inference-core's logs.
    pub request_id: String,
    pub prompt_version: PromptVersion,
    /// `sha256` of what the context was generated from (see
    /// [`super::prompt::input_hash`]): what the pipeline stores to decide reuse and
    /// to compare-and-swap.
    pub input_hash: String,
    /// The model was cut off by its output cap and the text was shortened to its
    /// last full sentence.
    pub cut_at_sentence: bool,
    /// The text opens with a phrase the prompt forbids ("This chunk ..."). Kept,
    /// counted: the rate is a quality signal for the prompt, not a reason to
    /// discard a usable context.
    pub meta_opening: bool,
}

/// One provider call that completed, for metering. Every completed call is here,
/// including one whose answer was then rejected (an empty answer, a content-filter
/// stop): the provider billed for it all the same, and an outcome that carried no
/// usage would make that spend invisible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeteredCall {
    pub request_id: String,
    pub model_used: String,
    pub provider_used: String,
    pub usage: Usage,
}

/// What one chunk came to: its outcome, and every provider call made for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkResult {
    pub outcome: ContextOutcome,
    /// Completed provider calls, in order. Empty when none was made (a skip, a
    /// refused bearer, a call that failed before an answer).
    pub calls: Vec<MeteredCall>,
}

impl ChunkResult {
    /// An outcome reached without any provider call.
    pub fn without_calls(outcome: ContextOutcome) -> Self {
        Self {
            outcome,
            calls: Vec::new(),
        }
    }
}

/// The outcome of generating one chunk's context.
///
/// `Clone`, because a failure that applies to a whole document (the bearer was
/// refused, the organization is zero-retention) is the outcome of every chunk of
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextOutcome {
    Ready(Box<Generated>),
    /// Withheld for zero retention. Zero provider calls were made for this unit.
    SkippedZdr {
        reason: ZdrReason,
    },
    /// No deployment meets the residency floor. Zero provider calls were made.
    /// Loud on purpose: a misconfigured region would otherwise refuse every call.
    ResidencyUnavailable,
    /// The organization's processing policy is off (operator-set, with a reason).
    SkippedPolicy,
    /// The organization's budget mode is `stop` and the budget is used up.
    SkippedBudget,
    /// Waiting. `retry_after` is when the next attempt is due.
    Pending {
        retry_after: Duration,
        reason: PendingReason,
    },
    /// Failed, retryable. `retry_after` is what the provider asked for, capped at
    /// [`MAX_RETRY_AFTER`]; `None` leaves the schedule to the caller's ladder.
    Transient {
        retry_after: Option<Duration>,
        class: ErrorClass,
    },
    /// Failed for good until someone changes something.
    Permanent {
        class: ErrorClass,
    },
}

impl ContextOutcome {
    pub fn transient(class: ErrorClass, retry_after: Option<Duration>) -> Self {
        Self::Transient {
            retry_after: retry_after.map(|after| after.min(MAX_RETRY_AFTER)),
            class,
        }
    }

    pub const fn permanent(class: ErrorClass) -> Self {
        Self::Permanent { class }
    }

    pub const fn skipped_zdr(reason: ZdrReason) -> Self {
        Self::SkippedZdr { reason }
    }

    pub const fn pending(reason: PendingReason, retry_after: Duration) -> Self {
        Self::Pending {
            retry_after,
            reason,
        }
    }

    /// The `context_status` this outcome moves a unit to.
    pub const fn status(&self) -> Status {
        match self {
            Self::Ready(_) => Status::Ready,
            Self::SkippedZdr { .. } => Status::SkippedZdr,
            // A wait for configuration to change, visible as a failure so a
            // missing region variable cannot hide inside a backlog.
            Self::ResidencyUnavailable => Status::FailedTransient,
            Self::SkippedPolicy => Status::SkippedPolicy,
            Self::SkippedBudget => Status::SkippedBudget,
            Self::Pending { .. } => Status::Pending,
            Self::Transient { .. } => Status::FailedTransient,
            Self::Permanent { .. } => Status::FailedPermanent,
        }
    }

    /// The `context_error_class` that goes with it. The migration requires one for
    /// every `failed_*` and for `skipped_zdr`.
    pub const fn error_class(&self) -> Option<ErrorClass> {
        match self {
            Self::Ready(_) | Self::SkippedPolicy | Self::SkippedBudget => None,
            Self::SkippedZdr { reason } => Some(reason.error_class()),
            Self::ResidencyUnavailable => Some(ErrorClass::ResidencyUnavailable),
            Self::Pending { reason, .. } => match reason {
                PendingReason::DeadlineExceeded => Some(ErrorClass::Timeout),
                PendingReason::PostureUnavailable => Some(ErrorClass::PostureUnavailable),
                PendingReason::BudgetThrottled => None,
            },
            Self::Transient { class, .. } | Self::Permanent { class } => Some(*class),
        }
    }

    /// When the next attempt is due, measured from now. `None` means no attempt is
    /// scheduled by this outcome (it is final, or the caller's own ladder decides).
    pub const fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Ready(_) | Self::SkippedPolicy | Self::SkippedBudget | Self::Permanent { .. } => {
                None
            }
            // Re-checked, not forgotten: the organization's setting and the
            // attestations change. A restricted DOCUMENT is final.
            Self::SkippedZdr {
                reason: ZdrReason::Org | ZdrReason::NoAttestedRoute,
            }
            | Self::ResidencyUnavailable => Some(SKIP_RECHECK_AFTER),
            Self::SkippedZdr {
                reason: ZdrReason::Document,
            } => None,
            Self::Pending { retry_after, .. } => Some(*retry_after),
            Self::Transient { retry_after, .. } => *retry_after,
        }
    }

    /// Whether the unit's `context_attempts` should go up. A posture, credential
    /// or configuration problem is not the unit's fault and must not walk it
    /// toward `failed_permanent`: only a failure of the work itself counts.
    pub const fn counts_as_attempt(&self) -> bool {
        match self {
            Self::Transient { class, .. } => !matches!(class, ErrorClass::Auth),
            Self::Permanent { .. } => true,
            Self::Ready(_)
            | Self::SkippedZdr { .. }
            | Self::ResidencyUnavailable
            | Self::SkippedPolicy
            | Self::SkippedBudget
            | Self::Pending { .. } => false,
        }
    }

    /// The generated context, when there is one.
    pub fn generated(&self) -> Option<&Generated> {
        match self {
            Self::Ready(generated) => Some(generated),
            _ => None,
        }
    }

    /// Whether this outcome says something about the whole deployment rather than
    /// about this chunk, so the remaining chunks of the document should not ask.
    /// A refused bearer, a zero-retention organization and an unmet residency floor
    /// are the same answer for every chunk; asking again only costs a round trip
    /// each (and, for a floor nobody can meet, a refusal each).
    pub const fn is_systemic(&self) -> bool {
        match self {
            Self::SkippedZdr {
                reason: ZdrReason::Org | ZdrReason::NoAttestedRoute,
            }
            | Self::ResidencyUnavailable
            | Self::Pending {
                reason: PendingReason::PostureUnavailable,
                ..
            }
            | Self::Transient {
                class: ErrorClass::Auth,
                ..
            } => true,
            Self::Ready(_)
            | Self::SkippedZdr {
                reason: ZdrReason::Document,
            }
            | Self::SkippedPolicy
            | Self::SkippedBudget
            | Self::Pending {
                reason: PendingReason::DeadlineExceeded | PendingReason::BudgetThrottled,
                ..
            }
            | Self::Transient { .. }
            | Self::Permanent { .. } => false,
        }
    }
}

/// Turn a decoded failure into the outcome of the chunk it interrupted.
///
/// Called once the caller has finished its own bounded retries (see
/// [`inline_retry_wait`]): what comes out is what the chunk's status becomes.
///
/// The table, by stage:
///
/// **Minting the bearer** (Auth Core said no, or could not say). Never counts as an
/// attempt of the unit, and never a send:
///
/// * could not resolve the posture, could not verify the organization, was
///   unreachable or failed: `pending` for five minutes (`posture_unavailable`).
/// * refused the credential, the principal or the organization, or issued a token
///   that does not match the deployment: `failed_transient` / `auth`, five
///   minutes. A deployment problem to fix, not a property of the unit.
///
/// **Calling inference-core:**
///
/// * zero-retention refusal: `skipped_zdr` / `zdr_unavailable`, no retry.
/// * residency refusal: [`ContextOutcome::ResidencyUnavailable`], no retry.
/// * tier refusal (this client never asks for a tier, so the server changed its
///   contract): `failed_transient` / `internal`, hourly.
/// * throttled: `failed_transient` / `rate_limited`, after the provider's own wait.
/// * unavailable, deadline, cancelled: `failed_transient` / `timeout` or
///   `provider_unavailable`.
/// * internal: `failed_transient` / `internal`.
/// * input too long: `failed_permanent` / `too_long`.
/// * credential refused by inference-core: `failed_transient` / `auth`.
/// * invalid request, or a status this client does not know: `failed_permanent` /
///   `internal`. Retrying a request that cannot succeed only re-sends the document.
pub fn outcome_for_failure(error: &InferenceError) -> ContextOutcome {
    let failure = error.failure();
    match error.stage() {
        Stage::Mint => match failure {
            Failure::Auth | Failure::InvalidArgument | Failure::TooLong | Failure::Other => {
                ContextOutcome::transient(ErrorClass::Auth, Some(PENDING_RETRY_AFTER))
            }
            // Every other way a mint fails is a way of "Auth Core could not tell
            // us": a wait.
            Failure::PostureUnavailable
            | Failure::DirectoryUnavailable
            | Failure::Transient { .. }
            | Failure::Internal
            | Failure::RateLimited { .. }
            | Failure::ZdrUnavailable
            | Failure::ResidencyUnavailable
            | Failure::TierUnavailable => {
                ContextOutcome::pending(PendingReason::PostureUnavailable, PENDING_RETRY_AFTER)
            }
        },
        Stage::Call => match failure {
            Failure::ZdrUnavailable => ContextOutcome::skipped_zdr(ZdrReason::NoAttestedRoute),
            Failure::ResidencyUnavailable => ContextOutcome::ResidencyUnavailable,
            Failure::TierUnavailable => {
                ContextOutcome::transient(ErrorClass::Internal, Some(SKIP_RECHECK_AFTER))
            }
            Failure::RateLimited { retry_after } => {
                ContextOutcome::transient(ErrorClass::RateLimited, *retry_after)
            }
            Failure::Transient { retry_after } => {
                let class = if matches!(
                    error.code(),
                    Some(tonic::Code::DeadlineExceeded | tonic::Code::Cancelled)
                ) {
                    ErrorClass::Timeout
                } else {
                    ErrorClass::ProviderUnavailable
                };
                ContextOutcome::transient(class, *retry_after)
            }
            Failure::Internal => ContextOutcome::transient(ErrorClass::Internal, None),
            Failure::TooLong => ContextOutcome::permanent(ErrorClass::TooLong),
            Failure::Auth => ContextOutcome::transient(ErrorClass::Auth, Some(PENDING_RETRY_AFTER)),
            Failure::InvalidArgument | Failure::Other => {
                ContextOutcome::permanent(ErrorClass::Internal)
            }
            // The two Auth Core waits cannot arrive on a call; if a future decoder
            // routes them here they are still a wait, never a skip.
            Failure::PostureUnavailable | Failure::DirectoryUnavailable => {
                ContextOutcome::pending(PendingReason::PostureUnavailable, PENDING_RETRY_AFTER)
            }
        },
    }
}

/// How long to wait before retrying a failed call inside the caller's own bounded
/// retry, or `None` when the failure is not worth a retry here.
///
/// The provider's own wish wins: a rate limit that says "1,500 ms" is waited for
/// exactly that, not for a ladder step that happens to be shorter (and would be
/// refused again) or longer (and would waste the time box). Only when the
/// provider named no wait does the exponential ladder apply, with the per-chunk
/// offset that keeps a burst of throttled chunks from retrying in lockstep.
///
/// Only the failures the decoder dispositions as `Retry` are retried; a refusal,
/// a bad request, an unknown status and an Auth Core wait never are.
pub fn inline_retry_wait(
    error: &InferenceError,
    base: Duration,
    attempt: u32,
    chunk_index: usize,
) -> Option<Duration> {
    if error.stage() != Stage::Call {
        return None;
    }
    match error.failure() {
        Failure::RateLimited { retry_after } | Failure::Transient { retry_after } => Some(
            retry_after
                .map(|after| after.min(MAX_RETRY_AFTER))
                .unwrap_or_else(|| ladder_backoff(base, attempt, chunk_index)),
        ),
        Failure::Internal => Some(ladder_backoff(base, attempt, chunk_index)),
        Failure::ZdrUnavailable
        | Failure::ResidencyUnavailable
        | Failure::TierUnavailable
        | Failure::TooLong
        | Failure::Auth
        | Failure::InvalidArgument
        | Failure::PostureUnavailable
        | Failure::DirectoryUnavailable
        | Failure::Other => None,
    }
}

/// Backoff before retry `attempt` (0-based) for the chunk at `chunk_index`, when
/// the provider named no wait.
///
/// Exponential in the attempt, plus a deterministic per-chunk offset. The offset is
/// what keeps `concurrency` chunks that were rate-limited by the same upstream
/// burst from retrying in lockstep and re-triggering the same limit; spreading
/// them across the step is the point. It is derived from the chunk index rather
/// than drawn randomly so the schedule is reproducible in tests, and decorrelating
/// concurrent retries within one document is the only property the jitter needs.
pub fn ladder_backoff(base: Duration, attempt: u32, chunk_index: usize) -> Duration {
    let step = base.saturating_mul(1u32 << attempt.min(6));
    let spread = step / 4;
    let offset = if spread.is_zero() {
        Duration::ZERO
    } else {
        // `% 4` then scaled, so the offset stays inside one quarter-step.
        spread * ((chunk_index % 4) as u32) / 4
    };
    step + offset
}

#[cfg(test)]
mod tests;
