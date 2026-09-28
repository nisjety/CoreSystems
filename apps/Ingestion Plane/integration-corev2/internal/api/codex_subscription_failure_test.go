package api

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/store"
)

const (
	unaryInferPath  = "/internal/model-subscriptions/openai-codex/infer"
	streamInferPath = "/internal/model-subscriptions/openai-codex/infer/stream"
)

// failureBody is the unary error envelope with every optional failure field.
type failureBody struct {
	Success bool `json:"success"`
	Error   struct {
		Code             string `json:"code"`
		Message          string `json:"message"`
		ResetsAt         string `json:"resetsAt"`
		RetryAfterMs     int64  `json:"retryAfterMs"`
		WindowMinutes    int    `json:"windowMinutes"`
		LimitReachedType string `json:"limitReachedType"`
	} `json:"error"`
}

func decodeFailureBody(t *testing.T, body string) failureBody {
	t.Helper()
	var decoded failureBody
	if err := json.Unmarshal([]byte(body), &decoded); err != nil {
		t.Fatalf("decode %q: %v", body, err)
	}
	return decoded
}

// streamErrorEvent returns the data of the stream's one subscription.error
// event, after checking the stream is ready, then that error, and no done.
func streamErrorEvent(t *testing.T, stream string) map[string]any {
	t.Helper()
	ready := strings.Index(stream, "event: subscription.ready\n")
	failed := strings.Index(stream, "event: subscription.error\ndata: ")
	if ready < 0 || failed <= ready || strings.Count(stream, "event: subscription.error") != 1 || strings.Contains(stream, "subscription.done") {
		t.Fatalf("stream = %q, want ready then exactly one error", stream)
	}
	data := stream[failed+len("event: subscription.error\ndata: "):]
	data = data[:strings.Index(data, "\n")]
	var event map[string]any
	if err := json.Unmarshal([]byte(data), &event); err != nil {
		t.Fatalf("decode error event %q: %v", data, err)
	}
	return event
}

