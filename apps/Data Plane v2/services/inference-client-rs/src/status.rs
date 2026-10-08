//! Typed decoding of the ways a call to inference-core can fail.
//!
//! inference-core classifies its own failures and says so on the wire: the gRPC
//! code, plus the `x-mp-provider-error` trailer (a closed vocabulary of kinds),
//! plus `x-mp-retry-after-ms` when the provider said how long to wait. The shape
//! is pinned by `apps/Model Plane/api/contracts/infer-statuses.json`, which
//! inference-core generates from the code that builds the statuses; this crate
//! vendors a byte-identical copy and `tests/status_contract.rs` decodes every
//! case in it.
//!
//! The rule that matters: **never read the message**. A message is prose written
//! for a person, it can change in any release, and for a provider failure it may
//! echo what the provider received. Before the trailer existed callers matched on
//! text, and a reworded message silently turned a refusal into a retry. Here the
//! decision comes from the code and the kind only; the message is carried for the
//! log line and nothing else.
//!
//! What a caller does with a failure is also decided here, once, by
//! [`Failure::disposition`], because the cost of getting it wrong is asymmetric:
//! a zero-retention refusal must never be retried into a different route, and an
//! organization whose retention posture could not be resolved must never be
//! treated as either "send it" or "skip it for good".

use std::fmt;
use std::time::Duration;

use tonic::metadata::MetadataMap;
use tonic::Code;

/// gRPC trailer carrying inference-core's own classification of a failure.
pub const KIND_TRAILER: &str = "x-mp-provider-error";
/// gRPC trailer carrying how long to wait before a retry could succeed, as whole
/// milliseconds. Sent only when the provider said.
pub const RETRY_AFTER_MS_TRAILER: &str = "x-mp-retry-after-ms";

/// A request required zero data retention and no deployment has a verified ZDR
/// contract. Refused before any provider was called.
pub const KIND_ZDR_UNAVAILABLE: &str = "zdr_unavailable";
/// No deployment meets the residency floor. Refused before any provider was called.
pub const KIND_RESIDENCY_UNAVAILABLE: &str = "residency_unavailable";
/// No provider satisfies the requested minimum privacy tier.
pub const KIND_TIER_UNAVAILABLE: &str = "tier_unavailable";
/// The input was over the model's limit.
pub const KIND_TOO_LONG: &str = "too_long";
/// The provider throttled the call.
pub const KIND_RATE_LIMITED: &str = "rate_limited";
/// The ChatGPT-subscription route's usage limit. The Data Plane never selects
/// that route; the kind is still decoded as a rate limit so a future caller does
/// not mistake it for an outage.
pub const KIND_SUBSCRIPTION_USAGE_LIMIT: &str = "subscription_usage_limit";

/// How long work stays `pending` when the organization's retention posture could
/// not be resolved, before the next attempt.
pub const PENDING_RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
/// How long a refusal to send is remembered before it is looked at again. A
/// refusal is a decision about the organization's setting and the provider
/// attestations, which change on the scale of hours, not seconds.
pub const SKIP_RECHECK_AFTER: Duration = Duration::from_secs(60 * 60);

/// A provider can ask for an absurd wait (a usage limit resets in hours). Callers
/// decide what to do with it; the decoder only refuses to carry nonsense.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_KIND_LEN: usize = 64;
const MAX_DETAIL_CHARS: usize = 200;

/// The closed set of things that can go wrong, as far as a caller must tell them
/// apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Zero retention was required and no attested route exists. The content
    /// must not be sent anywhere else.
    ZdrUnavailable,
    /// No deployment meets the residency floor.
    ResidencyUnavailable,
    /// No provider meets the privacy tier floor.
    TierUnavailable,
    /// The provider throttled the call.
    RateLimited { retry_after: Option<Duration> },
    /// The input does not fit the model. Retrying the same input cannot help.
    TooLong,
    /// The service or the path to it was not available. Worth retrying.
    Transient { retry_after: Option<Duration> },
    /// inference-core failed internally, or the provider call behind an embedding
    /// failed. Worth a bounded retry; not worth waiting for.
    Internal,
    /// The credential, the principal or the organization binding was refused.
    /// Retrying does not help until someone fixes the deployment.
    Auth,
    /// The request itself was invalid.
    InvalidArgument,
    /// Auth Core could not resolve the organization's retention posture and issued
    /// no token (503 `retention_posture_unavailable`). A wait, not an answer.
    PostureUnavailable,
    /// Auth Core could not tell whether the organization exists and issued no
    /// token (503 `organization_directory_unavailable`). A wait, not an answer.
    DirectoryUnavailable,
    /// A status this client does not recognise. Treated as terminal: an unknown
    /// failure is never retried into sending content.
    Other,
}

