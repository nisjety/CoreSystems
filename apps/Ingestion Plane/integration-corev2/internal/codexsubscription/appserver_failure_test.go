package codexsubscription

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/rs/zerolog"
)

// turnErrorNotification is the pinned v2 ErrorNotification; info is the raw
// codexErrorInfo JSON. message and additionalDetails carry the canary.
func turnErrorNotification(threadID, turnID, info string, willRetry bool) rpcNotification {
	return rpcNotification{Method: "error", Params: json.RawMessage(`{"error":{"message":"` + fakeCanary + `","codexErrorInfo":` + info + `,"additionalDetails":"` + fakeCanary + `"},"willRetry":` + strconv.FormatBool(willRetry) + `,"threadId":"` + threadID + `","turnId":"` + turnID + `"}`)}
}

// turnEnded is turn/completed with the given status and, unless info is
// empty, a TurnError.
func turnEnded(threadID, turnID, status, info string) rpcNotification {
	turnError := "null"
	if info != "" {
		turnError = `{"message":"` + fakeCanary + `","codexErrorInfo":` + info + `,"additionalDetails":"` + fakeCanary + `"}`
	}
	return rpcNotification{Method: "turn/completed", Params: json.RawMessage(`{"threadId":"` + threadID + `","turn":{"id":"` + turnID + `","status":"` + status + `","items":[],"error":` + turnError + `}}`)}
}

// rateLimitsUpdated is account/rateLimits/updated with the given primary and
// secondary windows (raw JSON or null). planType is present and never read.
func rateLimitsUpdated(primary, secondary string) rpcNotification {
	return rpcNotification{Method: "account/rateLimits/updated", Params: json.RawMessage(`{"rateLimits":{"limitId":"codex","limitName":null,"primary":` + primary + `,"secondary":` + secondary + `,"credits":null,"individualLimit":null,"spendControlReached":null,"planType":"plus","rateLimitReachedType":"rate_limit_reached"}}`)}
}

func rateWindow(usedPercent, minutes int, resetsAt time.Time) string {
	return `{"usedPercent":` + strconv.Itoa(usedPercent) + `,"windowDurationMins":` + strconv.Itoa(minutes) + `,"resetsAt":` + strconv.FormatInt(resetsAt.Unix(), 10) + `}`
}

