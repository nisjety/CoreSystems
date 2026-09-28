package codexsubscription

import (
	"bufio"
	"cmp"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/rs/zerolog"
)

// fakeAppServerEnv makes the test binary act as a minimal Codex app-server
// instead of running tests, so ProcessRunner can be driven end to end over
// real stdio pipes.
const fakeAppServerEnv = "INTEGRATION_CORE_FAKE_CODEX_APP_SERVER"

func TestMain(m *testing.M) {
	if os.Getenv(fakeAppServerEnv) == "1" {
		serveFakeAppServer(os.Stdin, os.Stdout)
		os.Exit(0)
	}
	os.Exit(m.Run())
}

// serveFakeAppServer answers one invocation's JSON-RPC requests in the
// app-server's documented shapes. It echoes the requested model, then sends
// one answer delta, a usage update and the turn's completion.
func serveFakeAppServer(in io.Reader, out io.Writer) {
	encoder := json.NewEncoder(out)
	reply := func(id json.RawMessage, result any) {
		_ = encoder.Encode(map[string]any{"jsonrpc": "2.0", "id": id, "result": result})
	}
	notify := func(method string, params any) {
		_ = encoder.Encode(map[string]any{"jsonrpc": "2.0", "method": method, "params": params})
	}
	usage := map[string]int64{"inputTokens": 1800, "cachedInputTokens": 1024, "cacheWriteInputTokens": 256, "outputTokens": 120, "reasoningOutputTokens": 40, "totalTokens": 1920}
	scanner := bufio.NewScanner(in)
	scanner.Buffer(make([]byte, 64*1024), 4*1024*1024)
	var threadStart fakeThreadStart
	var threadStartParams json.RawMessage
	scenario := os.Getenv(fakeScenarioEnv)
	for scanner.Scan() {
		recordFakeCall(scanner.Bytes())
		var request struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params struct {
				Model       string `json:"model"`
				ServiceTier string `json:"serviceTier"`
				Effort      string `json:"effort"`
				Config      struct {
					Effort string `json:"model_reasoning_effort"`
				} `json:"config"`
			} `json:"params"`
		}
		if json.Unmarshal(scanner.Bytes(), &request) != nil || len(request.ID) == 0 {
			continue // The initialized notification needs no answer.
		}
		switch request.Method {
		case "initialize", "account/logout":
			reply(request.ID, map[string]any{})
		case "thread/start":
			threadStart = fakeThreadStart{Effort: request.Params.Config.Effort, ServiceTier: request.Params.ServiceTier}
			var raw struct {
				Params json.RawMessage `json:"params"`
			}
			_ = json.Unmarshal(scanner.Bytes(), &raw)
			threadStartParams = raw.Params
			// Like Codex, answer with the thread's effective effort: the
			// override when given, else the model's default.
			effort := cmp.Or(request.Params.Config.Effort, "medium")
			reply(request.ID, map[string]any{"model": request.Params.Model, "modelProvider": "openai", "reasoningEffort": effort, "thread": map[string]string{"id": "thread-1"}})
		case "turn/start":
			threadStart.TurnEffort, threadStart.TurnServiceTier = request.Params.Effort, request.Params.ServiceTier
			reply(request.ID, map[string]any{"turn": map[string]string{"id": "turn-1"}})
			if scenario != "" {
				failFakeTurn(scenario, notify)
				continue
			}
			notify("item/agentMessage/delta", map[string]string{"threadId": "thread-1", "turnId": "turn-1", "itemId": "item-1", "delta": fakeAppServerAnswer(threadStart, threadStartParams)})
			notify("thread/tokenUsage/updated", map[string]any{"threadId": "thread-1", "turnId": "turn-1", "tokenUsage": map[string]any{"total": usage, "last": usage}})
			notify("turn/completed", map[string]any{"threadId": "thread-1", "turn": map[string]any{"id": "turn-1", "status": "completed", "items": []any{}}})
		case "account/rateLimits/read":
			switch scenario {
			case fakeUsageLimitRead:
				snapshot := fakeRateLimitSnapshot(100)
				reply(request.ID, map[string]any{"rateLimits": snapshot, "rateLimitsByLimitId": map[string]any{"codex": snapshot}, "rateLimitResetCredits": nil, "accountId": "acct-fake"})
			case fakeUsageLimitReadHang:
				// Never answers: the broker's read times out.
			default:
				_ = encoder.Encode(map[string]any{"jsonrpc": "2.0", "id": request.ID, "error": map[string]any{"code": -32603, "message": fakeCanary}})
			}
		}
	}
}

