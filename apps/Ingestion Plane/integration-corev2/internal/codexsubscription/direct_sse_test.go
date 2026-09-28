package codexsubscription

import (
	"errors"
	"net/http"
	"reflect"
	"strings"
	"testing"
	"time"
)

func consumeFrames(t *testing.T, request InvokeRequest, onDelta func(string) error, body string) (streamResult, *streamState, error) {
	t.Helper()
	state := &streamState{onDelta: onDelta}
	result, err := consumeStream(t.Context(), strings.NewReader(body), request, http.Header{}, state, time.Now)
	return result, state, err
}

func TestSSEDeltasUsageCompletion(t *testing.T) {
	// "é" and "世" are split across frames byte-wise in the source string,
	// so each delta is forwarded exactly as the upstream sent it.
	deltas := []string{"Hé", "llo ", "世界", "!"}
	usage := anyMap{
		"input_tokens":          1200,
		"input_tokens_details":  anyMap{"cached_tokens": 1024, "cache_write_tokens": 7},
		"output_tokens":         42,
		"output_tokens_details": anyMap{"reasoning_tokens": 30},
	}
	body := createdFrame("gpt-5.6-terra")
	for _, delta := range deltas {
		body += deltaFrame(delta)
	}
	body += sse(anyMap{"type": "response.reasoning_summary_text.delta", "delta": "SENTINEL-reasoning"})
	body += completedFrame("gpt-5.6-terra", usage)
	var forwarded []string
	result, _, err := consumeFrames(t, testRequest(), func(delta string) error {
		forwarded = append(forwarded, delta)
		return nil
	}, body)
	if err != nil {
		t.Fatalf("consumeStream: %v", err)
	}
	if !reflect.DeepEqual(forwarded, deltas) {
		t.Fatalf("forwarded %q, want %q", forwarded, deltas)
	}
	if result.content != strings.Join(deltas, "") {
		t.Fatalf("content = %q", result.content)
	}
	want := &TokenUsage{InputTokens: 1200, CachedInputTokens: 1024, CacheWriteInputTokens: 7, OutputTokens: 42, ReasoningOutputTokens: 30}
	if !reflect.DeepEqual(result.usage, want) {
		t.Fatalf("usage = %+v, want %+v", result.usage, want)
	}
	if strings.Contains(result.content, "SENTINEL") {
		t.Fatal("reasoning text leaked into the answer")
	}
}

func TestSSEUsageAbsentOrMalformedIsNil(t *testing.T) {
	for name, usage := range map[string]any{
		"absent":         nil,
		"string":         "lots",
		"missing output": anyMap{"input_tokens": 5},
		"bad details":    anyMap{"input_tokens": 5, "output_tokens": 1, "input_tokens_details": "x"},
	} {
		body := deltaFrame("ok") + completedFrame("gpt-5.6-terra", usage)
		result, _, err := consumeFrames(t, testRequest(), nil, body)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if result.usage != nil {
			t.Fatalf("%s: usage = %+v, want nil", name, result.usage)
		}
	}
}

func TestSSEFraming(t *testing.T) {
	body := ": keepalive comment\r\n" +
		"event: response.completed\r\n" +
		"data: {\"type\":\"response.output_text.delta\",\r\n" +
		"data: \"delta\":\"a\"}\r\n\r\n" +
		"event: noise\n" +
		"data: {\"type\":\"response.in_progress\"}\n\n" +
		"data:{\"type\":\"response.output_text.delta\",\"delta\":\"b\"}\n\n" +
		"data: {not json\n\n" +
		"data: {\"type\":\"response.unknown_event\",\"delta\":{\"nested\":1}}\n\n" +
		"data: [DONE]\n\n" +
		completedFrame("gpt-5.6-terra", nil)
	result, _, err := consumeFrames(t, testRequest(), nil, body)
	if err != nil {
		t.Fatalf("consumeStream: %v", err)
	}
	if result.content != "ab" {
		t.Fatalf("content = %q, want ab", result.content)
	}

	// An 8 MiB terminal event parses.
	big := strings.Repeat("x", 8<<20)
	result, _, err = consumeFrames(t, testRequest(), nil, sse(anyMap{"type": "response.completed", "response": anyMap{"model": "gpt-5.6-terra", "output": []any{anyMap{"type": "message", "content": []any{anyMap{"type": "output_text", "text": big}}}}}}))
	if err != nil || len(result.content) != len(big) {
		t.Fatalf("8 MiB event: len=%d err=%v", len(result.content), err)
	}

	// A 17 MiB event is refused.
	_, _, err = consumeFrames(t, testRequest(), nil, "data: {\"type\":\"response.output_text.delta\",\"delta\":\""+strings.Repeat("y", 17<<20)+"\"}\n\n")
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindTooLarge {
		t.Fatalf("17 MiB event error = %v, want stream_event_too_large", err)
	}
}

