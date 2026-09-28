//! ChatGPT-subscription refusals, as inference-core types them.
//!
//! inference-core is the only layer that reads the subscription broker's
//! reply, so it classifies a refusal once and declares it on gRPC trailers:
//! the reason code on `x-mp-provider-error`, and `x-mp-retry-after-ms` and
//! `x-mp-resets-at` where they are known. The gateway reads those and nothing
//! else. The status message is never parsed, and never shown on the
//! subscription route.
//!
//! The spellings are duplicated as literals rather than imported, because the
//! two crates are deployed separately and neither depends on the other; the
//! `subscription_failure_contract` test pins them together.
//!
//! Every message here says that no other model was used, because none was:
//! a refused subscription turn is never answered by another model or provider.

use chrono::{DateTime, TimeDelta, Timelike, Utc};

/// The furthest ahead a wait may put a retry and still be shown: the longest
/// Codex usage window (seven days) plus an hour, as inference-core bounds a
/// reported reset. A longer wait is treated as unknown.
const MAX_RETRY_AHEAD: TimeDelta = TimeDelta::hours(7 * 24 + 1);

/// The connected plan's Codex usage limit is exhausted.
pub const SUBSCRIPTION_USAGE_LIMIT: &str = "subscription_usage_limit";
/// The ChatGPT sign-in must be renewed.
pub const SUBSCRIPTION_REAUTH_REQUIRED: &str = "subscription_reauth_required";
/// ChatGPT or Codex failed transiently, after Codex's own retries.
pub const SUBSCRIPTION_UPSTREAM_UNAVAILABLE: &str = "subscription_upstream_unavailable";
/// ChatGPT rejected the request itself.
pub const SUBSCRIPTION_BAD_REQUEST: &str = "subscription_bad_request";
/// A short-term throttle. Honoured on the subscription route only: other
/// providers send it too, and their turns keep their own handling.
pub const RATE_LIMITED: &str = "rate_limited";
/// The gateway's own code for a subscription failure with no typed reason.
pub const SUBSCRIPTION_ROUTE_UNAVAILABLE: &str = "subscription_route_unavailable";
/// The gateway's code for a prompt still too long after compaction.
pub const PROMPT_TOO_LONG: &str = "prompt_too_long";

/// Trailer with the wait before a retry could succeed, in whole milliseconds.
pub const RETRY_AFTER_MS_TRAILER: &str = "x-mp-retry-after-ms";
/// Trailer with a usage limit's reset time, RFC 3339 UTC to whole seconds.
pub const RESETS_AT_TRAILER: &str = "x-mp-resets-at";

/// The reason codes inference-core may declare for a subscription refusal.
const TRAILER_CODES: [&str; 5] = [
    SUBSCRIPTION_USAGE_LIMIT,
    SUBSCRIPTION_REAUTH_REQUIRED,
    SUBSCRIPTION_UPSTREAM_UNAVAILABLE,
    SUBSCRIPTION_BAD_REQUEST,
    RATE_LIMITED,
];

/// A typed refusal: its code and the content-free timing that came with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionFailure {
    pub code: &'static str,
    pub retry_after_ms: Option<u64>,
    pub resets_at: Option<DateTime<Utc>>,
}

impl SubscriptionFailure {
    /// A failure known only by its code.
    #[must_use]
    pub const fn bare(code: &'static str) -> Self {
        Self {
            code,
            retry_after_ms: None,
            resets_at: None,
        }
    }

    /// The reset time as the wire carries it: RFC 3339 UTC, whole seconds.
    #[must_use]
    pub fn resets_at_label(&self) -> Option<String> {
        self.resets_at
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    }

    /// Whole seconds to wait before retrying, rounded up and at least one,
    /// when a wait is known.
    #[must_use]
    pub fn retry_after_secs(&self) -> Option<u64> {
        self.retry_after_ms.map(|ms| ms.div_ceil(1_000).max(1))
    }