// Failure scenarios of the fake app-server, selected by fakeScenarioEnv.
const (
	fakeScenarioEnv = "INTEGRATION_CORE_FAKE_CODEX_SCENARIO"
	// fakeCallsEnv names a file the fake appends each request's method and
	// params to, one per line.
	fakeCallsEnv = "INTEGRATION_CORE_FAKE_CODEX_CALLS"
	// fakeResetsAtEnv is the unix-seconds reset the fake reports.
	fakeResetsAtEnv = "INTEGRATION_CORE_FAKE_CODEX_RESETS_AT"

	fakeUsageLimitNotified  = "usage_limit_notified"
	fakeUsageLimitRead      = "usage_limit_read"
	fakeUsageLimitReadError = "usage_limit_read_error"
	fakeUsageLimitReadHang  = "usage_limit_read_hang"
	fakeUnauthorized        = "unauthorized"
	// fakeRetriedForbidden retries a 403 twice, as Codex 0.153.4 retries any
	// unexpected status, then fails the turn as `other`.
	fakeRetriedForbidden = "retried_forbidden"

	// fakeCanary is upstream text shaped like Codex's usage-limit message; it
	// must never reach an error string or a log line.
	fakeCanary = "Try again at 3:05 PM CANARY"
)

func recordFakeCall(line []byte) {
	path := os.Getenv(fakeCallsEnv)
	if path == "" {
		return
	}
	var request struct {
		ID     json.RawMessage `json:"id"`
		Method string          `json:"method"`
		Params json.RawMessage `json:"params"`
	}
	if json.Unmarshal(line, &request) != nil || len(request.ID) == 0 {
		return
	}
	file, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600)
	if err != nil {
		return
	}
	defer file.Close()
	_, _ = file.WriteString(request.Method + " " + string(request.Params) + "\n")
}

// fakeRateLimitSnapshot is a pinned RateLimitSnapshot whose primary window is
// usedPercent full and resets at fakeResetsAtEnv; the secondary window is
// not exhausted and resets later. planType is present and must never be read.
func fakeRateLimitSnapshot(usedPercent int) map[string]any {
	resetsAt, _ := strconv.ParseInt(os.Getenv(fakeResetsAtEnv), 10, 64)
	return map[string]any{
		"limitId": "codex", "limitName": nil,
		"primary":   map[string]any{"usedPercent": usedPercent, "windowDurationMins": 300, "resetsAt": resetsAt},
		"secondary": map[string]any{"usedPercent": 40, "windowDurationMins": 10080, "resetsAt": resetsAt + 86400},
		"credits":   nil, "individualLimit": nil, "spendControlReached": nil,
		"planType": "plus", "rateLimitReachedType": "rate_limit_reached",
	}
}

// failFakeTurn ends turn-1 failed the way Codex 0.153.4 does: an error
// notification (willRetry false), then turn/completed with status failed and
// the same TurnError, whose message and additionalDetails carry the canary.
func failFakeTurn(scenario string, notify func(string, any)) {
	info := "usageLimitExceeded"
	if scenario == fakeUnauthorized {
		info = "unauthorized"
	}
	if scenario == fakeUsageLimitNotified {
		notify("account/rateLimits/updated", map[string]any{"rateLimits": fakeRateLimitSnapshot(100)})
	}
	if scenario == fakeRetriedForbidden {
		info = "other"
		retried := map[string]any{"message": fakeCanary, "codexErrorInfo": map[string]any{"responseStreamDisconnected": map[string]any{"httpStatusCode": 403}}, "additionalDetails": fakeCanary}
		for range 2 {
			notify("error", map[string]any{"error": retried, "willRetry": true, "threadId": "thread-1", "turnId": "turn-1"})
		}
	}
	turnError := map[string]any{"message": fakeCanary, "codexErrorInfo": info, "additionalDetails": fakeCanary}
	notify("error", map[string]any{"error": turnError, "willRetry": false, "threadId": "thread-1", "turnId": "turn-1"})
	notify("turn/completed", map[string]any{"threadId": "thread-1", "turn": map[string]any{"id": "turn-1", "status": "failed", "items": []any{}, "error": turnError}})
}

// fakeReportEnv makes the fake app-server answer with a JSON report of the
// environment it was spawned into instead of "Hei", and leave a stand-in
// prompt log in its CODEX_SQLITE_HOME.
const (
	fakeReportEnv      = "INTEGRATION_CORE_FAKE_CODEX_REPORT"
	fakeFreshMarkerEnv = "INTEGRATION_CORE_FAKE_CODEX_FRESH_MARKER"
)

type fakeAppServerReport struct {
	SQLiteHome  string          `json:"sqliteHome"`
	AuthFresh   bool            `json:"authFresh"`
	Args        []string        `json:"args"`
	ThreadStart fakeThreadStart `json:"threadStart"`
	// ThreadStartParams is the thread/start params exactly as received.
	ThreadStartParams json.RawMessage `json:"threadStartParams"`
	// Managed holds every CODEX_MANAGED_* variable the process was spawned
	// with, as KEY=value entries in environment order.
	Managed []string `json:"managed"`
}