// TestSubscriptionFailureContract pins, for every public failure code, the
// unary status, code, message, Retry-After and optional fields, and the
// streamed subscription.error event carrying the same fields.
func TestSubscriptionFailureContract(t *testing.T) {
	resetsAt := time.Now().Add(2 * time.Hour).UTC().Truncate(time.Second)
	cases := []struct {
		name        string
		err         error
		status      int
		code        string
		message     string
		retryAfter  bool
		resetsAt    string
		retryMs     bool
		window      int
		limitType   string
		streamedMsg string
	}{
		{
			name:   "usage limit with a reset",
			err:    &codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitUsage, ResetsAt: resetsAt, RetryAfter: 2 * time.Hour, WindowMinutes: 300, LimitReachedType: "rate_limit_reached"},
			status: http.StatusTooManyRequests, code: "subscription_usage_limit", message: "The connected ChatGPT plan has reached its Codex usage limit.",
			retryAfter: true, resetsAt: resetsAt.Format(time.RFC3339), retryMs: true, window: 300, limitType: "rate_limit_reached",
		},
		{
			// Unknown reset: no Retry-After and no made-up time. It may also
			// be a plan without Codex, which the app-server reports the same.
			name:   "usage limit without a reset",
			err:    &codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitUsage, LimitReachedType: "SENTINEL-type"},
			status: http.StatusTooManyRequests, code: "subscription_usage_limit", message: "The connected ChatGPT plan has reached its Codex usage limit, or does not include Codex. Check the plan in ChatGPT.",
		},
		{
			name:   "usage limit with only a retry hint",
			err:    &codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitUsage, RetryAfter: 2 * time.Minute},
			status: http.StatusTooManyRequests, code: "subscription_usage_limit", message: "The connected ChatGPT plan has reached its Codex usage limit.",
			retryAfter: true, retryMs: true,
		},
		{
			name:   "plan without Codex usage",
			err:    &codexsubscription.UpstreamError{Kind: "usage_not_included", HTTPStatus: 429, Code: "usage_not_included"},
			status: http.StatusTooManyRequests, code: "subscription_usage_limit", message: "The connected ChatGPT plan has reached its Codex usage limit, or does not include Codex. Check the plan in ChatGPT.",
		},
		{
			// rate_limited does not blame ChatGPT: it is also the broker's
			// own queue (below) and Integration Core's request limiter.
			name:   "throttle",
			err:    &codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitThrottle, RetryAfter: 2 * time.Second},
			status: http.StatusTooManyRequests, code: "rate_limited", message: "The ChatGPT subscription is busy. Retry after the indicated time.",
			retryAfter: true, retryMs: true, streamedMsg: "The ChatGPT subscription is busy. Retry later.",
		},
		{
			name:   "broker queue",
			err:    &codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitQueue, RetryAfter: time.Second},
			status: http.StatusTooManyRequests, code: "rate_limited", message: "The ChatGPT subscription is busy. Retry after the indicated time.",
			retryAfter: true, retryMs: true, streamedMsg: "The ChatGPT subscription is busy. Retry later.",
		},
		{
			name:   "reauthentication",
			err:    codexsubscription.ErrReauthenticationRequired,
			status: http.StatusConflict, code: "subscription_reauth_required", message: "Reconnect your ChatGPT subscription in Integrations.",
		},
		{
			name:   "upstream outage",
			err:    &codexsubscription.UpstreamError{Kind: "http_status", HTTPStatus: 503, Retryable: true},
			status: http.StatusServiceUnavailable, code: "subscription_upstream_unavailable", message: "ChatGPT did not answer. Try again shortly.",
		},
		{
			name:   "broker deadline",
			err:    context.DeadlineExceeded,
			status: http.StatusServiceUnavailable, code: "subscription_upstream_unavailable", message: "ChatGPT did not answer. Try again shortly.",
		},
		{
			name:   "context too long",
			err:    &codexsubscription.UpstreamError{Kind: "context_length_exceeded", Code: "context_length_exceeded"},
			status: http.StatusRequestEntityTooLarge, code: "subscription_context_too_long", message: "The conversation is too long for the ChatGPT model.",
		},
		{
			name:   "bad request",
			err:    &codexsubscription.UpstreamError{Kind: "http_status", HTTPStatus: 400, Code: "invalid_value"},
			status: http.StatusUnprocessableEntity, code: "subscription_bad_request", message: "ChatGPT rejected the request.",
		},
		{
			name:   "policy",
			err:    &codexsubscription.UpstreamError{Kind: "policy", Code: "cyber_policy"},
			status: http.StatusUnprocessableEntity, code: "subscription_bad_request", message: "ChatGPT rejected the request.",
		},
		{
			name:   "anything else",
			err:    errors.New("SENTINEL-internal detail"),
			status: http.StatusBadGateway, code: "subscription_broker_failed", message: "The ChatGPT subscription broker could not complete the request.",
			streamedMsg: "The ChatGPT subscription broker could not complete the streamed request.",
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			app, _ := fakeRunnerStack(t, &apiCodexRunner{invokeErr: tc.err}, nil)
			resp := postInfer(t, app, unaryInferPath, directInferBody, -1)
			body := readBody(t, resp)
			if resp.StatusCode != tc.status {
				t.Fatalf("status = %d, want %d: %s", resp.StatusCode, tc.status, body)
			}
			decoded := decodeFailureBody(t, body)
			if decoded.Success || decoded.Error.Code != tc.code || decoded.Error.Message != tc.message {
				t.Fatalf("body = %s, want code %s and message %q", body, tc.code, tc.message)
			}
			header := resp.Header.Get("Retry-After")
			if (header != "") != tc.retryAfter {
				t.Fatalf("Retry-After = %q, want present=%v", header, tc.retryAfter)
			}
			if header != "" {
				seconds, err := strconv.ParseInt(header, 10, 64)
				if err != nil || seconds < 1 || seconds > 604800 || seconds != (decoded.Error.RetryAfterMs+999)/1000 {
					t.Fatalf("Retry-After = %q with retryAfterMs %d", header, decoded.Error.RetryAfterMs)
				}
			}
			if decoded.Error.ResetsAt != tc.resetsAt || (decoded.Error.RetryAfterMs > 0) != tc.retryMs || decoded.Error.WindowMinutes != tc.window || decoded.Error.LimitReachedType != tc.limitType {
				t.Fatalf("body = %s, want resetsAt %q retryAfterMs %v window %d type %q", body, tc.resetsAt, tc.retryMs, tc.window, tc.limitType)
			}
			if strings.Contains(body, "SENTINEL") {
				t.Fatalf("body leaks internal text: %s", body)
			}

			// A fresh stack: a reauthentication marks the connection
			// needs_refresh, which the pre-flight then refuses.
			app, _ = fakeRunnerStack(t, &apiCodexRunner{invokeErr: tc.err}, nil)
			resp = postInfer(t, app, streamInferPath, directInferBody, -1)
			stream := readBody(t, resp)
			if resp.StatusCode != http.StatusOK {
				t.Fatalf("stream status = %d: %s", resp.StatusCode, stream)
			}
			event := streamErrorEvent(t, stream)
			want := tc.streamedMsg
			if want == "" {
				want = tc.message
			}
			if event["type"] != "error" || event["code"] != tc.code || event["message"] != want {
				t.Fatalf("stream error = %v, want code %s and message %q", event, tc.code, want)
			}
			if resetsAt, _ := event["resetsAt"].(string); resetsAt != tc.resetsAt {
				t.Fatalf("stream resetsAt = %v, want %q", event["resetsAt"], tc.resetsAt)
			}
			if _, present := event["retryAfterMs"]; present != tc.retryMs {
				t.Fatalf("stream retryAfterMs = %v, want present=%v", event["retryAfterMs"], tc.retryMs)
			}
			if window, _ := event["windowMinutes"].(float64); int(window) != tc.window {
				t.Fatalf("stream windowMinutes = %v, want %d", event["windowMinutes"], tc.window)
			}
			if limitType, _ := event["limitReachedType"].(string); limitType != tc.limitType {
				t.Fatalf("stream limitReachedType = %v, want %q", event["limitReachedType"], tc.limitType)
			}
			if strings.Contains(stream, "SENTINEL") {
				t.Fatalf("stream leaks internal text: %s", stream)
			}
		})
	}
}