    /// The instant a retry could succeed: the reported reset when known,
    /// else `now` plus the declared wait, rounded up to a whole second.
    ///
    /// `retry_after_ms` is relative to when the gateway read it, so a frame
    /// that is buffered and replayed must carry this instant instead of
    /// letting the client add the wait to its own later clock. `None` when
    /// neither is known, or the wait is beyond any usage window.
    #[must_use]
    pub fn retry_at(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if let Some(at) = self.resets_at {
            return Some(at);
        }
        let wait = TimeDelta::try_milliseconds(i64::try_from(self.retry_after_ms?).ok()?)?;
        if wait > MAX_RETRY_AHEAD {
            return None;
        }
        let at = now.checked_add_signed(wait)?;
        if at.nanosecond() == 0 {
            Some(at)
        } else {
            at.with_nanosecond(0)?
                .checked_add_signed(TimeDelta::seconds(1))
        }
    }

    /// [`Self::retry_at`] as the wire carries it: RFC 3339 UTC, whole seconds.
    #[must_use]
    pub fn retry_at_label(&self, now: DateTime<Utc>) -> Option<String> {
        self.retry_at(now)
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    }
}

/// The typed refusal a failed inference call declares, if any.
///
/// Reads the reason trailer against the exact five codes; an unknown value is
/// no typed failure. A timing trailer that does not parse is dropped on its
/// own, so a bad number or date never costs the reason.
#[must_use]
pub fn subscription_failure(status: &tonic::Status) -> Option<SubscriptionFailure> {
    let metadata = status.metadata();
    let text = |key: &str| {
        metadata
            .get(key)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
    };
    let kind = text(crate::compaction::PROVIDER_ERROR_KIND_TRAILER)?;
    let code = TRAILER_CODES.into_iter().find(|code| *code == kind)?;
    let retry_after_ms = text(RETRY_AFTER_MS_TRAILER).and_then(|value| value.parse::<u64>().ok());
    let resets_at = text(RESETS_AT_TRAILER)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|at| at.with_timezone(&Utc));
    Some(SubscriptionFailure {
        code,
        retry_after_ms,
        resets_at,
    })
}

/// [`subscription_failure`] for a turn on or off the subscription route:
/// `rate_limited` counts only on it, since every provider can throttle.
#[must_use]
pub fn on_route(status: &tonic::Status, subscription_route: bool) -> Option<SubscriptionFailure> {
    subscription_failure(status)
        .filter(|failure| subscription_route || failure.code != RATE_LIMITED)
}

/// The static spelling of a code this module words, if `code` is one: the
/// five trailer codes, the generic route failure, and `prompt_too_long`,
/// which a subscription tool round can end on.
#[must_use]
pub fn known_code(code: &str) -> Option<&'static str> {
    TRAILER_CODES
        .into_iter()
        .chain([SUBSCRIPTION_ROUTE_UNAVAILABLE, PROMPT_TOO_LONG])
        .find(|known| *known == code)
}

/// The failure a failed call on the subscription route ends the turn with.
///
/// A typed refusal names its reason. An overlong prompt is `prompt_too_long`,
/// never the retryable generic failure: a retry of the same prompt cannot
/// succeed. Anything else is the generic `subscription_route_unavailable`.
/// Off the route, only a reason every provider shares is read, and nothing
/// else is typed.
#[must_use]
pub fn on_route_or_generic(
    status: &tonic::Status,
    subscription_route: bool,
) -> Option<SubscriptionFailure> {
    on_route(status, subscription_route).or_else(|| {
        subscription_route.then(|| {
            SubscriptionFailure::bare(if crate::compaction::is_context_length_status(status) {
                PROMPT_TOO_LONG
            } else {
                SUBSCRIPTION_ROUTE_UNAVAILABLE
            })
        })
    })
}

/// What the failed call was producing, which decides one sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureContext {
    /// The streamed answer.
    Answer,
    /// A tool-decision round.
    ToolRound,
    /// A source check or repair, or an answer held behind a staged artifact:
    /// the draft was not published.
    Draft,
    /// A one-message request (`/v1/invoke`): there is no history to compact
    /// or leave behind, so an overlong prompt is the message itself.
    Request,
}