func TestSSEFallbackText(t *testing.T) {
	item := sse(anyMap{"type": "response.output_item.done", "item": anyMap{"type": "message", "content": []any{anyMap{"type": "output_text", "text": "from item"}}}})
	var forwarded []string
	collect := func(delta string) error { forwarded = append(forwarded, delta); return nil }
	result, _, err := consumeFrames(t, testRequest(), collect, item+completedFrame("gpt-5.6-terra", nil))
	if err != nil || result.content != "from item" || !reflect.DeepEqual(forwarded, []string{"from item"}) {
		t.Fatalf("item fallback: %q %q %v", result.content, forwarded, err)
	}
	if result.firstText.IsZero() {
		t.Fatal("fallback text was not timed")
	}

	forwarded = nil
	completed := sse(anyMap{"type": "response.completed", "response": anyMap{"model": "gpt-5.6-terra", "output": []any{
		anyMap{"type": "reasoning", "content": []any{anyMap{"type": "output_text", "text": "SENTINEL-reasoning"}}},
		anyMap{"type": "message", "content": []any{anyMap{"type": "output_text", "text": "one "}}},
		anyMap{"type": "message", "content": []any{anyMap{"type": "output_text", "text": "two"}}},
	}}})
	result, _, err = consumeFrames(t, testRequest(), collect, completed)
	if err != nil || result.content != "one two" || !reflect.DeepEqual(forwarded, []string{"one two"}) {
		t.Fatalf("completed.output fallback: %q %q %v", result.content, forwarded, err)
	}

	_, _, err = consumeFrames(t, testRequest(), collect, deltaFrame("")+completedFrame("gpt-5.6-terra", nil))
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindEmpty {
		t.Fatalf("empty answer error = %v", err)
	}
	_, _, err = consumeFrames(t, testRequest(), nil, deltaFrame("   \n")+completedFrame("gpt-5.6-terra", nil))
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindEmpty {
		t.Fatalf("whitespace answer error = %v", err)
	}
}

func TestSSETerminalFailures(t *testing.T) {
	const message = "SENTINEL-upstream-message please try again in 2s"
	cases := []struct {
		name      string
		frame     string
		wantKind  string
		wantCode  string
		rateLimit RateLimitKind
		retryable bool
	}{
		{name: "failed usage limit", frame: sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "usage_limit_reached", "message": message, "resets_in_seconds": 90}}}), rateLimit: RateLimitUsage},
		{name: "failed throttle", frame: sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "rate_limit_exceeded", "message": message}}}), rateLimit: RateLimitThrottle},
		{name: "error event throttle by status", frame: sse(anyMap{"type": "error", "status_code": 429, "message": message}), rateLimit: RateLimitThrottle},
		{name: "not included", frame: sse(anyMap{"type": "error", "code": "usage_not_included", "message": message}), wantKind: upstreamKindNotIncluded, wantCode: "usage_not_included"},
		{name: "quota", frame: sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "insufficient_quota", "message": message}}}), wantKind: upstreamKindQuota, wantCode: "insufficient_quota"},
		{name: "credit", frame: sse(anyMap{"type": "error", "error": anyMap{"type": "credit_balance_exhausted", "message": message}}), wantKind: upstreamKindQuota, wantCode: "credit_balance_exhausted"},
		{name: "context", frame: sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "context_length_exceeded", "message": message}}}), wantKind: upstreamKindContext, wantCode: "context_length_exceeded"},
		{name: "policy", frame: sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "cyber_policy", "message": message}}}), wantKind: upstreamKindPolicy, wantCode: "cyber_policy"},
		{name: "invalid prompt", frame: sse(anyMap{"type": "error", "code": "invalid_prompt", "message": message}), wantKind: upstreamKindPolicy, wantCode: "invalid_prompt"},
		{name: "server error", frame: sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "server_error", "message": message}}}), wantKind: upstreamKindFailed, wantCode: "server_error", retryable: true},
		{name: "unknown code", frame: sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "SENTINEL-code", "message": message}}}), wantKind: upstreamKindFailed, wantCode: "other"},
		{name: "incomplete", frame: sse(anyMap{"type": "response.incomplete", "response": anyMap{"incomplete_details": anyMap{"reason": "max_output_tokens"}}}), wantKind: upstreamKindIncomplete, wantCode: "max_output_tokens"},
		{name: "eof", frame: createdFrame("gpt-5.6-terra"), wantKind: upstreamKindTruncated, retryable: true},
		{name: "only malformed", frame: "data: {nope\n\n", wantKind: upstreamKindMalformed},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, _, err := consumeFrames(t, testRequest(), nil, tc.frame)
			if err == nil {
				t.Fatal("want an error")
			}
			if strings.Contains(err.Error(), "SENTINEL") {
				t.Fatalf("error text leaks upstream content: %v", err)
			}
			if tc.rateLimit != "" {
				var limited *RateLimitError
				if !errors.As(err, &limited) || limited.Kind != tc.rateLimit || !errors.Is(err, ErrUpstreamRateLimited) {
					t.Fatalf("error = %v, want rate limit %s", err, tc.rateLimit)
				}
				return
			}
			var upstream *UpstreamError
			if !errors.As(err, &upstream) || upstream.Kind != tc.wantKind || upstream.Code != tc.wantCode || upstream.Retryable != tc.retryable {
				t.Fatalf("error = %#v, want kind %s code %q retryable %v", err, tc.wantKind, tc.wantCode, tc.retryable)
			}
		})
	}
}

