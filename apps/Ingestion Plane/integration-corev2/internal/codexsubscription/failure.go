package codexsubscription

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"time"
)

// FailureCode is the content-free public reason a subscription call failed.
// Every layer between the broker and the chat UI relays exactly these values
// (see the Errors section of the README); none of them is derived from
// upstream message text.
type FailureCode string

const (
	// FailureUsageLimit is the plan's Codex usage limit being exhausted, or a
	// plan without Codex usage or credits. The pinned app-server cannot tell
	// these apart, so they share the code; the direct transport logs a plan
	// without Codex as usage_not_included.
	FailureUsageLimit FailureCode = "subscription_usage_limit"
	// FailureReauthRequired means the ChatGPT sign-in must be renewed.
	FailureReauthRequired FailureCode = "subscription_reauth_required"
	// FailureUpstreamUnavailable is a transient Codex or ChatGPT failure that
	// Codex, or the direct transport, already retried.
	FailureUpstreamUnavailable FailureCode = "subscription_upstream_unavailable"
	// FailureContextTooLong is a prompt beyond the model's context window.
	FailureContextTooLong FailureCode = "subscription_context_too_long"
	// FailureBadRequest is ChatGPT rejecting the request (400/422 or policy).
	FailureBadRequest FailureCode = "subscription_bad_request"
	// FailureRateLimited is the broker queue timing out or a short-term
	// upstream throttle; Integration Core's request limiter sends the same
	// code. It does not say which, so it must not be worded as ChatGPT's.
	FailureRateLimited FailureCode = "rate_limited"
	// FailureBrokerFailed is anything else.
	FailureBrokerFailed FailureCode = "subscription_broker_failed"
)

// Failure is the content-free public classification of a failed call. The
// reset time and window length are account metadata: they carry no prompt,
// answer, plan type or upstream text.
type Failure struct {
	Code FailureCode
	// ResetsAt is when the exhausted usage window resets, in whole UTC
	// seconds; zero means unknown. Usage limit only.
	ResetsAt time.Time
	// RetryAfter is how long until a retry can succeed; zero means none is
	// known. For a usage limit with a known reset it is the time until
	// ResetsAt.
	RetryAfter time.Duration
	// WindowMinutes is the exhausted window's length; 0 means unknown. Usage
	// limit only.
	WindowMinutes int
	// LimitReachedType is an allowlisted RateLimitReachedType, "" when
	// unknown. Usage limit only.
	LimitReachedType string
}

// maxWindowMinutes bounds a reported window length; anything larger is
// treated as unknown rather than relayed.
const maxWindowMinutes = 366 * 24 * 60

// limitReachedTypes is the pinned Codex 0.153.4 RateLimitReachedType enum
// (app-server-protocol v2 account.rs), in its snake_case wire form.
var limitReachedTypes = map[string]struct{}{
	"rate_limit_reached":                   {},
	"workspace_owner_credits_depleted":     {},
	"workspace_member_credits_depleted":    {},
	"workspace_owner_usage_limit_reached":  {},
	"workspace_member_usage_limit_reached": {},
}

// allowedLimitReachedType keeps a limit-reached type only when it is one of
// the pinned enum values.
func allowedLimitReachedType(raw string) string {
	value := strings.ToLower(strings.TrimSpace(raw))
	if _, ok := limitReachedTypes[value]; ok {
		return value
	}
	return ""
}

// transientTurnCodes are the app-server codexErrorInfo variants (snake_case)
// for a failure Codex already retried or that is the service's own fault.
var transientTurnCodes = map[string]struct{}{
	"server_overloaded":                 {},
	"internal_server_error":             {},
	"http_connection_failed":            {},
	"response_stream_connection_failed": {},
	"response_stream_disconnected":      {},
}

// ClassifyFailure maps any error a transport or the manager returned to its
// public classification. It is the one mapping both transports and both
// routes use. It reads only typed, content-free fields.
func ClassifyFailure(err error, now time.Time) Failure {
	if err == nil {
		return Failure{Code: FailureBrokerFailed}
	}
	if errors.Is(err, ErrReauthenticationRequired) {
		return Failure{Code: FailureReauthRequired}
	}
	var limited *RateLimitError
	if errors.As(err, &limited) {
		if limited.Kind == RateLimitUsage {
			return usageFailure(limited, now)
		}
		return Failure{Code: FailureRateLimited, RetryAfter: clampRetryAfter(limited.RetryAfter)}
	}
	var upstream *UpstreamError
	if errors.As(err, &upstream) {
		if failure, ok := classifyUpstreamFailure(upstream); ok {
			return failure
		}
		return Failure{Code: FailureBrokerFailed}
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return Failure{Code: FailureUpstreamUnavailable}
	}
	return Failure{Code: FailureBrokerFailed}
}

// usageFailure keeps a reset only inside (now, now+7d], rounded up to a whole
// second, and derives RetryAfter from it; without one it keeps the
// transport's own retry hint, if any, under the same 7-day bound. A longer
// hint is dropped, not clamped: the UI shows RetryAfter as a reset time, and
// a clamped one would be earlier than the real reset.
func usageFailure(limited *RateLimitError, now time.Time) Failure {
	failure := Failure{Code: FailureUsageLimit, LimitReachedType: allowedLimitReachedType(limited.LimitReachedType)}
	if limited.WindowMinutes > 0 && limited.WindowMinutes <= maxWindowMinutes {
		failure.WindowMinutes = limited.WindowMinutes
	}
	if resetsAt, ok := knownReset(limited.ResetsAt, now); ok {
		failure.ResetsAt = resetsAt
		failure.RetryAfter = resetsAt.Sub(now)
		return failure
	}
	if limited.RetryAfter > 0 && limited.RetryAfter <= maxRetryAfter {
		failure.RetryAfter = clampRetryAfter(limited.RetryAfter)
	}
	return failure
}