const NO_OTHER_MODEL: &str = "Ingen annen modell ble brukt.";
const DRAFT_NOT_PUBLISHED: &str = "Utkastet er ikke publisert.";

/// The Norwegian explanation shown and persisted for a failure.
///
/// The gateway cannot know the viewer's time zone, so a reset time is written
/// in UTC; the SPA re-renders it locally from `resets_at`.
#[must_use]
pub fn message(failure: &SubscriptionFailure, context: FailureContext) -> String {
    message_at(failure, context, Utc::now())
}

/// [`message`] as of `now`, which dates a reset known only as a wait.
///
/// A usage limit whose reset time was not reported but whose wait was (the
/// direct transport's `Retry-After`) names `now` plus that wait, as the SPA
/// does for the same frame, rather than calling the reset unknown.
#[must_use]
pub fn message_at(
    failure: &SubscriptionFailure,
    context: FailureContext,
    now: DateTime<Utc>,
) -> String {
    let body = match failure.code {
        SUBSCRIPTION_USAGE_LIMIT => match failure.retry_at(now) {
            Some(at) => format!(
                "Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles {}.",
                at.format("%d.%m.%Y kl. %H:%M UTC")
            ),
            None => "Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Tidspunktet for \
                     tilbakestilling er ukjent; sjekk bruken i ChatGPT og prøv igjen senere."
                .to_owned(),
        },
        SUBSCRIPTION_REAUTH_REQUIRED => "ChatGPT-tilkoblingen må fornyes. Koble til ChatGPT på nytt \
                                         under Innstillinger → Integrasjoner, og prøv igjen."
            .to_owned(),
        SUBSCRIPTION_UPSTREAM_UNAVAILABLE => "ChatGPT svarte ikke akkurat nå. Prøv igjen om litt.".to_owned(),
        // Worded without naming who throttles: `rate_limited` is ChatGPT's
        // short-term throttle, but also the broker's own full queue, which
        // ChatGPT has no part in.
        RATE_LIMITED => match failure.retry_after_secs() {
            Some(1) => "Det går for mange forespørsler til ChatGPT-abonnementet akkurat nå. Prøv \
                        igjen om 1 sekund."
                .to_owned(),
            Some(seconds) => format!(
                "Det går for mange forespørsler til ChatGPT-abonnementet akkurat nå. Prøv igjen om \
                 {seconds} sekunder."
            ),
            None => "Det går for mange forespørsler til ChatGPT-abonnementet akkurat nå. Prøv igjen \
                     om litt."
                .to_owned(),
        },
        SUBSCRIPTION_BAD_REQUEST => {
            "ChatGPT avviste forespørselen. Endre eller kort ned meldingen, og prøv igjen.".to_owned()
        }
        PROMPT_TOO_LONG if context == FailureContext::Request => {
            "Meldingen er for lang for ChatGPT-modellen. Kort den ned, og prøv igjen.".to_owned()
        }
        PROMPT_TOO_LONG => {
            "Samtalen er for lang for ChatGPT-modellen, selv etter komprimering. Start en ny tråd."
                .to_owned()
        }
        _ => "Den valgte ChatGPT-abonnementstilkoblingen kunne ikke fullføre svaret. Prøv igjen om litt."
            .to_owned(),
    };
    match context {
        FailureContext::Draft => format!("{body} {DRAFT_NOT_PUBLISHED} {NO_OTHER_MODEL}"),
        FailureContext::Answer | FailureContext::ToolRound | FailureContext::Request => {
            format!("{body} {NO_OTHER_MODEL}")
        }
    }
}