impl Failure {
    /// A short, stable name for logs and metrics labels.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::ZdrUnavailable => "zdr_unavailable",
            Self::ResidencyUnavailable => "residency_unavailable",
            Self::TierUnavailable => "tier_unavailable",
            Self::RateLimited { .. } => "rate_limited",
            Self::TooLong => "too_long",
            Self::Transient { .. } => "transient",
            Self::Internal => "internal",
            Self::Auth => "auth",
            Self::InvalidArgument => "invalid_argument",
            Self::PostureUnavailable => "posture_unavailable",
            Self::DirectoryUnavailable => "directory_unavailable",
            Self::Other => "other",
        }
    }

    /// What the caller should do with the work.
    pub fn disposition(&self) -> Disposition {
        match self {
            Self::ZdrUnavailable | Self::ResidencyUnavailable | Self::TierUnavailable => {
                Disposition::Skip {
                    recheck_after: SKIP_RECHECK_AFTER,
                }
            }
            Self::RateLimited { retry_after } | Self::Transient { retry_after } => {
                Disposition::Retry {
                    after: *retry_after,
                }
            }
            Self::Internal => Disposition::Retry { after: None },
            Self::PostureUnavailable | Self::DirectoryUnavailable => Disposition::Pending {
                after: PENDING_RETRY_AFTER,
            },
            Self::TooLong | Self::Auth | Self::InvalidArgument | Self::Other => {
                Disposition::Terminal
            }
        }
    }
}

/// What to do with the work a failure interrupted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Try again soon, within the caller's own bounded retry. `after` is the
    /// provider's wish, when it gave one.
    Retry { after: Option<Duration> },
    /// The answer is not known yet. Keep the work pending and try again after
    /// the delay. **Never send it, and never give up on it.**
    Pending { after: Duration },
    /// This content must not go on this route. Mark it skipped with the reason,
    /// and look again after the delay: the organization's setting or the
    /// attestations may have changed. Never send it by another route.
    Skip { recheck_after: Duration },
    /// Retrying cannot help until something is fixed (a credential, the request,
    /// the size of the input).
    Terminal,
}

/// Where in the exchange a failure happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Getting a bearer from Auth Core.
    Mint,
    /// The call to inference-core itself.
    Call,
}

/// A failure, decoded. Implements [`std::error::Error`], so it travels inside an
/// `anyhow::Error` and a caller that cares recovers it with
/// `error.downcast_ref::<InferenceError>()`.
///
/// `Display` carries the failure's kind, the gRPC code and the provider kind, so
/// a log line says what happened. Wrapping this in a bare `.context("...")` and
/// logging the top of the chain throws all of that away: that is how a graph
/// extraction failure once showed only "model-plane inference failed". Log it
/// as is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceError {
    failure: Failure,
    stage: Stage,
    code: Option<Code>,
    wire_kind: Option<String>,
    detail: String,
    operation: Option<&'static str>,
}

impl InferenceError {
    /// Decode a status returned by inference-core. Reads the code and the two
    /// trailers; the message is kept for display and never inspected.
    pub fn from_status(status: &tonic::Status) -> Self {
        let wire_kind = kind_trailer(status.metadata());
        let retry_after = retry_after_trailer(status.metadata());
        Self {
            failure: classify(status.code(), wire_kind.as_deref(), retry_after),
            stage: Stage::Call,
            code: Some(status.code()),
            wire_kind,
            detail: sanitize(status.message()),
            operation: None,
        }
    }