func TestSSERetryAfterParsing(t *testing.T) {
	now := time.Unix(1_800_000_000, 0)
	usage := []struct {
		name         string
		failure      upstreamFailure
		headers      http.Header
		wantResetsAt time.Time
		want         time.Duration
		wantOK       bool
	}{
		{name: "resets_at", failure: upstreamFailure{ResetsAt: ptrFloat(1_800_003_725)}, wantResetsAt: time.Unix(1_800_003_725, 0), want: 3725 * time.Second, wantOK: true},
		{name: "resets_in", failure: upstreamFailure{ResetsInSeconds: ptrFloat(90)}, wantResetsAt: now.Add(90 * time.Second), want: 90 * time.Second, wantOK: true},
		{name: "exhausted primary header", headers: http.Header{"X-Codex-Primary-Used-Percent": {"100"}, "X-Codex-Primary-Reset-At": {"1800000120"}}, wantResetsAt: time.Unix(1_800_000_120, 0), want: 120 * time.Second, wantOK: true},
		{name: "unexhausted header is not a reset", headers: http.Header{"X-Codex-Primary-Used-Percent": {"42"}, "X-Codex-Primary-Reset-At": {"1800000120"}}},
		{name: "retry-after sets only the duration", headers: http.Header{"Retry-After": {"30"}}, want: 30 * time.Second, wantOK: true},
		{name: "retry-after of exactly 7 days", headers: http.Header{"Retry-After": {"604800"}}, want: 7 * 24 * time.Hour, wantOK: true},
		{name: "retry-after HTTP-date", headers: http.Header{"Retry-After": {now.Add(2 * time.Hour).UTC().Format(http.TimeFormat)}}, want: 2 * time.Hour, wantOK: true},
		// No reset reported: unknown, never a made-up 60s.
		{name: "usage unknown"},
		{name: "resets_in beyond 7 days is not a reset", failure: upstreamFailure{ResetsInSeconds: ptrFloat(30 * 24 * 3600)}},
		{name: "resets_at in the past is not a reset", failure: upstreamFailure{ResetsAt: ptrFloat(1_799_999_000)}},
		// A Retry-After beyond 7 days is dropped like the other sources, not
		// clamped to a 7-day reset that would come before the real one.
		{name: "retry-after beyond 7 days is not a reset", headers: http.Header{"Retry-After": {"2592000"}}},
		{name: "retry-after just beyond 7 days is not a reset", headers: http.Header{"Retry-After": {"604801"}}},
		{name: "retry-after HTTP-date beyond 7 days is not a reset", headers: http.Header{"Retry-After": {now.Add(30 * 24 * time.Hour).UTC().Format(http.TimeFormat)}}},
		{name: "retry-after HTTP-date in the past is not a reset", headers: http.Header{"Retry-After": {now.Add(-time.Hour).UTC().Format(http.TimeFormat)}}},
		{name: "retry-after of zero is not a reset", headers: http.Header{"Retry-After": {"0"}}},
		{name: "retry-after NaN is not a reset", headers: http.Header{"Retry-After": {"NaN"}}},
		{name: "retry-after infinity is not a reset", headers: http.Header{"Retry-After": {"+Inf"}}},
	}
	for _, tc := range usage {
		headers := tc.headers
		if headers == nil {
			headers = http.Header{}
		}
		resetsAt, retryAfter, ok := usageReset(tc.failure, headers, now)
		if !resetsAt.Equal(tc.wantResetsAt) || retryAfter != tc.want || ok != tc.wantOK {
			t.Fatalf("%s: usage reset = %v, %v, %v; want %v, %v, %v", tc.name, resetsAt, retryAfter, ok, tc.wantResetsAt, tc.want, tc.wantOK)
		}
	}
	throttle := []struct {
		name    string
		failure upstreamFailure
		headers http.Header
		want    time.Duration
	}{
		{name: "retry-after-ms", headers: http.Header{"Retry-After-Ms": {"2500"}}, want: 2500 * time.Millisecond},
		{name: "message seconds", failure: upstreamFailure{Message: "Rate limit reached. Please try again in 3.5s."}, want: 3500 * time.Millisecond},
		{name: "message ms", failure: upstreamFailure{Message: "try again in 20ms"}, want: time.Second},
		{name: "throttle default", want: time.Second},
	}
	for _, tc := range throttle {
		headers := tc.headers
		if headers == nil {
			headers = http.Header{}
		}
		if got := throttleRetryAfter(tc.failure, headers); got != tc.want {
			t.Fatalf("%s: retry after = %v, want %v", tc.name, got, tc.want)
		}
	}
	if CeilSeconds(3500*time.Millisecond) != 4 || CeilSeconds(0) != 1 || CeilSeconds(3725*time.Second) != 3725 {
		t.Fatal("CeilSeconds must round up to whole seconds, at least 1")
	}
}