/// Whether the client should offer a retry. A usage limit waits for its
/// reset, a sign-in for a reconnect, and a rejected or overlong request for a
/// change; only a transient failure is worth retrying as it stands.
#[must_use]
pub fn retryable(code: &str) -> bool {
    matches!(
        code,
        SUBSCRIPTION_UPSTREAM_UNAVAILABLE | RATE_LIMITED | SUBSCRIPTION_ROUTE_UNAVAILABLE
    )
}

/// The HTTP status of a typed failure on the unary routes.
#[must_use]
pub fn http_status(code: &str) -> axum::http::StatusCode {
    use axum::http::StatusCode;
    match code {
        SUBSCRIPTION_USAGE_LIMIT | RATE_LIMITED => StatusCode::TOO_MANY_REQUESTS,
        SUBSCRIPTION_REAUTH_REQUIRED => StatusCode::CONFLICT,
        SUBSCRIPTION_UPSTREAM_UNAVAILABLE => StatusCode::SERVICE_UNAVAILABLE,
        SUBSCRIPTION_BAD_REQUEST => StatusCode::UNPROCESSABLE_ENTITY,
        // The broker's own status for the same overflow.
        PROMPT_TOO_LONG => StatusCode::PAYLOAD_TOO_LARGE,
        _ => StatusCode::BAD_GATEWAY,
    }
}

