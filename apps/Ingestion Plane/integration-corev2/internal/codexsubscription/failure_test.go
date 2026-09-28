package codexsubscription

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"testing"
	"time"
)

// TestClassifyFailureCoversEveryReason walks every row of the reason table:
// each typed error of either transport maps to exactly one public code.
func TestClassifyFailureCoversEveryReason(t *testing.T) {
	now := time.Unix(1_800_000_000, 0).UTC()
	reset := now.Add(90 * time.Minute)
	cases := []struct {
		name string
		err  error
		want Failure
	}{
		// Usage limit.
		{name: "usage with a reset", err: &RateLimitError{Kind: RateLimitUsage, ResetsAt: reset, RetryAfter: time.Minute, WindowMinutes: 300, LimitReachedType: "workspace_member_usage_limit_reached"},
			want: Failure{Code: FailureUsageLimit, ResetsAt: reset, RetryAfter: 90 * time.Minute, WindowMinutes: 300, LimitReachedType: "workspace_member_usage_limit_reached"}},
		{name: "usage reset rounded up to a second", err: &RateLimitError{Kind: RateLimitUsage, ResetsAt: reset.Add(-300 * time.Millisecond)},
			want: Failure{Code: FailureUsageLimit, ResetsAt: reset, RetryAfter: 90 * time.Minute}},
		{name: "usage reset beyond 7 days is unknown", err: &RateLimitError{Kind: RateLimitUsage, ResetsAt: now.Add(8 * 24 * time.Hour)}, want: Failure{Code: FailureUsageLimit}},
		{name: "usage reset in the past is unknown", err: &RateLimitError{Kind: RateLimitUsage, ResetsAt: now.Add(-time.Minute)}, want: Failure{Code: FailureUsageLimit}},
		{name: "usage with only a retry hint", err: &RateLimitError{Kind: RateLimitUsage, RetryAfter: 30 * time.Second}, want: Failure{Code: FailureUsageLimit, RetryAfter: 30 * time.Second}},
		{name: "usage retry hint beyond 7 days is unknown", err: &RateLimitError{Kind: RateLimitUsage, RetryAfter: 8 * 24 * time.Hour}, want: Failure{Code: FailureUsageLimit}},
		{name: "usage drops an unknown limit type and a bad window", err: &RateLimitError{Kind: RateLimitUsage, LimitReachedType: "SENTINEL-type", WindowMinutes: -5}, want: Failure{Code: FailureUsageLimit}},
		{name: "wrapped usage", err: fmt.Errorf("invoke: %w", &RateLimitError{Kind: RateLimitUsage}), want: Failure{Code: FailureUsageLimit}},
		{name: "usage not included", err: &UpstreamError{Kind: upstreamKindNotIncluded, HTTPStatus: 429, Code: "usage_not_included"}, want: Failure{Code: FailureUsageLimit}},
		{name: "quota", err: &UpstreamError{Kind: upstreamKindQuota, Code: "insufficient_quota"}, want: Failure{Code: FailureUsageLimit}},
		// Reauthentication.
		{name: "reauth", err: ErrReauthenticationRequired, want: Failure{Code: FailureReauthRequired}},
		{name: "wrapped reauth", err: fmt.Errorf("refresh: %w", ErrReauthenticationRequired), want: Failure{Code: FailureReauthRequired}},
		// Upstream unavailable.
		{name: "direct 503", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 503, Retryable: true}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "direct 503 with a hint", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 503, Retryable: true, retryAfter: 20 * time.Second, hasRetryAfter: true}, want: Failure{Code: FailureUpstreamUnavailable, RetryAfter: 20 * time.Second}},
		{name: "direct 408", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 408, Retryable: true}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "retryable response.failed", err: &UpstreamError{Kind: upstreamKindFailed, Code: "server_error", Retryable: true}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "transport", err: &UpstreamError{Kind: upstreamKindTransport, Code: "eof", Retryable: true}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "incomplete", err: &UpstreamError{Kind: upstreamKindIncomplete, Code: "max_output_tokens"}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "truncated", err: &UpstreamError{Kind: upstreamKindTruncated, Retryable: true}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "refresh unavailable", err: &UpstreamError{Kind: upstreamKindRefresh, Code: "write_back_pending", Retryable: true}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "broker deadline", err: fmt.Errorf("send: %w", context.DeadlineExceeded), want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "server overloaded", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "server_overloaded"}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "internal server error", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "internal_server_error"}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "http connection failed", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "http_connection_failed", HTTPStatus: 502}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "stream connection failed", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "response_stream_connection_failed"}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "stream disconnected", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "response_stream_disconnected"}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "retry limit on a 5xx", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "response_too_many_failed_attempts", HTTPStatus: 500}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "other after retries", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "other", upstreamRetries: 2}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "other after retries of a 504", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "other", upstreamRetries: 2, retryHTTPStatus: 504}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "other after retries of a 408", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "other", upstreamRetries: 1, retryHTTPStatus: 408}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "other after retries of a 429", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "other", upstreamRetries: 1, retryHTTPStatus: 429}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "direct 401 after a recovered refresh", err: &UpstreamError{Kind: upstreamKindUnauthorized, HTTPStatus: 401, Code: "unauthorized", Retryable: true}, want: Failure{Code: FailureUpstreamUnavailable}},
		{name: "unauthorized after a recovered refresh", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "unauthorized", Retryable: true}, want: Failure{Code: FailureUpstreamUnavailable}},
		// Context too long.
		{name: "direct context", err: &UpstreamError{Kind: upstreamKindContext, Code: "context_length_exceeded"}, want: Failure{Code: FailureContextTooLong}},
		{name: "direct 400 naming the context", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 400, Code: "context_length_exceeded"}, want: Failure{Code: FailureContextTooLong}},
		{name: "app-server context", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "context_window_exceeded"}, want: Failure{Code: FailureContextTooLong}},
		// Bad request.
		{name: "direct 400", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 400, Code: "invalid_value"}, want: Failure{Code: FailureBadRequest}},
		{name: "direct 422", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 422}, want: Failure{Code: FailureBadRequest}},
		{name: "direct policy", err: &UpstreamError{Kind: upstreamKindPolicy, Code: "invalid_prompt"}, want: Failure{Code: FailureBadRequest}},
		{name: "cyber policy", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "cyber_policy"}, want: Failure{Code: FailureBadRequest}},
		{name: "misalignment policy", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "misalignment_policy_violation"}, want: Failure{Code: FailureBadRequest}},
		// Rate limited.
		{name: "throttle", err: &RateLimitError{Kind: RateLimitThrottle, RetryAfter: 2500 * time.Millisecond}, want: Failure{Code: FailureRateLimited, RetryAfter: 2500 * time.Millisecond}},
		{name: "queue", err: &RateLimitError{Kind: RateLimitQueue, RetryAfter: time.Second}, want: Failure{Code: FailureRateLimited, RetryAfter: time.Second}},
		{name: "app-server rate limit", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "rate_limit_exceeded"}, want: Failure{Code: FailureRateLimited, RetryAfter: time.Second}},
		{name: "retry limit on a 429", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "response_too_many_failed_attempts", HTTPStatus: 429}, want: Failure{Code: FailureRateLimited, RetryAfter: time.Second}},
		// Everything else.
		{name: "nil", err: nil, want: Failure{Code: FailureBrokerFailed}},
		{name: "plain error", err: errors.New("codex app-server closed before the turn completed"), want: Failure{Code: FailureBrokerFailed}},
		{name: "canceled", err: fmt.Errorf("send: %w", context.Canceled), want: Failure{Code: FailureBrokerFailed}},
		{name: "invalid request", err: ErrInvalidRequest, want: Failure{Code: FailureBrokerFailed}},
		{name: "direct 400 naming service_tier", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 400, tierRejected: true}, want: Failure{Code: FailureBrokerFailed}},
		{name: "direct 403", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 403, Code: "forbidden"}, want: Failure{Code: FailureBrokerFailed}},
		{name: "direct 404", err: &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: 404}, want: Failure{Code: FailureBrokerFailed}},
		{name: "unretryable response.failed", err: &UpstreamError{Kind: upstreamKindFailed, Code: "other"}, want: Failure{Code: FailureBrokerFailed}},
		{name: "malformed stream", err: &UpstreamError{Kind: upstreamKindMalformed}, want: Failure{Code: FailureBrokerFailed}},
		{name: "oversized event", err: &UpstreamError{Kind: upstreamKindTooLarge}, want: Failure{Code: FailureBrokerFailed}},
		{name: "empty answer", err: &UpstreamError{Kind: upstreamKindEmpty}, want: Failure{Code: FailureBrokerFailed}},
		{name: "served model mismatch", err: &UpstreamError{Kind: upstreamKindMismatch}, want: Failure{Code: FailureBrokerFailed}},
		{name: "other without retries", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "other"}, want: Failure{Code: FailureBrokerFailed}},
		{name: "other after retries of a 403", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "other", upstreamRetries: 3, retryHTTPStatus: 403}, want: Failure{Code: FailureBrokerFailed}},
		{name: "other after retries of a 404", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "other", upstreamRetries: 3, retryHTTPStatus: 404}, want: Failure{Code: FailureBrokerFailed}},
		{name: "other after retries of a 401", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "other", upstreamRetries: 1, retryHTTPStatus: 401}, want: Failure{Code: FailureBrokerFailed}},
		{name: "app-server bad request", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "bad_request"}, want: Failure{Code: FailureBrokerFailed}},
		{name: "session budget", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "session_budget_exceeded"}, want: Failure{Code: FailureBrokerFailed}},
		{name: "sandbox", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "sandbox_error"}, want: Failure{Code: FailureBrokerFailed}},
		{name: "unrecognized variant", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "unrecognized", HTTPStatus: 503}, want: Failure{Code: FailureBrokerFailed}},
		{name: "unauthorized without a recovery", err: &UpstreamError{Kind: upstreamKindTurnFailed, Code: "unauthorized"}, want: Failure{Code: FailureBrokerFailed}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := ClassifyFailure(tc.err, now); got != tc.want {
				t.Fatalf("ClassifyFailure(%#v) = %+v, want %+v", tc.err, got, tc.want)
			}
		})
	}
}

