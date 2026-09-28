package codexsubscription

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"

	"github.com/rs/zerolog"
)

// forbiddenInOutput is what no log line or error string may ever contain.
var forbiddenInOutput = []string{"SENTINEL-", "Bearer", "refresh_token=", "access_token", "error_description"}

func assertClean(t *testing.T, what, output string) {
	t.Helper()
	for _, forbidden := range forbiddenInOutput {
		if strings.Contains(output, forbidden) {
			t.Fatalf("%s contains %q:\n%s", what, forbidden, output)
		}
	}
}

func TestNoSecretsInLogsOrErrors(t *testing.T) {
	const upstreamMessage = "SENTINEL-upstream-message try again in 2s"
	promptSentinel := sentinel("prompt")
	answerSentinel := sentinel("answer")
	failedFrame := sse(anyMap{"type": "response.failed", "response": anyMap{"error": anyMap{"code": "server_error", "message": upstreamMessage}}})
	scenarios := []struct {
		name     string
		script   []fakeTurn
		respond  func(int, url.Values) (int, string)
		auth     func(h *directHarness)
		stream   bool
		wantFail bool
	}{
		{name: "success stream", script: []fakeTurn{{frames: successFrames("gpt-5.6-terra", answerSentinel, "tail")}}, stream: true},
		{name: "success unary with refresh", script: []fakeTurn{{frames: successFrames("gpt-5.6-terra", answerSentinel)}},
			respond: func(int, url.Values) (int, string) {
				return http.StatusOK, grantBody(rotatedAccess(time.Now(), sentinel("rotated")), sentinel("refresh"), "")
			},
			auth: func(h *directHarness) {
				writeAuthJSON(h.t, h.home, authOptions{accessExp: h.now.Add(time.Minute)})
			}},
		{name: "permanent refresh failure", wantFail: true,
			respond: func(int, url.Values) (int, string) {
				return http.StatusBadRequest, `{"error":"invalid_grant","error_description":"SENTINEL-description"}`
			},
			auth: func(h *directHarness) { writeAuthJSON(h.t, h.home, authOptions{accessExp: h.now.Add(time.Minute)}) }},
		{name: "transient refresh failure", wantFail: true,
			respond: func(int, url.Values) (int, string) {
				return http.StatusServiceUnavailable, `{"error":"SENTINEL-description"}`
			},
			auth: func(h *directHarness) { writeAuthJSON(h.t, h.home, authOptions{accessExp: h.now.Add(-time.Minute)}) }},
		{name: "401 twice", wantFail: true, script: []fakeTurn{{status: http.StatusUnauthorized, body: `{"error":{"message":"SENTINEL-upstream-message"}}`}},
			respond: func(int, url.Values) (int, string) {
				return http.StatusOK, grantBody(rotatedAccess(time.Now(), sentinel("rotated")), "", "")
			}},
		{name: "usage limit", wantFail: true, script: []fakeTurn{{status: http.StatusTooManyRequests, body: `{"error":{"type":"usage_limit_reached","message":"SENTINEL-upstream-message","plan_type":"pro","resets_in_seconds":60}}`}}},
		{name: "bad request", wantFail: true, script: []fakeTurn{{status: http.StatusBadRequest, body: `{"error":{"code":"SENTINEL-code","message":"SENTINEL-upstream-message"}}`}}},
		{name: "response failed", wantFail: true, stream: true, script: []fakeTurn{{frames: []string{failedFrame}}}},
	}
	for _, scenario := range scenarios {
		t.Run(scenario.name, func(t *testing.T) {
			h := newDirectHarness(t, scenario.script, func(o *harnessOptions) {
				o.respond = scenario.respond
				o.direct.MaxRetries = 1
			})
			if scenario.auth != nil {
				scenario.auth(h)
			} else {
				freshAuth(t, h.home, h.now)
			}
			request := testRequest()
			request.Messages = []ChatMessage{{Role: "system", Content: "rules " + promptSentinel}, {Role: "user", Content: "question " + promptSentinel}}
			var err error
			if scenario.stream {
				_, err = h.transport.InvokeStream(t.Context(), h.home, request, func(string) error { return nil })
			} else {
				_, err = h.transport.Invoke(t.Context(), h.home, request)
			}
			if (err != nil) != scenario.wantFail {
				t.Fatalf("error = %v, want failure %v", err, scenario.wantFail)
			}
			if err != nil {
				assertClean(t, "error", err.Error())
				assertClean(t, "error %+v", fmt.Sprintf("%+v %#v", err, err))
			}
			logs := h.logs.String()
			if !strings.Contains(logs, "codex_subscription_upstream") {
				t.Fatalf("no upstream log line: %s", logs)
			}
			assertClean(t, "log", logs)
		})
	}
}

func TestTokenSetRedactsEverywhere(t *testing.T) {
	home := t.TempDir()
	fixture := writeAuthJSON(t, home, authOptions{accessExp: time.Now().Add(time.Hour), accountID: strPtr("acct")})
	snapshot, err := readAuthSnapshot(home)
	if err != nil {
		t.Fatal(err)
	}
	tokens := snapshot.set
	encoded, err := json.Marshal(tokens)
	if err != nil {
		t.Fatal(err)
	}
	var zlog strings.Builder
	logger := zerolog.New(&zlog)
	logger.Info().Interface("t", tokens).Interface("s", tokens.Access).Object("o", tokens).Msg("x")
	outputs := []string{
		fmt.Sprintf("%v|%+v|%#v|%s", tokens, tokens, tokens, tokens),
		fmt.Sprintf("%v|%+v|%#v|%s", tokens.Access, tokens.Refresh, tokens.ID, tokens.Access),
		fmt.Sprintf("%v", []any{tokens, &tokens}),
		string(encoded),
		zlog.String(),
	}
	for _, output := range outputs {
		for _, value := range []string{fixture.access, fixture.refresh, fixture.id, "SENTINEL-"} {
			if strings.Contains(output, value) {
				t.Fatalf("a token leaked through %q", output)
			}
		}
		if !strings.Contains(output, "[redacted]") && output != zlog.String() {
			t.Fatalf("output %q is not redacted", output)
		}
	}
}