/// Whole seconds until a retry could succeed, for a `Retry-After` header:
/// until the reset for a usage limit (clamped to 1..=604800), else the
/// declared wait. `None` when neither is known.
#[must_use]
pub fn retry_after_header_secs(failure: &SubscriptionFailure, now: DateTime<Utc>) -> Option<u64> {
    if let Some(at) = failure.resets_at {
        let millis = u64::try_from((at - now).num_milliseconds()).unwrap_or(0);
        return Some(millis.div_ceil(1_000).clamp(1, 604_800));
    }
    failure.retry_after_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(trailers: &[(&'static str, &str)]) -> tonic::Status {
        let mut metadata = tonic::metadata::MetadataMap::new();
        for (key, value) in trailers {
            metadata.insert(*key, value.parse().unwrap());
        }
        tonic::Status::with_metadata(tonic::Code::Unavailable, "subscription refused", metadata)
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn reads_each_of_the_five_codes_and_nothing_else() {
        for code in TRAILER_CODES {
            let failure = subscription_failure(&status(&[("x-mp-provider-error", code)]))
                .unwrap_or_else(|| panic!("{code} is typed"));
            assert_eq!(failure, SubscriptionFailure::bare(code));
        }
        for other in [
            "too_long",
            "subscription_broker_failed",
            "SUBSCRIPTION_USAGE_LIMIT",
            "",
        ] {
            assert_eq!(
                subscription_failure(&status(&[("x-mp-provider-error", other)])),
                None,
                "{other}"
            );
        }
        assert_eq!(
            subscription_failure(&tonic::Status::unavailable("subscription_usage_limit")),
            None,
            "the message is never read"
        );
    }

    #[test]
    fn reads_the_timing_and_drops_only_a_bad_field() {
        let failure = subscription_failure(&status(&[
            ("x-mp-provider-error", "subscription_usage_limit"),
            ("x-mp-retry-after-ms", "52260000"),
            ("x-mp-resets-at", "2026-09-26T01:21:00Z"),
        ]))
        .unwrap();
        assert_eq!(failure.retry_after_ms, Some(52_260_000));
        assert_eq!(failure.resets_at, Some(at("2026-09-26T01:21:00Z")));
        assert_eq!(
            failure.resets_at_label().as_deref(),
            Some("2026-09-26T01:21:00Z")
        );

        let failure = subscription_failure(&status(&[
            ("x-mp-provider-error", "subscription_usage_limit"),
            ("x-mp-retry-after-ms", "-5"),
            ("x-mp-resets-at", "tomorrow"),
        ]))
        .unwrap();
        assert_eq!(failure, SubscriptionFailure::bare(SUBSCRIPTION_USAGE_LIMIT));
    }

    #[test]
    fn a_throttle_counts_only_on_the_subscription_route() {
        let throttled = status(&[
            ("x-mp-provider-error", "rate_limited"),
            ("x-mp-retry-after-ms", "1500"),
        ]);
        assert_eq!(
            on_route(&throttled, true).map(|f| f.code),
            Some(RATE_LIMITED)
        );
        assert_eq!(on_route(&throttled, false), None);
        let usage = status(&[("x-mp-provider-error", "subscription_usage_limit")]);
        assert!(on_route(&usage, false).is_some());
    }

    #[test]
    fn a_usage_limit_names_its_reset_in_utc_or_says_it_is_unknown() {
        let known = SubscriptionFailure {
            resets_at: Some(at("2026-09-26T01:21:00Z")),
            ..SubscriptionFailure::bare(SUBSCRIPTION_USAGE_LIMIT)
        };
        assert_eq!(
            message(&known, FailureContext::Answer),
            "Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles \
             26.09.2026 kl. 01:21 UTC. Ingen annen modell ble brukt."
        );
        assert_eq!(
            message(
                &SubscriptionFailure::bare(SUBSCRIPTION_USAGE_LIMIT),
                FailureContext::Answer
            ),
            "Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Tidspunktet for \
             tilbakestilling er ukjent; sjekk bruken i ChatGPT og prøv igjen senere. Ingen annen \
             modell ble brukt."
        );
    }

    /// The direct transport's `Retry-After`-only usage limit: the gateway
    /// dates the reset from the wait, as the SPA does for the same frame, so
    /// the saved explanation and the live one agree. A reported reset wins.
    #[test]
    fn a_usage_limit_known_only_by_its_wait_is_dated_from_now() {
        let now = at("2026-09-25T10:50:00Z");
        let wait_only = SubscriptionFailure {
            retry_after_ms: Some(52_260_000),
            ..SubscriptionFailure::bare(SUBSCRIPTION_USAGE_LIMIT)
        };
        assert_eq!(
            message_at(&wait_only, FailureContext::ToolRound, now),
            "Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles \
             26.09.2026 kl. 01:21 UTC. Ingen annen modell ble brukt."
        );
        let both = SubscriptionFailure {
            resets_at: Some(at("2026-09-26T03:00:00Z")),
            ..wait_only.clone()
        };
        assert!(
            message_at(&both, FailureContext::Answer, now).contains("26.09.2026 kl. 03:00 UTC"),
            "the reported reset wins over the wait"
        );
        // A wait beyond any usage window is not believed.
        let absurd = SubscriptionFailure {
            retry_after_ms: Some(30 * 24 * 3_600_000),
            ..SubscriptionFailure::bare(SUBSCRIPTION_USAGE_LIMIT)
        };
        let absurd_text = message_at(&absurd, FailureContext::Answer, now);
        assert!(
            absurd_text.contains("Tidspunktet for tilbakestilling er ukjent"),
            "{absurd_text}"
        );
    }

    #[test]
    fn the_retry_instant_is_the_reset_or_now_plus_the_wait_rounded_up() {
        let now = at("2026-09-25T10:50:00.250Z");
        let throttled = |ms| SubscriptionFailure {
            retry_after_ms: ms,
            ..SubscriptionFailure::bare(RATE_LIMITED)
        };
        assert_eq!(
            throttled(Some(1_000)).retry_at_label(now).as_deref(),
            Some("2026-09-25T10:50:02Z"),
            "10:50:01.250 rounds up to the next whole second"
        );
        assert_eq!(
            throttled(Some(750))
                .retry_at_label(at("2026-09-25T10:50:00.250Z"))
                .as_deref(),
            Some("2026-09-25T10:50:01Z"),
            "exactly on a second is kept"
        );
        assert_eq!(throttled(None).retry_at(now), None);
        let reset = SubscriptionFailure {
            resets_at: Some(at("2026-09-26T01:21:00Z")),
            retry_after_ms: Some(1),
            ..SubscriptionFailure::bare(SUBSCRIPTION_USAGE_LIMIT)
        };
        assert_eq!(reset.retry_at(now), Some(at("2026-09-26T01:21:00Z")));
        assert_eq!(throttled(Some(u64::MAX)).retry_at(now), None);
    }

    /// Neutral wording: `rate_limited` is also the broker's own full queue,
    /// so the text never says `ChatGPT` is the one throttling. One second is
    /// singular, as in the SPA.
    #[test]
    fn a_throttle_rounds_its_wait_up_to_whole_seconds() {
        let throttled = |ms| SubscriptionFailure {
            retry_after_ms: ms,
            ..SubscriptionFailure::bare(RATE_LIMITED)
        };
        for (ms, wait) in [
            (1, "1 sekund"),
            (1_000, "1 sekund"),
            (0, "1 sekund"),
            (1_001, "2 sekunder"),
            (59_500, "60 sekunder"),
        ] {
            assert_eq!(
                message(&throttled(Some(ms)), FailureContext::ToolRound),
                format!(
                    "Det går for mange forespørsler til ChatGPT-abonnementet akkurat nå. Prøv igjen \
                     om {wait}. Ingen annen modell ble brukt."
                ),
                "{ms} ms"
            );
        }
        assert_eq!(
            message(&throttled(None), FailureContext::Answer),
            "Det går for mange forespørsler til ChatGPT-abonnementet akkurat nå. Prøv igjen om \
             litt. Ingen annen modell ble brukt."
        );
        assert!(
            !message(&throttled(Some(1_000)), FailureContext::Answer).contains("ChatGPT begrenser"),
            "never blames ChatGPT for the broker's own queue"
        );
    }

    /// An overlong one-message request cannot be compacted or left behind in
    /// an old thread: it says to shorten the message.
    #[test]
    fn an_overlong_request_says_to_shorten_the_message() {
        let too_long = SubscriptionFailure::bare(PROMPT_TOO_LONG);
        assert_eq!(
            message(&too_long, FailureContext::Request),
            "Meldingen er for lang for ChatGPT-modellen. Kort den ned, og prøv igjen. Ingen annen \
             modell ble brukt."
        );
        assert!(message(&too_long, FailureContext::Answer).contains("Start en ny tråd."));
        assert_eq!(
            http_status(PROMPT_TOO_LONG),
            axum::http::StatusCode::PAYLOAD_TOO_LARGE
        );
        assert!(!retryable(PROMPT_TOO_LONG));
    }

    #[test]
    fn on_the_route_an_overflow_is_prompt_too_long_and_anything_else_generic() {
        let overflow = status(&[("x-mp-provider-error", "too_long")]);
        assert_eq!(
            on_route_or_generic(&overflow, true),
            Some(SubscriptionFailure::bare(PROMPT_TOO_LONG))
        );
        assert_eq!(on_route_or_generic(&overflow, false), None);
        assert_eq!(
            on_route_or_generic(&tonic::Status::unavailable("all providers exhausted"), true),
            Some(SubscriptionFailure::bare(SUBSCRIPTION_ROUTE_UNAVAILABLE))
        );
        let usage = status(&[("x-mp-provider-error", "subscription_usage_limit")]);
        assert_eq!(
            on_route_or_generic(&usage, true).map(|failure| failure.code),
            Some(SUBSCRIPTION_USAGE_LIMIT)
        );
    }

    #[test]
    fn a_draft_says_it_was_not_published_before_the_last_sentence() {
        for code in [
            SUBSCRIPTION_USAGE_LIMIT,
            SUBSCRIPTION_REAUTH_REQUIRED,
            SUBSCRIPTION_UPSTREAM_UNAVAILABLE,
            SUBSCRIPTION_BAD_REQUEST,
            RATE_LIMITED,
            SUBSCRIPTION_ROUTE_UNAVAILABLE,
            PROMPT_TOO_LONG,
        ] {
            let failure = SubscriptionFailure::bare(code);
            let answer = message(&failure, FailureContext::Answer);
            let draft = message(&failure, FailureContext::Draft);
            assert!(
                answer.ends_with(" Ingen annen modell ble brukt."),
                "{code}: {answer}"
            );
            assert!(!answer.contains("Utkastet"), "{code}");
            assert_eq!(
                draft,
                answer.replace(
                    " Ingen annen modell ble brukt.",
                    " Utkastet er ikke publisert. Ingen annen modell ble brukt."
                ),
                "{code}"
            );
            assert_eq!(
                message(&failure, FailureContext::ToolRound),
                answer,
                "{code}"
            );
            if code != PROMPT_TOO_LONG {
                assert_eq!(message(&failure, FailureContext::Request), answer, "{code}");
            }
        }
    }

    #[test]
    fn every_code_has_its_own_wording() {
        let texts: std::collections::BTreeSet<String> = [
            SUBSCRIPTION_USAGE_LIMIT,
            SUBSCRIPTION_REAUTH_REQUIRED,
            SUBSCRIPTION_UPSTREAM_UNAVAILABLE,
            SUBSCRIPTION_BAD_REQUEST,
            RATE_LIMITED,
            SUBSCRIPTION_ROUTE_UNAVAILABLE,
            PROMPT_TOO_LONG,
        ]
        .into_iter()
        .map(|code| message(&SubscriptionFailure::bare(code), FailureContext::Answer))
        .collect();
        assert_eq!(texts.len(), 7);
        let reauth = message(
            &SubscriptionFailure::bare(SUBSCRIPTION_REAUTH_REQUIRED),
            FailureContext::Answer,
        );
        assert!(reauth.contains("Innstillinger → Integrasjoner"), "{reauth}");
    }

    #[test]
    fn only_transient_failures_are_retryable() {
        for (code, expected) in [
            (SUBSCRIPTION_USAGE_LIMIT, false),
            (SUBSCRIPTION_REAUTH_REQUIRED, false),
            (SUBSCRIPTION_UPSTREAM_UNAVAILABLE, true),
            (SUBSCRIPTION_BAD_REQUEST, false),
            (RATE_LIMITED, true),
            (SUBSCRIPTION_ROUTE_UNAVAILABLE, true),
            (PROMPT_TOO_LONG, false),
        ] {
            assert_eq!(retryable(code), expected, "{code}");
        }
    }

    #[test]
    fn a_retry_after_header_counts_to_the_reset_or_the_declared_wait() {
        let now = at("2026-09-25T10:50:00Z");
        let usage = SubscriptionFailure {
            resets_at: Some(at("2026-09-26T01:21:00.200Z")),
            retry_after_ms: Some(1_000),
            ..SubscriptionFailure::bare(SUBSCRIPTION_USAGE_LIMIT)
        };
        assert_eq!(retry_after_header_secs(&usage, now), Some(52_261));
        let passed = SubscriptionFailure {
            resets_at: Some(at("2026-09-25T10:00:00Z")),
            ..SubscriptionFailure::bare(SUBSCRIPTION_USAGE_LIMIT)
        };
        assert_eq!(retry_after_header_secs(&passed, now), Some(1));
        let throttled = SubscriptionFailure {
            retry_after_ms: Some(1_500),
            ..SubscriptionFailure::bare(RATE_LIMITED)
        };
        assert_eq!(retry_after_header_secs(&throttled, now), Some(2));
        assert_eq!(
            retry_after_header_secs(
                &SubscriptionFailure::bare(SUBSCRIPTION_REAUTH_REQUIRED),
                now
            ),
            None
        );
    }

    #[test]
    fn known_codes_are_the_five_plus_the_gateway_generic_and_overflow() {
        for code in TRAILER_CODES
            .into_iter()
            .chain([SUBSCRIPTION_ROUTE_UNAVAILABLE, PROMPT_TOO_LONG])
        {
            assert_eq!(known_code(code), Some(code));
        }
        assert_eq!(known_code("too_long"), None);
        assert_eq!(known_code("audit_write_failed"), None);
    }
}