func TestFailureWireValues(t *testing.T) {
	failure := Failure{Code: FailureUsageLimit, ResetsAt: time.Date(2026, 9, 26, 1, 21, 0, 0, time.UTC), RetryAfter: 52260 * time.Second}
	if failure.FormatResetsAt() != "2026-09-26T01:21:00Z" || failure.RetryAfterMs() != 52260000 {
		t.Fatalf("resetsAt = %q retryAfterMs = %d", failure.FormatResetsAt(), failure.RetryAfterMs())
	}
	if seconds, ok := failure.RetryAfterSeconds(); !ok || seconds != 52260 {
		t.Fatalf("Retry-After = %d, %v", seconds, ok)
	}
	// Rounded up, never below a second, never above 7 days.
	if ms := (Failure{RetryAfter: 1500 * time.Microsecond}).RetryAfterMs(); ms != 2 {
		t.Fatalf("retryAfterMs of 1.5ms = %d, want 2", ms)
	}
	if seconds, _ := (Failure{RetryAfter: time.Millisecond}).RetryAfterSeconds(); seconds != 1 {
		t.Fatalf("Retry-After of 1ms = %d, want 1", seconds)
	}
	if seconds, _ := (Failure{RetryAfter: 30 * 24 * time.Hour}).RetryAfterSeconds(); seconds != 604800 {
		t.Fatalf("Retry-After of 30 days = %d, want 604800", seconds)
	}
	if _, ok := (Failure{Code: FailureUsageLimit}).RetryAfterSeconds(); ok {
		t.Fatal("an unknown reset must send no Retry-After")
	}
	if (Failure{}).FormatResetsAt() != "" || (Failure{}).RetryAfterMs() != 0 {
		t.Fatal("unknown values must be empty")
	}
}