func TestWaitForTurnClassifiesFailedTurns(t *testing.T) {
	now := time.Now().UTC().Truncate(time.Second)
	soon, later := now.Add(2*time.Hour), now.Add(50*time.Hour)
	cases := []struct {
		name          string
		notifications []rpcNotification
		want          FailureCode
		resetsAt      time.Time
		window        int
	}{
		{
			name: "usage limit with the notified reset",
			notifications: []rpcNotification{
				rateLimitsUpdated(rateWindow(100, 300, soon), rateWindow(30, 10080, later)),
				turnErrorNotification("thread-1", "turn-1", `"usageLimitExceeded"`, false),
				turnEnded("thread-1", "turn-1", "failed", `"usageLimitExceeded"`),
			},
			want: FailureUsageLimit, resetsAt: soon, window: 300,
		},
		{
			name: "both windows exhausted take the later reset",
			notifications: []rpcNotification{
				rateLimitsUpdated(rateWindow(100, 300, soon), rateWindow(100, 10080, later)),
				turnEnded("thread-1", "turn-1", "failed", `"usageLimitExceeded"`),
			},
			want: FailureUsageLimit, resetsAt: later, window: 10080,
		},
		{
			// A sparse update keeps the earlier exhausted window.
			name: "sparse update keeps the kept window",
			notifications: []rpcNotification{
				rateLimitsUpdated(rateWindow(100, 300, soon), "null"),
				rateLimitsUpdated("null", rateWindow(10, 10080, later)),
				turnEnded("thread-1", "turn-1", "failed", `"usageLimitExceeded"`),
			},
			want: FailureUsageLimit, resetsAt: soon, window: 300,
		},
		{
			name:          "usage limit without an exhausted window is unknown",
			notifications: []rpcNotification{rateLimitsUpdated(rateWindow(99, 300, soon), "null"), turnEnded("thread-1", "turn-1", "failed", `"usageLimitExceeded"`)},
			want:          FailureUsageLimit,
		},
		{name: "retry limit on a 503", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `{"responseTooManyFailedAttempts":{"httpStatusCode":503}}`)}, want: FailureUpstreamUnavailable},
		{name: "retry limit on a 429", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `{"responseTooManyFailedAttempts":{"httpStatusCode":429}}`)}, want: FailureRateLimited},
		{name: "rate limit exceeded", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"rateLimitExceeded"`)}, want: FailureRateLimited},
		{name: "server overloaded", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"serverOverloaded"`)}, want: FailureUpstreamUnavailable},
		{name: "stream disconnected", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `{"responseStreamDisconnected":{"httpStatusCode":null}}`)}, want: FailureUpstreamUnavailable},
		{name: "context window", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"contextWindowExceeded"`)}, want: FailureContextTooLong},
		{name: "cyber policy", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"cyberPolicy"`)}, want: FailureBadRequest},
		{name: "misalignment policy", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"misalignmentPolicyViolation"`)}, want: FailureBadRequest},
		{
			name: "other after Codex retried",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":502}}`, true),
				turnErrorNotification("thread-1", "turn-1", `"other"`, false),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureUpstreamUnavailable,
		},
		{
			// Codex retries a dropped stream without a status: transient.
			name: "other after Codex retried a dropped stream",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":null}}`, true),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureUpstreamUnavailable,
		},
		{
			name: "other after Codex retried a 408",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":408}}`, true),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureUpstreamUnavailable,
		},
		{
			// Codex 0.153.4 retries every unexpected status, a 403 too, and
			// then fails the turn as `other` without it. A retry will not
			// cure a 403, so it is not labelled transient.
			name: "other after Codex retried a 403",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":403}}`, true),
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":403}}`, true),
				turnErrorNotification("thread-1", "turn-1", `"other"`, false),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureBrokerFailed,
		},
		{
			name: "other after Codex retried a 404",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":404}}`, true),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureBrokerFailed,
		},
		{
			// The latest retry names the attempt closest to the final one.
			name: "the latest retry's status decides",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":403}}`, true),
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":502}}`, true),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureUpstreamUnavailable,
		},
		{
			name: "a later retry without a status is transient",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":403}}`, true),
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":null}}`, true),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureUpstreamUnavailable,
		},
		{
			// A retry of another turn names nothing about this one.
			name: "another turn's retried 403 is ignored",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `{"responseStreamDisconnected":{"httpStatusCode":502}}`, true),
				turnErrorNotification("thread-1", "turn-2", `{"responseStreamDisconnected":{"httpStatusCode":403}}`, true),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureUpstreamUnavailable,
		},
		{name: "other alone", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"other"`)}, want: FailureBrokerFailed},
		{name: "unknown variant", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"flexUnavailable"`)}, want: FailureBrokerFailed},
		{name: "unknown object variant", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `{"flexUnavailable":{"httpStatusCode":503}}`)}, want: FailureBrokerFailed},
		{name: "app-server bad request", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"badRequest"`)}, want: FailureBrokerFailed},
		{name: "session budget", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "failed", `"sessionBudgetExceeded"`)}, want: FailureBrokerFailed},
		{
			// turn.error without codexErrorInfo falls back to the last error.
			name: "failed turn falls back to the last error",
			notifications: []rpcNotification{
				turnErrorNotification("thread-1", "turn-1", `"contextWindowExceeded"`, false),
				turnEnded("thread-1", "turn-1", "failed", `null`),
			},
			want: FailureContextTooLong,
		},
		{name: "interrupted", notifications: []rpcNotification{turnEnded("thread-1", "turn-1", "interrupted", "")}, want: FailureBrokerFailed},
		{
			name: "notifications of another turn are ignored",
			notifications: []rpcNotification{
				rateLimitsUpdated(rateWindow(100, 300, soon), "null"),
				turnErrorNotification("thread-1", "turn-2", `"contextWindowExceeded"`, false),
				turnErrorNotification("thread-1", "turn-2", `"serverOverloaded"`, true),
				turnEnded("thread-1", "turn-2", "failed", `"cyberPolicy"`),
				turnEnded("thread-1", "turn-1", "failed", `"other"`),
			},
			want: FailureBrokerFailed,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := fakeTurnClient(tc.notifications...).waitForTurn(t.Context(), "thread-1", "turn-1", nil)
			if err == nil {
				t.Fatal("want a failed turn")
			}
			failure := ClassifyFailure(err, now)
			if failure.Code != tc.want || !failure.ResetsAt.Equal(tc.resetsAt) || failure.WindowMinutes != tc.window {
				t.Fatalf("error %#v classifies as %+v, want %s resetting at %v in a %d-minute window", err, failure, tc.want, tc.resetsAt, tc.window)
			}
			if tc.want == FailureUsageLimit {
				var limited *RateLimitError
				if !errors.As(err, &limited) || limited.Kind != RateLimitUsage || limited.LimitReachedType != "rate_limit_reached" {
					t.Fatalf("error = %#v, want a usage-limit *RateLimitError", err)
				}
			}
			if output := fmt.Sprintf("%v | %+v | %#v", err, err, err); strings.Contains(output, "CANARY") || strings.Contains(output, "3:05") {
				t.Fatalf("error leaks upstream text: %s", output)
			}
		})
	}
}

func TestParseCodexErrorInfo(t *testing.T) {
	cases := []struct {
		raw  string
		want codexErrorInfo
	}{
		{raw: ``, want: codexErrorInfo{}},
		{raw: `null`, want: codexErrorInfo{}},
		{raw: `"usageLimitExceeded"`, want: codexErrorInfo{present: true, code: "usage_limit_exceeded"}},
		{raw: `{"httpConnectionFailed":{"httpStatusCode":502}}`, want: codexErrorInfo{present: true, code: "http_connection_failed", httpStatus: 502}},
		{raw: `{"responseStreamConnectionFailed":{"httpStatusCode":null}}`, want: codexErrorInfo{present: true, code: "response_stream_connection_failed"}},
		{raw: `{"activeTurnNotSteerable":{"turnKind":"review"}}`, want: codexErrorInfo{present: true, code: "active_turn_not_steerable"}},
		{raw: `{"responseTooManyFailedAttempts":{"httpStatusCode":99999}}`, want: codexErrorInfo{present: true, code: "response_too_many_failed_attempts"}},
		{raw: `"SENTINEL-variant"`, want: codexErrorInfo{present: true, code: "unrecognized"}},
		{raw: `{"a":{},"b":{}}`, want: codexErrorInfo{present: true, code: "unrecognized"}},
		{raw: `42`, want: codexErrorInfo{present: true, code: "unrecognized"}},
	}
	for _, tc := range cases {
		if got := parseCodexErrorInfo(json.RawMessage(tc.raw)); got != tc.want {
			t.Fatalf("parseCodexErrorInfo(%s) = %+v, want %+v", tc.raw, got, tc.want)
		}
	}
}

// A JSON-RPC error keeps only its numeric code: its message is upstream text.
func TestAppServerRPCErrorDropsItsMessage(t *testing.T) {
	requests, stdin := io.Pipe()
	stdout, responses := io.Pipe()
	t.Cleanup(func() {
		_ = requests.Close()
		_ = responses.Close()
	})
	client := newAppServerClient(&exec.Cmd{}, stdin, stdout)
	go func() {
		scanner := bufio.NewScanner(requests)
		if !scanner.Scan() {
			return
		}
		var request struct {
			ID json.RawMessage `json:"id"`
		}
		_ = json.Unmarshal(scanner.Bytes(), &request)
		_, _ = io.WriteString(responses, `{"jsonrpc":"2.0","id":`+string(request.ID)+`,"error":{"code":-32603,"message":"`+fakeCanary+`"}}`+"\n")
	}()
	err := client.call(t.Context(), "account/rateLimits/read", codexEmptyObjectParams(), nil)
	if err == nil || strings.Contains(err.Error(), "CANARY") || err.Error() != "account/rateLimits/read: codex app-server error -32603" {
		t.Fatalf("error = %v, want the code without the message", err)
	}
}

// failingAppServer is a ProcessRunner against the fake app-server running a
// failure scenario, with its log, the file of the calls the fake received,
// and the reset the fake reports (two hours ahead, in whole seconds).
type failingAppServer struct {
	runner   *ProcessRunner
	home     string
	calls    string
	logs     *syncBuffer
	resetsAt time.Time
}

func newFailingAppServer(t *testing.T, scenario string, store *credentialStore, home string) *failingAppServer {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatalf("locate test binary: %v", err)
	}
	resetsAt := time.Now().Add(2 * time.Hour).UTC().Truncate(time.Second)
	calls := filepath.Join(t.TempDir(), "calls.txt")
	t.Setenv(fakeAppServerEnv, "1")
	t.Setenv(fakeScenarioEnv, scenario)
	t.Setenv(fakeCallsEnv, calls)
	t.Setenv(fakeResetsAtEnv, strconv.FormatInt(resetsAt.Unix(), 10))
	if home == "" {
		home = t.TempDir()
		if err := os.WriteFile(filepath.Join(home, "auth.json"), []byte("{}"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	logs := &syncBuffer{}
	options := []ProcessOption{WithLogger(zerolog.New(logs))}
	if store != nil {
		options = append(options, WithCredentials(store))
	}
	return &failingAppServer{runner: NewProcessRunner(executable, options...), home: home, calls: calls, logs: logs, resetsAt: resetsAt}
}

func (f *failingAppServer) invoke(t *testing.T, streamed bool) error {
	t.Helper()
	request := InvokeRequest{RequestID: "req-1", ConnectionID: "conn-1", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "hello"}}, ReasoningEffort: "low"}
	var err error
	if streamed {
		forwarded := 0
		_, err = f.runner.InvokeStream(t.Context(), f.home, request, func(string) error {
			forwarded++
			return nil
		})
		if forwarded != 0 {
			t.Fatalf("a refused turn forwarded %d deltas", forwarded)
		}
	} else {
		_, err = f.runner.Invoke(t.Context(), f.home, request)
	}
	if err == nil {
		t.Fatal("want the turn to fail")
	}
	return err
}

// rateLimitReads is how many account/rateLimits/read requests the fake got;
// each must have carried {} params.
func (f *failingAppServer) rateLimitReads(t *testing.T) int {
	t.Helper()
	recorded, err := os.ReadFile(f.calls)
	if err != nil {
		t.Fatalf("read recorded calls: %v", err)
	}
	reads := 0
	for _, line := range strings.Split(strings.TrimSpace(string(recorded)), "\n") {
		if method, params, _ := strings.Cut(line, " "); method == "account/rateLimits/read" {
			if params != "{}" {
				t.Fatalf("account/rateLimits/read params = %s, want {}", params)
			}
			reads++
		}
	}
	return reads
}

// assertNoCanary checks the upstream message and additionalDetails reached
// neither the error nor the log.
func (f *failingAppServer) assertNoCanary(t *testing.T, err error) {
	t.Helper()
	for what, output := range map[string]string{"error": fmt.Sprintf("%v | %+v | %#v", err, err, err), "log": f.logs.String()} {
		if strings.Contains(output, "CANARY") || strings.Contains(output, "3:05") {
			t.Fatalf("%s leaks upstream text: %s", what, output)
		}
	}
}

// logLine is the call's one codex_subscription_upstream line.
func (f *failingAppServer) logLine(t *testing.T) map[string]any {
	t.Helper()
	var found map[string]any
	for _, line := range strings.Split(strings.TrimSpace(f.logs.String()), "\n") {
		var entry map[string]any
		if json.Unmarshal([]byte(line), &entry) == nil && entry["message"] == "codex_subscription_upstream" {
			if found != nil {
				t.Fatalf("more than one failure line: %s", f.logs.String())
			}
			found = entry
		}
	}
	if found == nil {
		t.Fatalf("no codex_subscription_upstream line: %s", f.logs.String())
	}
	return found
}

func TestAppServerUsageLimitTakesTheNotifiedReset(t *testing.T) {
	for _, streamed := range []bool{false, true} {
		fake := newFailingAppServer(t, fakeUsageLimitNotified, nil, "")
		err := fake.invoke(t, streamed)
		var limited *RateLimitError
		if !errors.As(err, &limited) || limited.Kind != RateLimitUsage || !limited.ResetsAt.Equal(fake.resetsAt) || limited.WindowMinutes != 300 || limited.LimitReachedType != "rate_limit_reached" {
			t.Fatalf("streamed=%v: error = %#v, want a usage limit resetting at %v in a 300-minute window", streamed, err, fake.resetsAt)
		}
		if reads := fake.rateLimitReads(t); reads != 0 {
			t.Fatalf("streamed=%v: rate-limit reads = %d, want none: the notification gave the reset", streamed, reads)
		}
		failure := ClassifyFailure(err, time.Now())
		if failure.Code != FailureUsageLimit || failure.FormatResetsAt() != fake.resetsAt.Format(time.RFC3339) {
			t.Fatalf("streamed=%v: failure = %+v", streamed, failure)
		}
		line := fake.logLine(t)
		want := map[string]any{
			"level": "warn", "transport": "app_server", "request_id": "req-1", "connection_id": "conn-1",
			"error_kind": "usage_limit", "error_code": "usage_limit_exceeded", "http_status": float64(0), "upstream_retries": float64(0),
			"reason": "subscription_usage_limit", "resets_at": fake.resetsAt.Format(time.RFC3339), "window_mins": float64(300),
			"limit_reached_type": "rate_limit_reached", "rate_limits_source": "notification",
		}
		for key, value := range want {
			if line[key] != value {
				t.Fatalf("streamed=%v: log %s = %v, want %v: %v", streamed, key, line[key], value, line)
			}
		}
		fake.assertNoCanary(t, err)
	}
}

func TestAppServerUsageLimitReadsRateLimitsOnce(t *testing.T) {
	fake := newFailingAppServer(t, fakeUsageLimitRead, nil, "")
	err := fake.invoke(t, false)
	var limited *RateLimitError
	if !errors.As(err, &limited) || limited.Kind != RateLimitUsage || !limited.ResetsAt.Equal(fake.resetsAt) || limited.WindowMinutes != 300 || limited.LimitReachedType != "rate_limit_reached" {
		t.Fatalf("error = %#v, want the reset account/rateLimits/read reported", err)
	}
	if reads := fake.rateLimitReads(t); reads != 1 {
		t.Fatalf("rate-limit reads = %d, want exactly one", reads)
	}
	if seconds, ok := ClassifyFailure(err, time.Now()).RetryAfterSeconds(); !ok || seconds < 7190 || seconds > 7200 {
		t.Fatalf("Retry-After = %d, %v; want about two hours", seconds, ok)
	}
	if line := fake.logLine(t); line["rate_limits_source"] != "read" || line["reason"] != "subscription_usage_limit" {
		t.Fatalf("log = %v", line)
	}
	fake.assertNoCanary(t, err)
}

func TestAppServerUsageLimitWithoutAReadableResetIsUnknown(t *testing.T) {
	previous := rateLimitsReadTimeout
	rateLimitsReadTimeout = 300 * time.Millisecond
	t.Cleanup(func() { rateLimitsReadTimeout = previous })
	for _, scenario := range []string{fakeUsageLimitReadError, fakeUsageLimitReadHang} {
		fake := newFailingAppServer(t, scenario, nil, "")
		started := time.Now()
		err := fake.invoke(t, true)
		if elapsed := time.Since(started); elapsed > 5*time.Second {
			t.Fatalf("%s: the read was not bounded: %v", scenario, elapsed)
		}
		var limited *RateLimitError
		if !errors.As(err, &limited) || limited.Kind != RateLimitUsage || !limited.ResetsAt.IsZero() || limited.RetryAfter != 0 {
			t.Fatalf("%s: error = %#v, want a usage limit with an unknown reset", scenario, err)
		}
		if reads := fake.rateLimitReads(t); reads != 1 {
			t.Fatalf("%s: rate-limit reads = %d, want exactly one", scenario, reads)
		}
		failure := ClassifyFailure(err, time.Now())
		if _, ok := failure.RetryAfterSeconds(); ok || failure.Code != FailureUsageLimit || failure.FormatResetsAt() != "" {
			t.Fatalf("%s: failure = %+v, want an unknown reset and no Retry-After", scenario, failure)
		}
		if line := fake.logLine(t); line["rate_limits_source"] != "none" || line["resets_at"] != "" {
			t.Fatalf("%s: log = %v", scenario, line)
		}
		fake.assertNoCanary(t, err)
	}
}

// A 403 Codex retried before failing the turn as `other` is not labelled
// transient, and the log line names the retried status.
func TestAppServerRetriedForbiddenIsNotTransient(t *testing.T) {
	fake := newFailingAppServer(t, fakeRetriedForbidden, nil, "")
	err := fake.invoke(t, true)
	if got := ClassifyFailure(err, time.Now()).Code; got != FailureBrokerFailed {
		t.Fatalf("error = %#v classifies as %s, want %s", err, got, FailureBrokerFailed)
	}
	line := fake.logLine(t)
	want := map[string]any{
		"error_kind": "turn_failed", "error_code": "other", "upstream_retries": float64(2),
		"retry_http_status": float64(403), "reason": "subscription_broker_failed",
	}
	for key, value := range want {
		if line[key] != value {
			t.Fatalf("log %s = %v, want %v: %v", key, line[key], value, line)
		}
	}
	fake.assertNoCanary(t, err)
}

// A turn Codex failed as unauthorized gets the broker's one forced recovery:
// a refresh the token endpoint refuses is reauthentication, a refresh that
// succeeds is a transient failure that never disconnects the user.
func TestAppServerUnauthorizedRunsOneForcedRecovery(t *testing.T) {
	cases := []struct {
		name    string
		respond func(int, url.Values) (int, string)
		want    FailureCode
	}{
		{name: "refresh refused", respond: func(int, url.Values) (int, string) {
			return http.StatusBadRequest, `{"error":"invalid_grant"}`
		}, want: FailureReauthRequired},
		{name: "refresh succeeds", respond: func(int, url.Values) (int, string) {
			return http.StatusOK, grantBody(rotatedAccess(time.Now(), sentinel("recovered")), "", "")
		}, want: FailureUpstreamUnavailable},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			store, auth, home, now := storeHarness(t, tc.respond)
			writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Hour), accountID: strPtr("acct-file")})
			fake := newFailingAppServer(t, fakeUnauthorized, store, home)
			err := fake.invoke(t, false)
			if auth.callCount() != 1 {
				t.Fatalf("token calls = %d, want exactly one forced refresh", auth.callCount())
			}
			if got := ClassifyFailure(err, time.Now()).Code; got != tc.want {
				t.Fatalf("error = %v classifies as %s, want %s", err, got, tc.want)
			}
			if (tc.want == FailureReauthRequired) != errors.Is(err, ErrReauthenticationRequired) {
				t.Fatalf("error = %v: only a refused refresh may mark the connection needs_refresh", err)
			}
			if line := fake.logLine(t); line["reason"] != string(tc.want) {
				t.Fatalf("log = %v", line)
			}
			fake.assertNoCanary(t, err)
			assertClean(t, "log", fake.logs.String())
		})
	}
}