// knownReset reports a reset in (now, now+7d], rounded up to a whole UTC
// second. Anything else is unknown.
func knownReset(resetsAt, now time.Time) (time.Time, bool) {
	if resetsAt.IsZero() || !resetsAt.After(now) || resetsAt.After(now.Add(maxRetryAfter)) {
		return time.Time{}, false
	}
	rounded := resetsAt.UTC().Truncate(time.Second)
	if rounded.Before(resetsAt) {
		rounded = rounded.Add(time.Second)
	}
	return rounded, true
}

// classifyUpstreamFailure maps a typed upstream failure of either transport.
// ok is false for anything that stays subscription_broker_failed.
func classifyUpstreamFailure(upstream *UpstreamError) (Failure, bool) {
	switch upstream.Kind {
	case upstreamKindNotIncluded, upstreamKindQuota:
		return Failure{Code: FailureUsageLimit}, true
	case upstreamKindContext:
		return Failure{Code: FailureContextTooLong}, true
	case upstreamKindPolicy:
		return Failure{Code: FailureBadRequest}, true
	case upstreamKindTurnFailed:
		return classifyTurnFailure(upstream)
	case upstreamKindTransport, upstreamKindIncomplete, upstreamKindTruncated, upstreamKindRefresh, upstreamKindUnauthorized:
		return upstreamUnavailable(upstream), true
	}
	if upstream.Code == "context_length_exceeded" {
		// A pre-stream 400 naming the context limit.
		return Failure{Code: FailureContextTooLong}, true
	}
	if upstream.Kind == upstreamKindHTTPStatus && (upstream.HTTPStatus == http.StatusBadRequest || upstream.HTTPStatus == http.StatusUnprocessableEntity) && !upstream.tierRejected {
		return Failure{Code: FailureBadRequest}, true
	}
	if upstream.Retryable {
		return upstreamUnavailable(upstream), true
	}
	return Failure{}, false
}

// classifyTurnFailure maps an app-server turn failure by its codexErrorInfo
// variant. badRequest is an app-server-internal fault, not an upstream 400,
// so it stays subscription_broker_failed with every other variant.
func classifyTurnFailure(upstream *UpstreamError) (Failure, bool) {
	switch code := upstream.Code; {
	case code == "context_window_exceeded":
		return Failure{Code: FailureContextTooLong}, true
	case code == "cyber_policy", code == "misalignment_policy_violation":
		return Failure{Code: FailureBadRequest}, true
	case code == "usage_limit_exceeded":
		return Failure{Code: FailureUsageLimit}, true
	case code == "rate_limit_exceeded",
		code == "response_too_many_failed_attempts" && upstream.HTTPStatus == http.StatusTooManyRequests:
		return Failure{Code: FailureRateLimited, RetryAfter: defaultThrottleRetry}, true
	case code == "response_too_many_failed_attempts":
		return upstreamUnavailable(upstream), true
	case code == upstreamCodeOther && upstream.upstreamRetries > 0:
		// Codex retries any unexpected status, then fails the turn as
		// `other` without one. Its retry notifications do name it: a
		// persistent client error (a 403 from a workspace without Codex, a
		// 404) will fail again, so it is not labelled transient.
		if persistentClientStatus(upstream.retryHTTPStatus) {
			return Failure{}, false
		}
		return upstreamUnavailable(upstream), true
	}
	if _, transient := transientTurnCodes[upstream.Code]; transient || upstream.Retryable {
		return upstreamUnavailable(upstream), true
	}
	return Failure{}, false
}

// persistentClientStatus reports a 4xx that a retry will not cure. 408
// (request timeout), 425 (too early) and 429 (throttle) are transient.
func persistentClientStatus(status int) bool {
	switch {
	case status < 400 || status > 499:
		return false
	case status == http.StatusRequestTimeout, status == http.StatusTooEarly, status == http.StatusTooManyRequests:
		return false
	}
	return true
}

func upstreamUnavailable(upstream *UpstreamError) Failure {
	failure := Failure{Code: FailureUpstreamUnavailable}
	if upstream.hasRetryAfter && upstream.retryAfter > 0 {
		failure.RetryAfter = clampRetryAfter(upstream.retryAfter)
	}
	return failure
}

// FormatResetsAt renders a known reset as RFC3339 UTC to whole seconds, ""
// when unknown.
func (f Failure) FormatResetsAt() string {
	if f.ResetsAt.IsZero() {
		return ""
	}
	return f.ResetsAt.UTC().Format(time.RFC3339)
}

// RetryAfterMs is RetryAfter in whole milliseconds, rounded up; 0 when none
// is known.
func (f Failure) RetryAfterMs() int64 {
	if f.RetryAfter <= 0 {
		return 0
	}
	ms := f.RetryAfter.Milliseconds()
	if time.Duration(ms)*time.Millisecond < f.RetryAfter {
		ms++
	}
	return ms
}

// RetryAfterSeconds is the whole-second Retry-After header value, rounded up
// and clamped to 1..604800; ok is false when no header should be sent.
func (f Failure) RetryAfterSeconds() (int64, bool) {
	if f.RetryAfter <= 0 {
		return 0, false
	}
	return min(CeilSeconds(f.RetryAfter), int64(maxRetryAfter/time.Second)), true
}