// TestDirectUsageLimitReset pins the direct transport's reset sources.
func TestDirectUsageLimitReset(t *testing.T) {
	now := time.Unix(1_800_000_000, 0).UTC()
	resetsAt := now.Add(3 * time.Hour)
	unix := strconv.FormatInt(resetsAt.Unix(), 10)
	later := strconv.FormatInt(resetsAt.Add(24*time.Hour).Unix(), 10)
	cases := []struct {
		name     string
		body     string
		headers  http.Header
		resetsAt time.Time
		retry    time.Duration
		window   int
		limit    string
	}{
		{name: "resets_at", body: `{"error":{"type":"usage_limit_reached","message":"` + fakeCanary + `","plan_type":"plus","resets_at":` + unix + `}}`, resetsAt: resetsAt, retry: 3 * time.Hour},
		{
			name: "only the secondary window is exhausted",
			body: `{"error":{"type":"usage_limit_reached"}}`,
			headers: http.Header{
				"X-Codex-Primary-Used-Percent": {"35"}, "X-Codex-Primary-Window-Minutes": {"300"}, "X-Codex-Primary-Reset-At": {unix},
				"X-Codex-Secondary-Used-Percent": {"100"}, "X-Codex-Secondary-Window-Minutes": {"10080"}, "X-Codex-Secondary-Reset-At": {later},
				"X-Codex-Rate-Limit-Reached-Type": {"rate_limit_reached"},
			},
			resetsAt: resetsAt.Add(24 * time.Hour), retry: 27 * time.Hour, window: 10080, limit: "rate_limit_reached",
		},
		{
			name: "both exhausted take the later reset",
			body: `{"error":{"type":"usage_limit_reached"}}`,
			headers: http.Header{
				"X-Codex-Primary-Used-Percent": {"100"}, "X-Codex-Primary-Window-Minutes": {"300"}, "X-Codex-Primary-Reset-At": {unix},
				"X-Codex-Secondary-Used-Percent": {"100.0"}, "X-Codex-Secondary-Window-Minutes": {"10080"}, "X-Codex-Secondary-Reset-At": {later},
			},
			resetsAt: resetsAt.Add(24 * time.Hour), retry: 27 * time.Hour, window: 10080,
		},
		{
			name: "the active limit selects its header family",
			body: `{"error":{"type":"usage_limit_reached"}}`,
			headers: http.Header{
				"X-Codex-Active-Limit":         {"codex_other"},
				"X-Codex-Primary-Used-Percent": {"100"}, "X-Codex-Primary-Reset-At": {later},
				"X-Codex-Other-Primary-Used-Percent": {"100"}, "X-Codex-Other-Primary-Window-Minutes": {"60"}, "X-Codex-Other-Primary-Reset-At": {unix},
			},
			resetsAt: resetsAt, retry: 3 * time.Hour, window: 60,
		},
		{name: "retry-after only", body: `{"error":{"type":"usage_limit_reached"}}`, headers: http.Header{"Retry-After": {"120"}}, retry: 120 * time.Second},
		// Beyond 7 days the reset stays unknown instead of a clamped 7 days.
		{name: "retry-after beyond 7 days", body: `{"error":{"type":"usage_limit_reached"}}`, headers: http.Header{"Retry-After": {"2592000"}}},
		{name: "retry-after date beyond 7 days", body: `{"error":{"type":"usage_limit_reached"}}`, headers: http.Header{"Retry-After": {now.Add(30 * 24 * time.Hour).Format(http.TimeFormat)}}},
		// Nothing reported: unknown, never the old 60s default.
		{name: "nothing", body: `{"error":{"type":"usage_limit_reached"}}`, headers: http.Header{"X-Codex-Rate-Limit-Reached-Type": {"SENTINEL-type"}}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			headers := tc.headers
			if headers == nil {
				headers = http.Header{}
			}
			err := classifyHTTPFailure(http.StatusTooManyRequests, headers, []byte(tc.body), now)
			var limited *RateLimitError
			if !errors.As(err, &limited) || limited.Kind != RateLimitUsage {
				t.Fatalf("error = %#v, want a usage limit", err)
			}
			if !limited.ResetsAt.Equal(tc.resetsAt) || limited.RetryAfter != tc.retry || limited.WindowMinutes != tc.window || limited.LimitReachedType != tc.limit {
				t.Fatalf("usage limit = %+v, want reset %v retry %v window %d type %q", limited, tc.resetsAt, tc.retry, tc.window, tc.limit)
			}
			if output := fmt.Sprintf("%v %+v %#v", err, err, err); strings.Contains(output, "CANARY") || strings.Contains(output, "plus") {
				t.Fatalf("error leaks upstream text: %s", output)
			}
			failure := ClassifyFailure(err, now)
			if failure.Code != FailureUsageLimit || !failure.ResetsAt.Equal(tc.resetsAt) {
				t.Fatalf("failure = %+v", failure)
			}
		})
	}
}