// The exact streamed usage-limit event, field for field.
func TestStreamUsageLimitEventShape(t *testing.T) {
	encoded, err := json.Marshal(subscriptionFailureStreamEvent(codexsubscription.Failure{
		Code:          codexsubscription.FailureUsageLimit,
		ResetsAt:      time.Date(2026, 9, 26, 1, 21, 0, 0, time.UTC),
		RetryAfter:    52260 * time.Second,
		WindowMinutes: 300,
	}))
	if err != nil {
		t.Fatal(err)
	}
	const want = `{"type":"error","code":"subscription_usage_limit","message":"The connected ChatGPT plan has reached its Codex usage limit.","resetsAt":"2026-09-26T01:21:00Z","retryAfterMs":52260000,"windowMinutes":300}`
	if string(encoded) != want {
		t.Fatalf("event = %s\nwant    %s", encoded, want)
	}
}

// A connection already marked needs_refresh is refused before any turn, on
// both routes, with the reconnect code.
func TestNeedsRefreshConnectionIsReauthBeforeAnyTurn(t *testing.T) {
	runner := &apiCodexRunner{}
	app, repo := fakeRunnerStack(t, runner, nil)
	connection, err := repo.GetConnection(t.Context(), directConnectionID)
	if err != nil {
		t.Fatal(err)
	}
	connection.Status = "needs_refresh"
	if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{unaryInferPath, streamInferPath} {
		resp := postInfer(t, app, path, directInferBody, -1)
		body := readBody(t, resp)
		if resp.StatusCode != http.StatusConflict || decodeAPIError(t, body).Error.Code != "subscription_reauth_required" {
			t.Fatalf("%s: status = %d body = %s, want 409 subscription_reauth_required", path, resp.StatusCode, body)
		}
	}
	if runner.invocations != 0 {
		t.Fatalf("invocations = %d, want none", runner.invocations)
	}

	// Any other inactive state keeps its own code.
	connection.Status = "pending"
	if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
		t.Fatal(err)
	}
	resp := postInfer(t, app, unaryInferPath, directInferBody, -1)
	if body := readBody(t, resp); resp.StatusCode != http.StatusConflict || decodeAPIError(t, body).Error.Code != "subscription_connection_inactive" {
		t.Fatalf("pending: status = %d body = %s", resp.StatusCode, body)
	}
	var stored store.Connection
	if stored, err = repo.GetConnection(t.Context(), directConnectionID); err != nil || stored.Status != "pending" {
		t.Fatalf("connection = %+v, %v", stored, err)
	}
}

