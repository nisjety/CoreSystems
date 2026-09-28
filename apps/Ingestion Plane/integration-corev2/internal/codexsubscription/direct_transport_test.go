package codexsubscription

import (
	"context"
	"errors"
	"net/http"
	"net/url"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestInvokeAggregatesStream(t *testing.T) {
	h := newDirectHarness(t, []fakeTurn{{frames: successFrames("gpt-5.6-terra", "Hel", "lo")}})
	freshAuth(t, h.home, h.now)
	response, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	if h.upstream.lastBody(t)["stream"] != true {
		t.Fatal("a unary call must still stream")
	}
	if response.Content != "Hello" || response.ModelUsed != "gpt-5.6-terra" || response.RequestID != "req-test" {
		t.Fatalf("response = %+v", response)
	}
	want := &TokenUsage{InputTokens: 1200, CachedInputTokens: 1024, OutputTokens: 42, ReasoningOutputTokens: 30}
	if !reflect.DeepEqual(response.Usage, want) {
		t.Fatalf("usage = %+v", response.Usage)
	}
	if len(response.PhasesMs) == 0 {
		t.Fatal("phases missing")
	}
}

func TestDirectPhases(t *testing.T) {
	clock := newScriptedClock(0, 10*time.Millisecond, 30*time.Millisecond, 50*time.Millisecond)
	h := newDirectHarness(t, []fakeTurn{{frames: successFrames("gpt-5.6-terra", "Hel", "lo")}}, func(o *harnessOptions) { o.direct.Now = clock.now })
	freshAuth(t, h.home, h.now)
	response, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]int64{PhaseTurnStart: 10, PhaseFirstDelta: 20, PhaseTurnTotal: 40}
	if !reflect.DeepEqual(response.PhasesMs, want) {
		t.Fatalf("phases = %v, want %v", response.PhasesMs, want)
	}

	// turn_start covers a 503 retry and a 401 recovery.
	clock = newScriptedClock(0, 10*time.Millisecond, 20*time.Millisecond, 30*time.Millisecond, 35*time.Millisecond, 40*time.Millisecond)
	h = newDirectHarness(t, []fakeTurn{
		{status: http.StatusServiceUnavailable, body: `{"error":{"code":"server_is_overloaded"}}`},
		{status: http.StatusUnauthorized, body: `{}`},
		{frames: successFrames("gpt-5.6-terra", "ok")},
	}, func(o *harnessOptions) {
		o.direct.Now = clock.now
		o.respond = func(int, url.Values) (int, string) {
			return http.StatusOK, grantBody(fakeJWT(anyMap{"exp": time.Now().Add(time.Hour).Unix()}, "rotated"), "", "")
		}
	})
	freshAuth(t, h.home, h.now)
	response, err = h.transport.Invoke(t.Context(), h.home, testRequest())
	if err != nil {
		t.Fatal(err)
	}
	want = map[string]int64{PhaseTurnStart: 30, PhaseFirstDelta: 5, PhaseTurnTotal: 10}
	if !reflect.DeepEqual(response.PhasesMs, want) {
		t.Fatalf("phases with retry and recovery = %v, want %v", response.PhasesMs, want)
	}
	for _, phase := range []string{PhaseSpawn, PhaseInitialize, PhaseThreadStart} {
		if _, present := response.PhasesMs[phase]; present {
			t.Fatalf("direct transport reported app-server phase %s", phase)
		}
	}
	if h.upstream.count() != 3 || h.auth.callCount() != 1 {
		t.Fatalf("upstream=%d token calls=%d, want 3 and 1", h.upstream.count(), h.auth.callCount())
	}
}

func TestStreamDisconnectCancelsUpstream(t *testing.T) {
	h := newDirectHarness(t, []fakeTurn{{frames: successFrames("gpt-5.6-terra", "a", "b", "c"), waitForCancel: true}})
	freshAuth(t, h.home, h.now)
	gone := errors.New("stream client gone")
	started := time.Now()
	_, err := h.transport.InvokeStream(t.Context(), h.home, testRequest(), func(string) error { return gone })
	if !errors.Is(err, gone) {
		t.Fatalf("error = %v, want the sink error", err)
	}
	if time.Since(started) > time.Second {
		t.Fatal("InvokeStream did not return promptly")
	}
	select {
	case <-h.upstream.canceled:
	case <-time.After(time.Second):
		t.Fatal("the upstream request was not cancelled")
	}
	if h.upstream.count() != 1 {
		t.Fatalf("attempts = %d, want 1 (no retry)", h.upstream.count())
	}
}

func TestContextCancelBeforeHeaders(t *testing.T) {
	// The caller goes away once the request has arrived and before any
	// header is sent, never before the request itself is sent.
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	h := newDirectHarness(t, []fakeTurn{{before: func(r *http.Request) {
		cancel()
		select {
		case <-r.Context().Done():
		case <-time.After(5 * time.Second):
		}
	}}})
	freshAuth(t, h.home, h.now)
	_, err := h.transport.InvokeStream(ctx, h.home, testRequest(), func(string) error { return nil })
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want context.Canceled", err)
	}
	if h.upstream.count() != 1 || len(h.recordedSleeps()) != 0 {
		t.Fatalf("a cancelled call was retried: attempts=%d", h.upstream.count())
	}
}