func ptrFloat(value float64) *float64 { return &value }

func TestServedModelCheck(t *testing.T) {
	// A mismatching openai-model header fails before any delta.
	h := newDirectHarness(t, []fakeTurn{{headers: map[string]string{"openai-model": "gpt-4o-mini"}, frames: successFrames("gpt-5.6-terra", "never")}})
	freshAuth(t, h.home, h.now)
	forwarded := 0
	_, err := h.transport.InvokeStream(t.Context(), h.home, testRequest(), func(string) error { forwarded++; return nil })
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindMismatch || forwarded != 0 {
		t.Fatalf("header mismatch: err=%v forwarded=%d", err, forwarded)
	}

	// A dated snapshot is accepted and reported as the requested model.
	h = newDirectHarness(t, []fakeTurn{{frames: successFrames("gpt-5.6-terra-2026-08-15", "ok")}})
	freshAuth(t, h.home, h.now)
	response, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	if err != nil || response.ModelUsed != "gpt-5.6-terra" {
		t.Fatalf("snapshot: %+v %v", response, err)
	}

	// Another model is rejected.
	h = newDirectHarness(t, []fakeTurn{{frames: []string{deltaFrame("ok"), completedFrame("gpt-5.6-sol", nil)}}})
	freshAuth(t, h.home, h.now)
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); !errors.As(err, &upstream) || upstream.Kind != upstreamKindMismatch {
		t.Fatalf("other model: %v", err)
	}

	// An absent served model is accepted.
	h = newDirectHarness(t, []fakeTurn{{frames: []string{deltaFrame("ok"), sse(anyMap{"type": "response.completed", "response": anyMap{}})}}})
	freshAuth(t, h.home, h.now)
	if response, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err != nil || response.ModelUsed != "gpt-5.6-terra" {
		t.Fatalf("absent model: %+v %v", response, err)
	}

	for _, tc := range []struct {
		served string
		ok     bool
	}{{"", true}, {"gpt-5.6-terra", true}, {"gpt-5.6-terra-2026-08-15", true}, {"gpt-5.6-terra-mini", false}, {"gpt-5.6-terra-2026-8-15", false}, {"xgpt-5.6-terra", false}} {
		if servedModelMatches("gpt-5.6-terra", tc.served) != tc.ok {
			t.Fatalf("servedModelMatches(%q) != %v", tc.served, tc.ok)
		}
	}
}