// fakeThreadStart records the reuse properties thread/start and turn/start
// carried.
type fakeThreadStart struct {
	Effort          string `json:"effort"`
	ServiceTier     string `json:"serviceTier"`
	TurnEffort      string `json:"turnEffort"`
	TurnServiceTier string `json:"turnServiceTier"`
}

func fakeAppServerAnswer(threadStart fakeThreadStart, threadStartParams json.RawMessage) string {
	if os.Getenv(fakeReportEnv) != "1" {
		return "Hei"
	}
	sqliteHome := os.Getenv("CODEX_SQLITE_HOME")
	if sqliteHome != "" {
		_ = os.WriteFile(filepath.Join(sqliteHome, "logs_2.sqlite"), []byte("prompt log"), 0o600)
	}
	auth, _ := os.ReadFile(filepath.Join(os.Getenv("CODEX_HOME"), "auth.json"))
	marker := os.Getenv(fakeFreshMarkerEnv)
	var managed []string
	for _, entry := range os.Environ() {
		if strings.HasPrefix(strings.ToUpper(entry), "CODEX_MANAGED_") {
			managed = append(managed, entry)
		}
	}
	report, _ := json.Marshal(fakeAppServerReport{SQLiteHome: sqliteHome, AuthFresh: marker != "" && strings.Contains(string(auth), marker), Args: os.Args[1:], ThreadStart: threadStart, ThreadStartParams: threadStartParams, Managed: managed})
	return string(report)
}

func TestAppServerRefreshesBeforeSpawn(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatalf("locate test binary: %v", err)
	}
	marker := sentinel("fresh")
	t.Setenv(fakeAppServerEnv, "1")
	t.Setenv(fakeReportEnv, "1")
	t.Setenv(fakeFreshMarkerEnv, marker)
	store, auth, home, now := storeHarness(t, func(int, url.Values) (int, string) {
		return http.StatusOK, grantBody(rotatedAccess(time.Now(), marker), "", "")
	})
	writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
	runner := NewProcessRunner(executable, WithCredentials(store))
	request := InvokeRequest{RequestID: "req-1", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "hello"}}, ReasoningEffort: "low"}
	response, err := runner.Invoke(t.Context(), home, request)
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	if auth.callCount() != 1 {
		t.Fatalf("token calls = %d, want 1 before spawn", auth.callCount())
	}
	var report fakeAppServerReport
	if err := json.Unmarshal([]byte(response.Content), &report); err != nil {
		t.Fatalf("decode report %q: %v", response.Content, err)
	}
	if !report.AuthFresh {
		t.Fatal("the spawned app-server did not see the refreshed auth.json")
	}

	// A permanent refresh failure spawns nothing.
	store, _, home, now = storeHarness(t, func(int, url.Values) (int, string) {
		return http.StatusBadRequest, `{"error":"invalid_grant"}`
	})
	writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
	runner = NewProcessRunner(executable, WithCredentials(store))
	if _, err := runner.Invoke(t.Context(), home, request); !errors.Is(err, ErrReauthenticationRequired) {
		t.Fatalf("error = %v, want ErrReauthenticationRequired", err)
	}
	if _, err := os.Stat(filepath.Join(home, ephemeralDirName)); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("an app-server was spawned although reauthentication is required")
	}
}

func TestEphemeralSQLiteHome(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatalf("locate test binary: %v", err)
	}
	t.Setenv(fakeAppServerEnv, "1")
	t.Setenv(fakeReportEnv, "1")
	t.Setenv("CODEX_SQLITE_HOME", filepath.Join(t.TempDir(), "shared-state"))
	home := t.TempDir()
	if err := os.WriteFile(filepath.Join(home, "auth.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	runner := NewProcessRunner(executable)
	request := InvokeRequest{RequestID: "req-1", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "hello"}}, ReasoningEffort: "low"}
	response, err := runner.Invoke(t.Context(), home, request)
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	var report fakeAppServerReport
	if err := json.Unmarshal([]byte(response.Content), &report); err != nil {
		t.Fatalf("decode report: %v", err)
	}
	parent := filepath.Join(home, ephemeralDirName)
	if filepath.Dir(report.SQLiteHome) != parent {
		t.Fatalf("CODEX_SQLITE_HOME = %q, want a directory below %q", report.SQLiteHome, parent)
	}
	if _, err := os.Stat(report.SQLiteHome); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("the ephemeral SQLite home survived Close")
	}
	if _, err := os.Stat(filepath.Join(home, "logs_2.sqlite")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("a prompt log landed in the connection home")
	}
}