func TestDirectHTTPFailureReasons(t *testing.T) {
	now := time.Now()
	cases := []struct {
		name   string
		status int
		body   string
		want   FailureCode
	}{
		{name: "usage not included", status: 429, body: `{"error":{"type":"usage_not_included","message":"` + fakeCanary + `"}}`, want: FailureUsageLimit},
		{name: "throttle", status: 429, body: `{"error":{"code":"rate_limit_exceeded"}}`, want: FailureRateLimited},
		{name: "bad request", status: 400, body: `{"error":{"code":"invalid_value","message":"` + fakeCanary + `"}}`, want: FailureBadRequest},
		{name: "context in a 400", status: 400, body: `{"error":{"code":"context_length_exceeded","message":"` + fakeCanary + `"}}`, want: FailureContextTooLong},
		{name: "unprocessable", status: 422, body: `{"detail":[{"loc":["body","input"],"msg":"` + fakeCanary + `"}]}`, want: FailureBadRequest},
		{name: "service unavailable", status: 503, body: `{}`, want: FailureUpstreamUnavailable},
		{name: "forbidden", status: 403, body: `{}`, want: FailureBrokerFailed},
	}
	for _, tc := range cases {
		err := classifyHTTPFailure(tc.status, http.Header{}, []byte(tc.body), now)
		if got := ClassifyFailure(err, now).Code; got != tc.want {
			t.Fatalf("%s: %v classifies as %s, want %s", tc.name, err, got, tc.want)
		}
		if strings.Contains(fmt.Sprintf("%v %+v %#v", err, err, err), "CANARY") {
			t.Fatalf("%s: error leaks upstream text", tc.name)
		}
	}
}