// directCanary is upstream text shaped like Codex's own usage-limit message.
// It must never reach a response body, an SSE event or a log line.
const directCanary = "Try again at 3:05 PM CANARY"

type lockedBuffer struct {
	mu  sync.Mutex
	buf strings.Builder
}

func (b *lockedBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *lockedBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}

func TestDirectFailureCanaryNeverLeaves(t *testing.T) {
	resetsAt := strconv.FormatInt(time.Now().Add(time.Hour).Unix(), 10)
	scenarios := []struct {
		name    string
		code    string
		handler http.HandlerFunc
	}{
		{name: "usage limit body", code: "subscription_usage_limit", handler: func(w http.ResponseWriter, _ *http.Request) {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = io.WriteString(w, `{"error":{"type":"usage_limit_reached","message":"`+directCanary+`","plan_type":"plus","resets_at":`+resetsAt+`}}`)
		}},
		{name: "bad request body", code: "subscription_bad_request", handler: func(w http.ResponseWriter, _ *http.Request) {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusBadRequest)
			_, _ = io.WriteString(w, `{"error":{"code":"invalid_value","message":"`+directCanary+`"},"detail":"`+directCanary+`"}`)
		}},
		{name: "response.failed usage limit", code: "subscription_usage_limit", handler: serveFrames([]string{
			directSSE(map[string]any{"type": "response.created", "response": map[string]any{"id": "resp_1", "model": "gpt-5.6-terra"}}),
			directSSE(map[string]any{"type": "response.failed", "response": map[string]any{"error": map[string]any{"code": "usage_limit_reached", "message": directCanary, "resets_in_seconds": 600}}}),
		})},
		{name: "response.failed context", code: "subscription_context_too_long", handler: serveFrames([]string{
			directSSE(map[string]any{"type": "response.failed", "response": map[string]any{"error": map[string]any{"code": "context_length_exceeded", "message": directCanary}}}),
		})},
	}
	for _, scenario := range scenarios {
		t.Run(scenario.name, func(t *testing.T) {
			logs := &lockedBuffer{}
			app, _ := directStack(t, scenario.handler, directStackOptions{logs: logs})
			unary := postInfer(t, app, unaryInferPath, directInferBody, -1)
			body := readBody(t, unary)
			if decodeAPIError(t, body).Error.Code != scenario.code {
				t.Fatalf("unary body = %s, want %s", body, scenario.code)
			}
			streamResp := postInfer(t, app, streamInferPath, directInferBody, -1)
			stream := readBody(t, streamResp)
			if event := streamErrorEvent(t, stream); event["code"] != scenario.code {
				t.Fatalf("stream = %s, want %s", stream, scenario.code)
			}
			for what, output := range map[string]string{"unary body": body, "stream": stream, "logs": logs.String()} {
				if strings.Contains(output, "CANARY") || strings.Contains(output, "3:05") {
					t.Fatalf("%s leaks upstream text: %s", what, output)
				}
			}
			if !strings.Contains(logs.String(), `"reason":"`+scenario.code+`"`) {
				t.Fatalf("logs lack the reason %s: %s", scenario.code, logs.String())
			}
		})
	}
}