func TestProcessRunnerReportsUsageAndBrokerPhases(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatalf("locate test binary: %v", err)
	}
	t.Setenv(fakeAppServerEnv, "1")
	home := t.TempDir()
	if err := os.WriteFile(filepath.Join(home, "auth.json"), []byte("{}"), 0o600); err != nil {
		t.Fatalf("write auth.json: %v", err)
	}
	runner := NewProcessRunner(executable)
	request := InvokeRequest{
		RequestID:       "req-1",
		Model:           "gpt-5.6-terra",
		Messages:        []ChatMessage{{Role: "user", Content: "hello"}},
		ReasoningEffort: "low",
	}
	wantUsage := &TokenUsage{InputTokens: 1800, CachedInputTokens: 1024, CacheWriteInputTokens: 256, OutputTokens: 120, ReasoningOutputTokens: 40}
	wantPhases := []string{PhaseSpawn, PhaseInitialize, PhaseThreadStart, PhaseTurnStart, PhaseFirstDelta, PhaseTurnTotal}

	for _, streamed := range []bool{false, true} {
		var response InvokeResponse
		var deltas strings.Builder
		if streamed {
			response, err = runner.InvokeStream(t.Context(), home, request, func(delta string) error {
				deltas.WriteString(delta)
				return nil
			})
		} else {
			response, err = runner.Invoke(t.Context(), home, request)
		}
		if err != nil {
			t.Fatalf("streamed=%v: %v", streamed, err)
		}
		if response.Content != "Hei" || response.ModelUsed != "gpt-5.6-terra" || (streamed && deltas.String() != "Hei") {
			t.Fatalf("streamed=%v: response = %+v, deltas = %q", streamed, response, deltas.String())
		}
		if !reflect.DeepEqual(response.Usage, wantUsage) {
			t.Fatalf("streamed=%v: usage = %+v, want %+v", streamed, response.Usage, wantUsage)
		}
		if len(response.PhasesMs) != len(wantPhases) {
			t.Fatalf("streamed=%v: phases = %v, want exactly %v", streamed, response.PhasesMs, wantPhases)
		}
		for _, phase := range wantPhases {
			if ms, ok := response.PhasesMs[phase]; !ok || ms < 0 {
				t.Fatalf("streamed=%v: phase %s = %d (present %v)", streamed, phase, ms, ok)
			}
		}
		if response.PhasesMs[PhaseFirstDelta] > response.PhasesMs[PhaseTurnTotal] {
			t.Fatalf("streamed=%v: first delta %dms after the turn's end %dms", streamed, response.PhasesMs[PhaseFirstDelta], response.PhasesMs[PhaseTurnTotal])
		}
	}
}

// TestAppServerThreadEffortSwitchOnTheWire drives a real stdio app-server with
// CODEX_APP_SERVER_THREAD_EFFORT on and off. On, thread/start carries the
// turn's effort; off, it receives byte for byte the params sent before the
// override existed, the turn keeps its effort, and the expected effort
// difference is not logged.
func TestAppServerThreadEffortSwitchOnTheWire(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatalf("locate test binary: %v", err)
	}
	t.Setenv(fakeAppServerEnv, "1")
	t.Setenv(fakeReportEnv, "1")
	home := t.TempDir()
	if err := os.WriteFile(filepath.Join(home, "auth.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	request := InvokeRequest{RequestID: "req-1", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "hello"}}, ReasoningEffort: "high", ServiceTier: "priority"}
	workspace := filepath.Join(home, "workspace")
	for _, enabled := range []bool{true, false} {
		var logs strings.Builder
		response, err := NewProcessRunner(executable, WithThreadEffort(enabled), WithLogger(zerolog.New(&logs))).Invoke(t.Context(), home, request)
		if err != nil {
			t.Fatalf("enabled=%v: Invoke: %v", enabled, err)
		}
		var report fakeAppServerReport
		if err := json.Unmarshal([]byte(response.Content), &report); err != nil {
			t.Fatalf("enabled=%v: decode report %q: %v", enabled, response.Content, err)
		}
		want := fakeThreadStart{Effort: "high", ServiceTier: "priority", TurnEffort: "high", TurnServiceTier: "priority"}
		wantParams := mustMarshal(t, threadStartParams(workspace, request, true))
		if !enabled {
			want.Effort = ""
			wantParams = mustMarshal(t, preThreadEffortStartParams(workspace, request))
		}
		if report.ThreadStart != want {
			t.Fatalf("enabled=%v: reuse properties = %+v, want %+v", enabled, report.ThreadStart, want)
		}
		if string(report.ThreadStartParams) != string(wantParams) {
			t.Fatalf("enabled=%v: thread/start params on the wire = %s, want %s", enabled, report.ThreadStartParams, wantParams)
		}
		if strings.Contains(logs.String(), "thread_effort_differs") {
			t.Fatalf("enabled=%v: logged an effort difference: %s", enabled, logs.String())
		}
	}
}
