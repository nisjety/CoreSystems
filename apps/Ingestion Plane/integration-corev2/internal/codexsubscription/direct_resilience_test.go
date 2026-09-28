package codexsubscription

import (
	"bytes"
	"encoding/json"
	"errors"
	"net/http"
	"reflect"
	"strings"
	"testing"
	"time"
)

// tierRejectingHandler answers a priority request for model with reject and
// anything else with a successful stream.
func tierRejectingHandler(h *directHarness, model string, reject func(w http.ResponseWriter)) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		h.upstream.mu.Lock()
		h.upstream.requests = append(h.upstream.requests, recordedRequest{header: r.Header.Clone(), body: mustJSON(body)})
		h.upstream.mu.Unlock()
		if body["service_tier"] == "priority" && body["model"] == model {
			reject(w)
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		for _, frame := range successFrames(body["model"].(string), "ok") {
			_, _ = w.Write([]byte(frame))
		}
	}
}

func TestServiceTierMemoryIsPerConnection(t *testing.T) {
	memory := newTierMemory()
	memory.reject("conn-a", "gpt-5.6-terra")
	if !memory.rejected("conn-a", "gpt-5.6-terra") {
		t.Fatal("the rejection was not remembered")
	}
	if memory.rejected("conn-b", "gpt-5.6-terra") || memory.rejected("conn-a", "gpt-5.6-sol") {
		t.Fatal("one connection's rejection leaked to another connection or model")
	}

	h := newDirectHarness(t, nil)
	h.upstream.setHandler(tierRejectingHandler(h, "gpt-5.6-terra", func(w http.ResponseWriter) {
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"error":{"type":"invalid_request_error","param":"service_tier"}}`))
	}))
	freshAuth(t, h.home, h.now)
	request := testRequest()
	request.ServiceTier = "priority"
	if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
		t.Fatal(err)
	}
	other := request
	other.ConnectionID = "conn-other"
	if _, err := h.transport.Invoke(t.Context(), h.home, other); err != nil {
		t.Fatal(err)
	}
	requests := h.upstream.recorded()
	if len(requests) != 4 || !bytes.Contains(requests[2].body, []byte(`"service_tier":"priority"`)) {
		t.Fatalf("another connection lost priority after one connection's rejection (%d requests)", len(requests))
	}
}

func TestDirectServiceTierDegradeShapes(t *testing.T) {
	cases := map[string]func(w http.ResponseWriter){
		"detail string": func(w http.ResponseWriter) {
			w.WriteHeader(http.StatusBadRequest)
			_, _ = w.Write([]byte(`{"detail":"Unsupported parameter: service_tier"}`))
		},
		"detail validation list": func(w http.ResponseWriter) {
			w.WriteHeader(http.StatusBadRequest)
			_, _ = w.Write([]byte(`{"detail":[{"loc":["body","service_tier"],"msg":"SENTINEL-upstream-message","type":"literal_error"}]}`))
		},
		"message": func(w http.ResponseWriter) {
			w.WriteHeader(http.StatusBadRequest)
			_, _ = w.Write([]byte(`{"error":{"type":"invalid_request_error","param":null,"message":"Unsupported value: 'priority' is not supported with service_tier for this plan."}}`))
		},
		"sse error with top-level param": func(w http.ResponseWriter) {
			w.Header().Set("Content-Type", "text/event-stream")
			_, _ = w.Write([]byte(sse(anyMap{"type": "error", "code": "invalid_value", "param": "service_tier", "message": "SENTINEL-upstream-message"})))
		},
	}
	for name, reject := range cases {
		t.Run(name, func(t *testing.T) {
			h := newDirectHarness(t, nil)
			h.upstream.setHandler(tierRejectingHandler(h, "gpt-5.6-terra", reject))
			freshAuth(t, h.home, h.now)
			request := testRequest()
			request.ServiceTier = "priority"
			if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
				t.Fatalf("degraded call: %v", err)
			}
			requests := h.upstream.recorded()
			if len(requests) != 2 || !bytes.Contains(requests[0].body, []byte(`"service_tier":"priority"`)) || bytes.Contains(requests[1].body, []byte("service_tier")) {
				t.Fatalf("want one priority attempt then one without: %d requests", len(requests))
			}
			if !h.transport.tiers.rejected(request.ConnectionID, request.Model) {
				t.Fatal("the rejection was not remembered")
			}
			if strings.Contains(h.logs.String(), "SENTINEL") {
				t.Fatal("upstream text reached the log")
			}
		})
	}
}

func TestUpstreamErrorDetailCodes(t *testing.T) {
	cases := []struct {
		body, code string
	}{
		{`{"detail":"Unsupported parameter: max_output_tokens"}`, "unsupported_parameter"},
		{`{"detail":"Store must be set to false"}`, "invalid_store"},
		{`{"detail":"Stream must be set to true"}`, "invalid_stream"},
		{`{"detail":"Instructions are not valid"}`, "invalid_instructions"},
		{`{"detail":"Missing required parameter: 'input'."}`, "missing_required_parameter"},
		{`{"detail":"SENTINEL-upstream-message"}`, "other"},
		{`{"detail":[{"loc":["body","reasoning"],"msg":"SENTINEL-upstream-message","type":"extra_forbidden"}]}`, "other"},
		{`{"error":{"code":"invalid_value"},"detail":"Unsupported parameter: x"}`, "invalid_value"},
		{`{}`, ""},
	}
	for _, tc := range cases {
		err := classifyHTTPFailure(http.StatusBadRequest, http.Header{}, []byte(tc.body), time.Now())
		var upstream *UpstreamError
		if !errors.As(err, &upstream) || upstream.Kind != upstreamKindHTTPStatus || upstream.Code != tc.code {
			t.Fatalf("%s: error = %#v, want code %q", tc.body, err, tc.code)
		}
		if upstream.tierRejected {
			t.Fatalf("%s: marked as a service tier rejection", tc.body)
		}
		for _, leaked := range []string{"SENTINEL", "max_output_tokens", "Store must"} {
			if strings.Contains(err.Error(), leaked) {
				t.Fatalf("%s: error leaks upstream text: %v", tc.body, err)
			}
		}
	}
}

func TestServedModelFromEventHeaders(t *testing.T) {
	const restricted = "gpt-5.6-cyber-restricted"
	cases := []struct {
		name      string
		turn      fakeTurn
		forwarded []string
	}{
		{name: "response headers on created (Codex fixture)", turn: fakeTurn{frames: []string{
			sse(anyMap{"type": "response.created", "response": anyMap{"id": "resp-1", "headers": anyMap{"OpenAI-Model": restricted}}}),
			deltaFrame("never"), sse(anyMap{"type": "response.completed", "response": anyMap{"id": "resp-1"}}),
		}}},
		{name: "top-level headers on a metadata event", turn: fakeTurn{frames: []string{
			sse(anyMap{"type": "response.metadata", "headers": anyMap{"x-openai-model": restricted}}),
			deltaFrame("never"), completedFrame("gpt-5.6-terra", nil),
		}}},
		{name: "reroute after the first delta", turn: fakeTurn{frames: []string{
			createdFrame("gpt-5.6-terra"), deltaFrame("a"),
			sse(anyMap{"type": "response.in_progress", "response": anyMap{"headers": anyMap{"openai-model": restricted}}}),
			deltaFrame("b"), completedFrame("gpt-5.6-terra", nil),
		}}, forwarded: []string{"a"}},
		{name: "x-openai-model response header", turn: fakeTurn{headers: map[string]string{"x-openai-model": restricted}, frames: successFrames("gpt-5.6-terra", "never")}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h := newDirectHarness(t, []fakeTurn{tc.turn})
			freshAuth(t, h.home, h.now)
			var forwarded []string
			_, err := h.transport.InvokeStream(t.Context(), h.home, testRequest(), func(delta string) error {
				forwarded = append(forwarded, delta)
				return nil
			})
			var upstream *UpstreamError
			if !errors.As(err, &upstream) || upstream.Kind != upstreamKindMismatch {
				t.Fatalf("error = %v, want served_model_mismatch", err)
			}
			if !reflect.DeepEqual(forwarded, tc.forwarded) || h.upstream.count() != 1 {
				t.Fatalf("forwarded %q over %d attempts, want %q over 1", forwarded, h.upstream.count(), tc.forwarded)
			}
			if !strings.Contains(h.logs.String(), `"served_model":"`+restricted+`"`) {
				t.Fatalf("the served model was not logged: %s", h.logs.String())
			}
		})
	}

	// A header that names the requested model, or a dated snapshot of it, is
	// accepted, and ModelUsed stays the requested model.
	h := newDirectHarness(t, []fakeTurn{{frames: []string{
		sse(anyMap{"type": "response.created", "response": anyMap{"id": "resp-1", "headers": anyMap{"openai-model": "gpt-5.6-terra-2026-08-15"}}}),
		deltaFrame("ok"), sse(anyMap{"type": "response.completed", "response": anyMap{"id": "resp-1"}}),
	}}})
	freshAuth(t, h.home, h.now)
	response, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	if err != nil || response.ModelUsed != "gpt-5.6-terra" || response.Content != "ok" {
		t.Fatalf("matching header: %+v, %v", response, err)
	}
	if !strings.Contains(h.logs.String(), `"served_model":"gpt-5.6-terra-2026-08-15"`) {
		t.Fatalf("the served snapshot was not logged: %s", h.logs.String())
	}
}

func TestRetryAfterHintsCannotOverflow(t *testing.T) {
	huge := http.Header{"Retry-After-Ms": {"1e20"}}
	hint, ok := headerRetryHint(huge)
	if !ok || hint != maxRetryAfter {
		t.Fatalf("retry-after-ms 1e20 = %v, %v; want the 7-day ceiling", hint, ok)
	}
	if got := throttleRetryAfter(upstreamFailure{}, huge); got != maxRetryAfter {
		t.Fatalf("throttle retry after = %v", got)
	}
	if got := throttleRetryAfter(upstreamFailure{Message: "try again in 99999999999999999999ms"}, http.Header{}); got != maxRetryAfter {
		t.Fatalf("message retry after = %v", got)
	}
	// A far-future usage reset cannot overflow into a negative or bogus
	// delay: it is simply not a reset.
	if resetsAt, retryAfter, ok := usageReset(upstreamFailure{ResetsAt: ptrFloat(1e300)}, http.Header{}, time.Now()); ok || !resetsAt.IsZero() || retryAfter != 0 {
		t.Fatalf("resets_at 1e300 = %v, %v, %v; want unknown", resetsAt, retryAfter, ok)
	}
	exhausted := http.Header{"X-Codex-Primary-Used-Percent": {"100"}, "X-Codex-Primary-Reset-At": {"1e300"}}
	if resetsAt, retryAfter, ok := usageReset(upstreamFailure{}, exhausted, time.Now()); ok || !resetsAt.IsZero() || retryAfter != 0 {
		t.Fatalf("x-codex-primary-reset-at 1e300 = %v, %v, %v; want unknown", resetsAt, retryAfter, ok)
	}

	// A 503 whose hint overflowed before is not retried at once.
	h := newDirectHarness(t, []fakeTurn{
		{status: http.StatusServiceUnavailable, headers: map[string]string{"retry-after-ms": "1e20"}, body: `{}`},
		{frames: successFrames("gpt-5.6-terra", "ok")},
	})
	freshAuth(t, h.home, h.now)
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err == nil {
		t.Fatal("a 503 with a huge retry hint was retried")
	}
	if h.upstream.count() != 1 || len(h.recordedSleeps()) != 0 {
		t.Fatalf("attempts=%d sleeps=%v, want 1 and none", h.upstream.count(), h.recordedSleeps())
	}
}

func TestRetryBeforeFirstDelta(t *testing.T) {
	failed := sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "server_error", "message": "SENTINEL-upstream-message"}}})
	cases := []struct {
		name     string
		first    fakeTurn
		wantKind string
	}{
		{name: "response.failed server_error", first: fakeTurn{frames: []string{createdFrame("gpt-5.6-terra"), failed}}, wantKind: upstreamKindFailed},
		{name: "error event server_is_overloaded", first: fakeTurn{frames: []string{sse(anyMap{"type": "error", "code": "server_is_overloaded"})}}, wantKind: upstreamKindFailed},
		{name: "stream truncated", first: fakeTurn{frames: successFrames("gpt-5.6-terra", "never"), killAfterFrames: 1}, wantKind: upstreamKindTruncated},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h := newDirectHarness(t, []fakeTurn{tc.first, {frames: successFrames("gpt-5.6-terra", "ok")}})
			freshAuth(t, h.home, h.now)
			var forwarded []string
			response, err := h.transport.InvokeStream(t.Context(), h.home, testRequest(), func(delta string) error {
				forwarded = append(forwarded, delta)
				return nil
			})
			if err != nil || response.Content != "ok" {
				t.Fatalf("InvokeStream: %+v, %v", response, err)
			}
			if h.upstream.count() != 2 || !reflect.DeepEqual(forwarded, []string{"ok"}) {
				t.Fatalf("attempts=%d forwarded=%q, want 2 and [ok]", h.upstream.count(), forwarded)
			}

			h = newDirectHarness(t, []fakeTurn{tc.first, {frames: successFrames("gpt-5.6-terra", "ok")}}, func(o *harnessOptions) { o.direct.MaxRetries = 0 })
			freshAuth(t, h.home, h.now)
			_, err = h.transport.InvokeStream(t.Context(), h.home, testRequest(), func(string) error { return nil })
			var upstream *UpstreamError
			if !errors.As(err, &upstream) || upstream.Kind != tc.wantKind || !upstream.Retryable || h.upstream.count() != 1 {
				t.Fatalf("MaxRetries=0: err=%v attempts=%d, want one retryable %s", err, h.upstream.count(), tc.wantKind)
			}
			if strings.Contains(err.Error(), "SENTINEL") {
				t.Fatalf("error leaks upstream text: %v", err)
			}
		})
	}
}