func TestRetryTransportErrorBeforeHeaders(t *testing.T) {
	script := []fakeTurn{{closeBeforeHeaders: true}, {closeBeforeHeaders: true}, {frames: successFrames("gpt-5.6-terra", "ok")}}
	h := newDirectHarness(t, script)
	freshAuth(t, h.home, h.now)
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	if h.upstream.count() != 3 {
		t.Fatalf("attempts = %d, want 3", h.upstream.count())
	}
	if got := h.recordedSleeps(); !reflect.DeepEqual(got, []time.Duration{250 * time.Millisecond, 750 * time.Millisecond}) {
		t.Fatalf("backoff = %v", got)
	}

	h = newDirectHarness(t, script, func(o *harnessOptions) { o.direct.MaxRetries = 0 })
	freshAuth(t, h.home, h.now)
	_, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindTransport {
		t.Fatalf("error = %v, want a transport error", err)
	}
	if h.upstream.count() != 1 {
		t.Fatalf("attempts with MaxRetries=0 = %d, want 1", h.upstream.count())
	}
}

func TestRetry503WithRetryAfter(t *testing.T) {
	h := newDirectHarness(t, []fakeTurn{
		{status: http.StatusServiceUnavailable, headers: map[string]string{"Retry-After": "1"}, body: `{}`},
		{frames: successFrames("gpt-5.6-terra", "ok")},
	})
	freshAuth(t, h.home, h.now)
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err != nil {
		t.Fatal(err)
	}
	if got := h.recordedSleeps(); !reflect.DeepEqual(got, []time.Duration{time.Second}) {
		t.Fatalf("sleeps = %v, want the 1s Retry-After", got)
	}

	h = newDirectHarness(t, []fakeTurn{{status: http.StatusServiceUnavailable, headers: map[string]string{"Retry-After": "30"}, body: `{}`}})
	freshAuth(t, h.home, h.now)
	_, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.HTTPStatus != http.StatusServiceUnavailable {
		t.Fatalf("error = %v", err)
	}
	if h.upstream.count() != 1 {
		t.Fatalf("a 30s Retry-After was retried: attempts=%d", h.upstream.count())
	}
}

func TestNoRetryCases(t *testing.T) {
	for _, status := range []int{http.StatusBadRequest, http.StatusForbidden, http.StatusNotFound, http.StatusTooManyRequests, http.StatusConflict, http.StatusRequestEntityTooLarge, http.StatusUnprocessableEntity} {
		h := newDirectHarness(t, []fakeTurn{{status: status, body: `{"error":{"type":"invalid_request_error","message":"SENTINEL-upstream-message"}}`}})
		freshAuth(t, h.home, h.now)
		_, err := h.transport.Invoke(t.Context(), h.home, testRequest())
		if err == nil || h.upstream.count() != 1 {
			t.Fatalf("status %d: err=%v attempts=%d, want one failed attempt", status, err, h.upstream.count())
		}
		if strings.Contains(err.Error(), "SENTINEL") {
			t.Fatalf("status %d: error leaks upstream text: %v", status, err)
		}
	}

	// A connection killed after the first delta is never retried.
	h := newDirectHarness(t, []fakeTurn{{frames: successFrames("gpt-5.6-terra", "partial", "rest"), killAfterFrames: 2}})
	freshAuth(t, h.home, h.now)
	var forwarded []string
	_, err := h.transport.InvokeStream(t.Context(), h.home, testRequest(), func(delta string) error { forwarded = append(forwarded, delta); return nil })
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindTruncated || h.upstream.count() != 1 {
		t.Fatalf("killed stream: err=%v attempts=%d", err, h.upstream.count())
	}
	if !reflect.DeepEqual(forwarded, []string{"partial"}) {
		t.Fatalf("forwarded = %q", forwarded)
	}

	// Less than 5s to the deadline: no retry.
	h = newDirectHarness(t, []fakeTurn{{status: http.StatusServiceUnavailable, body: `{}`}, {frames: successFrames("gpt-5.6-terra", "ok")}})
	freshAuth(t, h.home, h.now)
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
	defer cancel()
	if _, err := h.transport.Invoke(ctx, h.home, testRequest()); err == nil || h.upstream.count() != 1 {
		t.Fatalf("near-deadline: err=%v attempts=%d, want no retry", err, h.upstream.count())
	}
}

func TestUsageLimitFromUpstreamBodyIsRateLimit(t *testing.T) {
	resetsAt := time.Now().Add(120 * time.Second).Unix()
	h := newDirectHarness(t, []fakeTurn{{status: http.StatusTooManyRequests, body: `{"error":{"type":"usage_limit_reached","message":"SENTINEL-upstream-message","plan_type":"plus","resets_at":` + strconv.FormatInt(resetsAt, 10) + `}}`}})
	freshAuth(t, h.home, h.now)
	_, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	var limited *RateLimitError
	if !errors.As(err, &limited) || limited.Kind != RateLimitUsage {
		t.Fatalf("error = %v, want a usage-limit rate limit", err)
	}
	if limited.RetryAfter < 118*time.Second || limited.RetryAfter > 121*time.Second {
		t.Fatalf("retry after = %v, want about 120s", limited.RetryAfter)
	}
	if h.upstream.count() != 1 {
		t.Fatalf("a usage limit was retried: attempts=%d", h.upstream.count())
	}
}