    /// A failure of the token mint, already classified by the token client.
    pub(crate) fn from_mint(
        failure: Failure,
        wire_kind: Option<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            failure,
            stage: Stage::Mint,
            code: None,
            wire_kind,
            detail: sanitize(&detail.into()),
            operation: None,
        }
    }

    /// Name the operation in the message ("model-plane embedding failed: ...").
    #[must_use]
    pub fn during(mut self, operation: &'static str) -> Self {
        self.operation = Some(operation);
        self
    }

    pub fn failure(&self) -> &Failure {
        &self.failure
    }

    pub fn disposition(&self) -> Disposition {
        self.failure.disposition()
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// The gRPC code the failure arrived with. `None` for a mint failure.
    pub fn code(&self) -> Option<Code> {
        self.code
    }

    /// The `x-mp-provider-error` value, or, for a mint failure, the `error` Auth
    /// Core named in its 503 body.
    pub fn wire_kind(&self) -> Option<&str> {
        self.wire_kind.as_deref()
    }

    /// The server's own message, with control characters removed and the length
    /// capped. For a person reading a log; never for a decision.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for InferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = self.operation.unwrap_or(match self.stage {
            Stage::Mint => "inference service-token mint",
            Stage::Call => "inference call",
        });
        write!(formatter, "{operation} failed: {}", self.failure.kind())?;
        if let Some(code) = self.code {
            write!(formatter, " (grpc_code={code:?}")?;
            if let Some(kind) = &self.wire_kind {
                write!(formatter, ", provider_error={kind}")?;
            }
            if let Failure::RateLimited {
                retry_after: Some(after),
            }
            | Failure::Transient {
                retry_after: Some(after),
            } = &self.failure
            {
                write!(formatter, ", retry_after_ms={}", after.as_millis())?;
            }
            write!(formatter, ")")?;
        } else if let Some(kind) = &self.wire_kind {
            write!(formatter, " (auth_core_error={kind})")?;
        }
        if !self.detail.is_empty() {
            write!(formatter, ": {}", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for InferenceError {}

/// The decision table: the provider kind where there is one, the code otherwise.
fn classify(code: Code, kind: Option<&str>, retry_after: Option<Duration>) -> Failure {
    // The kind is authoritative. A posture refusal is a refusal whatever code it
    // rode on, and `too_long` rides on UNAVAILABLE (so a fallback chain can try a
    // larger model) which would otherwise read as a transient outage.
    match kind {
        Some(KIND_ZDR_UNAVAILABLE) => return Failure::ZdrUnavailable,
        Some(KIND_RESIDENCY_UNAVAILABLE) => return Failure::ResidencyUnavailable,
        Some(KIND_TIER_UNAVAILABLE) => return Failure::TierUnavailable,
        Some(KIND_TOO_LONG) => return Failure::TooLong,
        Some(KIND_RATE_LIMITED | KIND_SUBSCRIPTION_USAGE_LIMIT) => {
            return Failure::RateLimited { retry_after }
        }
        // Any other kind is one this client has not learned. It is not an
        // error to meet one: fall back to what the code says.
        _ => {}
    }
    match code {
        Code::ResourceExhausted => Failure::RateLimited { retry_after },
        Code::Unavailable | Code::DeadlineExceeded | Code::Aborted | Code::Cancelled => {
            Failure::Transient { retry_after }
        }
        Code::Unauthenticated | Code::PermissionDenied => Failure::Auth,
        Code::InvalidArgument | Code::OutOfRange => Failure::InvalidArgument,
        Code::Internal | Code::Unknown | Code::DataLoss => Failure::Internal,
        // FAILED_PRECONDITION without a recognised kind is what an inference-core
        // that predates the trailers sends for a ZDR refusal. It cannot be told
        // from any other precondition without reading the message, which this
        // client does not do, so it is terminal: never retried, never sent on.
        Code::FailedPrecondition | Code::NotFound | Code::Unimplemented | Code::AlreadyExists => {
            Failure::Other
        }
        Code::Ok => Failure::Other,
    }
}

/// The provider kind, if it is a well-formed token of the frozen vocabulary's
/// shape. Anything else is treated as absent so it cannot reach a log line or a
/// decision.
fn kind_trailer(metadata: &MetadataMap) -> Option<String> {
    let value = metadata.get(KIND_TRAILER)?.to_str().ok()?;
    let value = value.trim();
    let well_formed = !value.is_empty()
        && value.len() <= MAX_KIND_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    well_formed.then(|| value.to_owned())
}

fn retry_after_trailer(metadata: &MetadataMap) -> Option<Duration> {
    let value = metadata.get(RETRY_AFTER_MS_TRAILER)?.to_str().ok()?.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let millis: u64 = value.parse().ok()?;
    Some(Duration::from_millis(millis).min(MAX_RETRY_AFTER))
}

fn sanitize(message: &str) -> String {
    let cleaned: String = message
        .chars()
        .take(MAX_DETAIL_CHARS)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    cleaned.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::metadata::MetadataValue;
    use tonic::Status;

    fn status(code: Code, message: &str, trailers: &[(&'static str, &str)]) -> Status {
        let mut metadata = MetadataMap::new();
        for (key, value) in trailers {
            metadata.insert(*key, MetadataValue::try_from(*value).expect("ascii"));
        }
        Status::with_metadata(code, message, metadata)
    }

    #[test]
    fn the_kind_overrides_the_code_in_both_directions() {
        // too_long rides UNAVAILABLE so a fallback chain can try a larger model;
        // read by code alone it is a transient outage worth retrying forever.
        let too_long = status(Code::Unavailable, "", &[(KIND_TRAILER, "too_long")]);
        assert_eq!(
            InferenceError::from_status(&too_long).failure(),
            &Failure::TooLong
        );
        // A posture refusal is a refusal whatever code carried it.
        let zdr = status(Code::Unavailable, "", &[(KIND_TRAILER, "zdr_unavailable")]);
        assert_eq!(
            InferenceError::from_status(&zdr).failure(),
            &Failure::ZdrUnavailable
        );
    }

    #[test]
    fn the_message_is_never_consulted() {
        // The text of every other kind, on a status that carries no trailer.
        for message in [
            "zdr_unavailable",
            "no deployment has a verified zero data retention contract",
            "residency_unavailable",
            "rate_limited",
            "too_long",
        ] {
            let decoded =
                InferenceError::from_status(&status(Code::FailedPrecondition, message, &[]));
            assert_eq!(decoded.failure(), &Failure::Other, "{message}");
        }
        // And a typed status is not talked out of its kind by a misleading text.
        let decoded = InferenceError::from_status(&status(
            Code::FailedPrecondition,
            "service unavailable, please retry",
            &[(KIND_TRAILER, "zdr_unavailable")],
        ));
        assert_eq!(decoded.failure(), &Failure::ZdrUnavailable);
    }

    #[test]
    fn an_unlearned_kind_falls_back_to_the_code() {
        let decoded = InferenceError::from_status(&status(
            Code::Unavailable,
            "",
            &[(KIND_TRAILER, "some_future_kind")],
        ));
        assert_eq!(decoded.failure(), &Failure::Transient { retry_after: None });
        assert_eq!(decoded.wire_kind(), Some("some_future_kind"));
    }

    #[test]
    fn a_malformed_kind_trailer_is_ignored() {
        for hostile in ["ZDR_UNAVAILABLE", "zdr unavailable", "zdr-unavailable", ""] {
            let decoded = InferenceError::from_status(&status(
                Code::Unavailable,
                "",
                &[(KIND_TRAILER, hostile)],
            ));
            assert_eq!(decoded.wire_kind(), None, "{hostile:?}");
            assert_eq!(
                decoded.failure(),
                &Failure::Transient { retry_after: None },
                "{hostile:?}"
            );
        }
        let too_long_value = "a".repeat(MAX_KIND_LEN + 1);
        let decoded = InferenceError::from_status(&status(
            Code::Unavailable,
            "",
            &[(KIND_TRAILER, too_long_value.as_str())],
        ));
        assert_eq!(decoded.wire_kind(), None);
    }

    #[test]
    fn the_retry_after_is_whole_milliseconds_and_bounded() {
        let ok = status(
            Code::ResourceExhausted,
            "",
            &[
                (KIND_TRAILER, "rate_limited"),
                (RETRY_AFTER_MS_TRAILER, "1500"),
            ],
        );
        assert_eq!(
            InferenceError::from_status(&ok).failure(),
            &Failure::RateLimited {
                retry_after: Some(Duration::from_millis(1500))
            }
        );
        for bad in ["", "soon", "-1", "1.5", "1e3", "0x10", " "] {
            let decoded = InferenceError::from_status(&status(
                Code::ResourceExhausted,
                "",
                &[(RETRY_AFTER_MS_TRAILER, bad)],
            ));
            assert_eq!(
                decoded.failure(),
                &Failure::RateLimited { retry_after: None },
                "{bad:?}"
            );
        }
        let absurd = status(
            Code::ResourceExhausted,
            "",
            &[(RETRY_AFTER_MS_TRAILER, "99999999999999")],
        );
        assert_eq!(
            InferenceError::from_status(&absurd).failure(),
            &Failure::RateLimited {
                retry_after: Some(MAX_RETRY_AFTER)
            }
        );
        let overflow = status(
            Code::ResourceExhausted,
            "",
            &[(RETRY_AFTER_MS_TRAILER, "999999999999999999999999")],
        );
        assert_eq!(
            InferenceError::from_status(&overflow).failure(),
            &Failure::RateLimited { retry_after: None }
        );
    }

    #[test]
    fn every_untyped_code_has_one_decision() {
        let cases = [
            (Code::Cancelled, Failure::Transient { retry_after: None }),
            (Code::Unknown, Failure::Internal),
            (Code::InvalidArgument, Failure::InvalidArgument),
            (
                Code::DeadlineExceeded,
                Failure::Transient { retry_after: None },
            ),
            (Code::NotFound, Failure::Other),
            (Code::AlreadyExists, Failure::Other),
            (Code::PermissionDenied, Failure::Auth),
            (
                Code::ResourceExhausted,
                Failure::RateLimited { retry_after: None },
            ),
            (Code::FailedPrecondition, Failure::Other),
            (Code::Aborted, Failure::Transient { retry_after: None }),
            (Code::OutOfRange, Failure::InvalidArgument),
            (Code::Unimplemented, Failure::Other),
            (Code::Internal, Failure::Internal),
            (Code::Unavailable, Failure::Transient { retry_after: None }),
            (Code::DataLoss, Failure::Internal),
            (Code::Unauthenticated, Failure::Auth),
        ];
        for (code, expected) in cases {
            assert_eq!(
                InferenceError::from_status(&status(code, "", &[])).failure(),
                &expected,
                "{code:?}"
            );
        }
    }

    #[test]
    fn what_to_do_is_decided_once() {
        let hour = SKIP_RECHECK_AFTER;
        assert_eq!(
            Failure::ZdrUnavailable.disposition(),
            Disposition::Skip {
                recheck_after: hour
            }
        );
        assert_eq!(
            Failure::ResidencyUnavailable.disposition(),
            Disposition::Skip {
                recheck_after: hour
            }
        );
        assert_eq!(
            Failure::TierUnavailable.disposition(),
            Disposition::Skip {
                recheck_after: hour
            }
        );
        // The two Auth Core waits: pending, five minutes, never a skip.
        for failure in [Failure::PostureUnavailable, Failure::DirectoryUnavailable] {
            assert_eq!(
                failure.disposition(),
                Disposition::Pending {
                    after: Duration::from_secs(300)
                },
                "{failure:?}"
            );
        }
        let wait = Some(Duration::from_secs(7));
        assert_eq!(
            Failure::RateLimited { retry_after: wait }.disposition(),
            Disposition::Retry { after: wait }
        );
        assert_eq!(
            Failure::Transient { retry_after: None }.disposition(),
            Disposition::Retry { after: None }
        );
        assert_eq!(
            Failure::Internal.disposition(),
            Disposition::Retry { after: None }
        );
        for failure in [
            Failure::TooLong,
            Failure::Auth,
            Failure::InvalidArgument,
            Failure::Other,
        ] {
            assert_eq!(failure.disposition(), Disposition::Terminal, "{failure:?}");
        }
    }

    #[test]
    fn display_names_the_kind_the_code_and_the_provider_kind() {
        let rendered = InferenceError::from_status(&status(
            Code::FailedPrecondition,
            "no deployment is attested\nfor zero retention",
            &[(KIND_TRAILER, "zdr_unavailable")],
        ))
        .during("model-plane embedding")
        .to_string();
        assert!(rendered.starts_with("model-plane embedding failed: zdr_unavailable"));
        assert!(
            rendered.contains("grpc_code=FailedPrecondition"),
            "{rendered}"
        );
        assert!(
            rendered.contains("provider_error=zdr_unavailable"),
            "{rendered}"
        );
        assert!(!rendered.contains('\n'), "a log line stays one line");

        let throttled = InferenceError::from_status(&status(
            Code::ResourceExhausted,
            "",
            &[
                (KIND_TRAILER, "rate_limited"),
                (RETRY_AFTER_MS_TRAILER, "1500"),
            ],
        ))
        .to_string();
        assert!(throttled.contains("retry_after_ms=1500"), "{throttled}");
    }

    #[test]
    fn the_detail_is_one_bounded_line() {
        let long = format!("{}\r\n{}", "x".repeat(500), "tail");
        let decoded = InferenceError::from_status(&status(Code::Internal, &long, &[]));
        assert!(decoded.detail().chars().count() <= MAX_DETAIL_CHARS);
        assert!(!decoded.detail().contains(['\n', '\r']));
    }

    #[test]
    fn it_travels_inside_anyhow_and_is_recovered_by_type() {
        let error = anyhow_like(InferenceError::from_status(&status(
            Code::Unavailable,
            "",
            &[],
        )));
        assert!(error
            .downcast_ref::<InferenceError>()
            .is_some_and(|inner| inner.stage() == Stage::Call));
    }

    /// `anyhow` is not a dependency of this crate; `Box<dyn Error>` is what it
    /// wraps, and `downcast_ref` on it is the same operation.
    fn anyhow_like(error: InferenceError) -> Box<dyn std::error::Error + Send + Sync> {
        Box::new(error)
    }
}