// End to end over the direct transport: a usage limit reported only in the
// exhausted secondary window's headers carries that reset, is not retried,
// and its log line names the reason without any upstream text.
func TestDirectUsageLimitFromHeadersEndToEnd(t *testing.T) {
	reset := time.Now().Add(26 * time.Hour).Unix()
	h := newDirectHarness(t, []fakeTurn{{status: http.StatusTooManyRequests, headers: map[string]string{
		"x-codex-primary-used-percent": "12", "x-codex-primary-window-minutes": "300", "x-codex-primary-reset-at": strconv.FormatInt(reset-80000, 10),
		"x-codex-secondary-used-percent": "100", "x-codex-secondary-window-minutes": "10080", "x-codex-secondary-reset-at": strconv.FormatInt(reset, 10),
		"Content-Type": "application/json",
	}, body: `{"error":{"type":"usage_limit_reached","message":"` + fakeCanary + `","plan_type":"pro"}}`}})
	freshAuth(t, h.home, h.now)
	_, err := h.transport.InvokeStream(t.Context(), h.home, testRequest(), func(string) error { return nil })
	var limited *RateLimitError
	if !errors.As(err, &limited) || limited.Kind != RateLimitUsage || limited.ResetsAt.Unix() != reset || limited.WindowMinutes != 10080 {
		t.Fatalf("error = %#v, want the secondary window's reset", err)
	}
	if h.upstream.count() != 1 {
		t.Fatalf("attempts = %d, a usage limit must not be retried", h.upstream.count())
	}
	logs := h.logs.String()
	for _, want := range []string{`"reason":"subscription_usage_limit"`, `"error_kind":"usage_limit"`, `"error_code":"usage_limit_reached"`, `"window_mins":10080`, `"resets_at":"` + time.Unix(reset, 0).UTC().Format(time.RFC3339) + `"`} {
		if !strings.Contains(logs, want) {
			t.Fatalf("log lacks %s: %s", want, logs)
		}
	}
	if strings.Contains(logs, "CANARY") || strings.Contains(logs, "plan") {
		t.Fatalf("log leaks upstream text: %s", logs)
	}
}
