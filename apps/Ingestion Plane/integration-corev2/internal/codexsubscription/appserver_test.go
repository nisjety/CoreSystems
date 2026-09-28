package codexsubscription

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/rs/zerolog"
)

func TestCodexAppServerArgsHardened(t *testing.T) {
	want := []string{
		"--strict-config",
		"-c", `cli_auth_credentials_store="file"`,
		"-c", "features.plugins=false",
		"-c", "features.remote_plugin=false",
		"-c", "features.apps=false",
		"-c", "features.hooks=false",
		"-c", "features.tool_suggest=false",
		"-c", "features.skill_search=false",
		"-c", "features.skill_mcp_dependency_install=false",
		"-c", "features.goals=false",
		"-c", "features.multi_agent=false",
		"-c", "features.image_generation=false",
		"-c", "features.shell_snapshot=false",
		"-c", "features.memories=false",
		"-c", "features.shell_tool=false",
		"-c", "features.unified_exec=false",
		"-c", "apps._default.enabled=false",
		"-c", "include_apps_instructions=false",
		"-c", "skills.bundled.enabled=false",
		"-c", "skills.include_instructions=false",
		"-c", "analytics.enabled=false",
		"-c", "feedback.enabled=false",
		"-c", `history.persistence="none"`,
		"-c", "check_for_update_on_startup=false",
		"-c", `web_search="disabled"`,
		"app-server",
	}
	if got := codexAppServerArgs(true); !reflect.DeepEqual(got, want) {
		t.Fatalf("codexAppServerArgs(true) = %q, want %q", got, want)
	}
	// The escape hatch restores exactly the pre-hardening list, which still
	// forces the file credential store.
	legacy := []string{"app-server", "-c", `cli_auth_credentials_store="file"`}
	if got := codexAppServerArgs(false); !reflect.DeepEqual(got, legacy) {
		t.Fatalf("codexAppServerArgs(false) = %q, want %q", got, legacy)
	}
	if !NewProcessRunner("codex").hardened {
		t.Fatal("a process runner must be hardened by default")
	}
	if NewProcessRunner("codex", WithHardening(false)).hardened {
		t.Fatal("WithHardening(false) was ignored")
	}
}

func TestCodexEnvironmentScopesHomeAndSQLiteHome(t *testing.T) {
	t.Setenv("CODEX_HOME", "/somewhere/else")
	t.Setenv("CODEX_SQLITE_HOME", "/shared/state")
	environment := codexEnvironment("/conn/home", "/conn/home/.ephemeral/x", "")
	var homes, sqliteHomes []string
	for _, entry := range environment {
		switch {
		case strings.HasPrefix(entry, "CODEX_HOME="):
			homes = append(homes, entry)
		case strings.HasPrefix(entry, "CODEX_SQLITE_HOME="):
			sqliteHomes = append(sqliteHomes, entry)
		}
	}
	if !reflect.DeepEqual(homes, []string{"CODEX_HOME=/conn/home"}) || !reflect.DeepEqual(sqliteHomes, []string{"CODEX_SQLITE_HOME=/conn/home/.ephemeral/x"}) {
		t.Fatalf("CODEX_HOME=%v CODEX_SQLITE_HOME=%v", homes, sqliteHomes)
	}
}

// A model-driven Codex process, and anything it starts, must not inherit the
// broker's own credentials.
func TestCodexEnvironmentDropsBrokerSecrets(t *testing.T) {
	secrets := []string{
		"DATABASE_URL", "INTERNAL_API_KEY", "INTERNAL_API_KEY_HEADER", "OPENAI_API_KEY", "CODEX_API_KEY",
		"INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", "CODEX_SUBSCRIPTION_MODEL_PLANE_API_KEY",
		"AUTH_CORE_OAUTH_SERVICE_TOKEN", "GOOGLE_CLIENT_SECRET", "NATS_PASSWORD", "SLACK_TOKEN",
		"GOOGLE_INTEGRATION_KEY", "SCIM_ORG_BEARER_TOKENS", "X_ACCESS_TOKKEN", "verevon_test_secret",
	}
	values := map[string]string{}
	for _, name := range secrets {
		values[name] = sentinel(name)
		t.Setenv(name, values[name])
	}
	kept := map[string]string{
		"PATH": "/usr/local/bin:/usr/bin", "HOME": "/home/app", "TZ": "UTC", "LANG": "C.UTF-8",
		"SSL_CERT_FILE": "/etc/ssl/certs/ca-certificates.crt", "HTTPS_PROXY": "http://proxy.invalid:3128",
		"CODEX_SUBSCRIPTION_TRANSPORT": "app-server",
	}
	for name, value := range kept {
		t.Setenv(name, value)
	}
	environment := codexEnvironment("/conn/home", "/conn/home/.ephemeral/x", "/usr/local/lib/node_modules/@openai/codex")
	joined := strings.Join(environment, "\n")
	for name, value := range values {
		if strings.Contains(joined, value) {
			t.Fatalf("%s reached the Codex environment", name)
		}
	}
	for name, value := range kept {
		if !slices.Contains(environment, name+"="+value) {
			t.Fatalf("%s was dropped from the Codex environment", name)
		}
	}
	for _, set := range []string{"CODEX_HOME=/conn/home", "CODEX_SQLITE_HOME=/conn/home/.ephemeral/x", codexManagedByNPMEnv + "=1"} {
		if !slices.Contains(environment, set) {
			t.Fatalf("%s is missing from the Codex environment", set)
		}
	}
}

func TestCodexAppServerUsesProtocolSandboxValue(t *testing.T) {
	if codexReadOnlySandbox != "read-only" {
		t.Fatalf("codexReadOnlySandbox = %q, want read-only", codexReadOnlySandbox)
	}
}

func TestThreadStartUsesMinimalEphemeralChatContract(t *testing.T) {
	params := threadStartParams("/isolated/workspace", InvokeRequest{
		Model:       "gpt-6-astra",
		ServiceTier: "priority",
	}, true)
	if params["baseInstructions"] != codexBaseInstructions {
		t.Fatalf("base instructions = %q", params["baseInstructions"])
	}
	if params["ephemeral"] != true || params["sandbox"] != codexReadOnlySandbox {
		t.Fatalf("thread params = %#v", params)
	}
	if params["serviceTier"] != "priority" {
		t.Fatalf("service tier = %q, want priority", params["serviceTier"])
	}
}

// TestThreadStartCarriesTheTurnsReuseProperties pins that thread/start gets
// the model, service tier and reasoning effort turn/start uses, so Codex's
// startup-prewarm request can match the turn's first request.
func TestThreadStartCarriesTheTurnsReuseProperties(t *testing.T) {
	for _, effort := range []string{"low", "high"} {
		for _, tier := range []string{"", "priority"} {
			request := InvokeRequest{Model: "gpt-5.6-terra", ReasoningEffort: effort, ServiceTier: tier, Messages: []ChatMessage{{Role: "user", Content: "hello"}}}
			thread := threadStartParams("/isolated/workspace", request, true)
			turn := turnStartParams("thread-1", request)
			config := thread["config"].(map[string]any)
			if config["model_reasoning_effort"] != turn["effort"] || turn["effort"] != effort {
				t.Fatalf("effort %q tier %q: thread effort %v, turn effort %v", effort, tier, config["model_reasoning_effort"], turn["effort"])
			}
			threadTier, threadHasTier := thread["serviceTier"]
			turnTier, turnHasTier := turn["serviceTier"]
			if threadHasTier != turnHasTier || threadTier != turnTier || threadHasTier != (tier != "") {
				t.Fatalf("effort %q tier %q: thread tier %v (%v), turn tier %v (%v)", effort, tier, threadTier, threadHasTier, turnTier, turnHasTier)
			}
			if thread["model"] != request.Model {
				t.Fatalf("thread model = %v", thread["model"])
			}
			// The execution guards stay in the same config map.
			if config["features.shell_tool"] != false || config["features.unified_exec"] != false || config["web_search"] != "disabled" || config["forced_login_method"] != "chatgpt" {
				t.Fatalf("thread config lost its guards: %#v", config)
			}
		}
	}
	// No effort requested (the manager normalizes to low before this point):
	// the thread keeps Codex's own default rather than an empty override.
	config := threadStartParams("/isolated/workspace", InvokeRequest{Model: "gpt-5.6-terra"}, true)["config"].(map[string]any)
	if _, present := config["model_reasoning_effort"]; present {
		t.Fatalf("empty effort produced an override: %#v", config)
	}
}

// preThreadEffortStartParams is threadStartParams as it was before thread/start
// carried the reasoning effort, copied verbatim from that release. It is what
// CODEX_APP_SERVER_THREAD_EFFORT=false must send.
func preThreadEffortStartParams(workspace string, request InvokeRequest) map[string]any {
	params := map[string]any{
		"cwd":              workspace,
		"model":            request.Model,
		"modelProvider":    "openai",
		"approvalPolicy":   "never",
		"sandbox":          codexReadOnlySandbox,
		"serviceName":      "coresystem_integration_core",
		"baseInstructions": codexBaseInstructions,
		"ephemeral":        true,
		// Tool proposals are JSON data for Model Gateway. The broker must not
		// independently execute a shell or perform web searches for that data.
		"config": map[string]any{"features.shell_tool": false, "features.unified_exec": false, "web_search": "disabled", "forced_login_method": "chatgpt"},
	}
	if request.ServiceTier != "" {
		params["serviceTier"] = request.ServiceTier
	}
	return params
}

func mustMarshal(t *testing.T, value any) []byte {
	t.Helper()
	encoded, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return encoded
}

// TestThreadStartEffortSwitch pins both settings of
// CODEX_APP_SERVER_THREAD_EFFORT on the encoded thread/start params: on adds
// only config.model_reasoning_effort, off is byte-identical to the params
// before the override existed, for every effort, tier and output schema.
func TestThreadStartEffortSwitch(t *testing.T) {
	if !NewProcessRunner("codex").threadEffort {
		t.Fatal("a process runner must send the thread effort by default")
	}
	if NewProcessRunner("codex", WithThreadEffort(false)).threadEffort {
		t.Fatal("WithThreadEffort(false) was ignored")
	}
	schema := json.RawMessage(`{"type":"object","properties":{"content":{"type":"string"}},"required":["content"],"additionalProperties":false}`)
	for _, effort := range []string{"", "low", "high"} {
		for _, tier := range []string{"", "priority"} {
			for _, outputSchema := range []json.RawMessage{nil, schema} {
				request := InvokeRequest{RequestID: "req-1", Model: "gpt-5.6-terra", ReasoningEffort: effort, ServiceTier: tier, OutputSchema: outputSchema, Messages: []ChatMessage{{Role: "user", Content: "hello"}}}
				legacy := mustMarshal(t, preThreadEffortStartParams("/isolated/workspace", request))
				if off := mustMarshal(t, threadStartParams("/isolated/workspace", request, false)); string(off) != string(legacy) {
					t.Fatalf("effort %q tier %q: off = %s, want the earlier params %s", effort, tier, off, legacy)
				}
				on := threadStartParams("/isolated/workspace", request, true)
				config := on["config"].(map[string]any)
				got, present := config["model_reasoning_effort"]
				if present != (effort != "") || (present && got != effort) {
					t.Fatalf("effort %q tier %q: on config = %#v", effort, tier, config)
				}
				// Removing the override from the on params leaves exactly the
				// earlier params: the switch changes nothing else.
				delete(config, "model_reasoning_effort")
				if stripped := mustMarshal(t, on); string(stripped) != string(legacy) {
					t.Fatalf("effort %q tier %q: on differs beyond the effort: %s, want %s", effort, tier, stripped, legacy)
				}
			}
		}
	}
	// A fixed encoding, so the pre-override copy above cannot drift with the
	// code it guards.
	request := InvokeRequest{Model: "gpt-5.6-terra", ReasoningEffort: "low", ServiceTier: "priority"}
	prefix := `{"approvalPolicy":"never","baseInstructions":` + string(mustMarshal(t, codexBaseInstructions)) + `,"config":{"features.shell_tool":false,"features.unified_exec":false,"forced_login_method":"chatgpt",`
	suffix := `"web_search":"disabled"},"cwd":"/isolated/workspace","ephemeral":true,"model":"gpt-5.6-terra","modelProvider":"openai","sandbox":"read-only","serviceName":"coresystem_integration_core","serviceTier":"priority"}`
	if got, want := string(mustMarshal(t, threadStartParams("/isolated/workspace", request, false))), prefix+suffix; got != want {
		t.Fatalf("off = %s\nwant  %s", got, want)
	}
	if got, want := string(mustMarshal(t, threadStartParams("/isolated/workspace", request, true))), prefix+`"model_reasoning_effort":"low",`+suffix; got != want {
		t.Fatalf("on = %s\nwant %s", got, want)
	}
}

func TestCheckThreadEffortLogsOnlyAMismatch(t *testing.T) {
	var logs strings.Builder
	runner := NewProcessRunner("codex", WithLogger(zerolog.New(&logs)))
	high, medium, odd := "high", "medium", "Weird Value"
	runner.checkThreadEffort("high", &high)
	runner.checkThreadEffort("", nil)
	if logs.Len() != 0 {
		t.Fatalf("matching effort logged: %s", logs.String())
	}
	runner.checkThreadEffort("high", &medium)
	runner.checkThreadEffort("low", nil)
	runner.checkThreadEffort("low", &odd)
	lines := strings.Split(strings.TrimSpace(logs.String()), "\n")
	if len(lines) != 3 {
		t.Fatalf("log lines = %q", lines)
	}
	for index, want := range []string{`"thread_effort":"medium"`, `"thread_effort":"none"`, `"thread_effort":"unrecognized"`} {
		if !strings.Contains(lines[index], `"event":"thread_effort_differs"`) || !strings.Contains(lines[index], want) {
			t.Fatalf("line %d = %s, want %s", index, lines[index], want)
		}
	}
	if strings.Contains(logs.String(), odd) {
		t.Fatal("an unrecognized upstream effort reached the log")
	}
}

func TestCodexAccountCallsUseObjectParams(t *testing.T) {
	encoded, err := json.Marshal(codexEmptyObjectParams())
	if err != nil {
		t.Fatalf("marshal empty Codex params: %v", err)
	}
	if string(encoded) != "{}" {
		t.Fatalf("empty Codex params = %s, want {}", encoded)
	}
}

func TestTurnStartParamsIncludesOptionalPriorityTier(t *testing.T) {
	request := InvokeRequest{
		ReasoningEffort: "low",
		ServiceTier:     "priority",
		Messages:        []ChatMessage{{Role: "user", Content: "hello"}},
	}
	params := turnStartParams("thread-1", request)
	if params["effort"] != "low" || params["serviceTier"] != "priority" {
		t.Fatalf("turn params = %#v", params)
	}

	request.ServiceTier = ""
	params = turnStartParams("thread-1", request)
	if _, exists := params["serviceTier"]; exists {
		t.Fatalf("standard turn unexpectedly included serviceTier: %#v", params)
	}
}

func TestTurnStartForwardsStructuredOutputAsData(t *testing.T) {
	schema := json.RawMessage(`{"type":"object","properties":{"content":{"type":"string"}},"required":["content"],"additionalProperties":false}`)
	request := InvokeRequest{Model: "gpt-5.6-terra", OutputSchema: schema}
	encoded, err := json.Marshal(turnStartParams("thread", request))
	if err != nil {
		t.Fatal(err)
	}
	var params map[string]any
	if err := json.Unmarshal(encoded, &params); err != nil {
		t.Fatal(err)
	}
	if params["outputSchema"].(map[string]any)["type"] != "object" {
		t.Fatal("schema was not forwarded as a JSON object")
	}
	if _, present := turnStartParams("thread", InvokeRequest{})["outputSchema"]; present {
		t.Fatal("plain text should omit outputSchema")
	}
	thread := threadStartParams("/isolated/workspace", request, true)
	config := thread["config"].(map[string]any)
	if config["features.shell_tool"] != false || config["features.unified_exec"] != false || config["web_search"] != "disabled" || config["forced_login_method"] != "chatgpt" {
		t.Fatal("broker native execution must be disabled")
	}
	if thread["model"] != "gpt-5.6-terra" || thread["sandbox"] != "read-only" || thread["approvalPolicy"] != "never" || thread["ephemeral"] != true {
		t.Fatal("structured output changed model or execution policy")
	}
}

func TestServingModelMustMatchBeforeStartingATurn(t *testing.T) {
	for _, item := range [][2]string{{"", "openai"}, {"gpt-5.6-sol", "openai"}, {"gpt-5.6-terra", "azure"}, {"gpt-5.6-terra", ""}} {
		if validateServingModel("gpt-5.6-terra", item[0], item[1]) == nil {
			t.Fatal("changed or missing route accepted")
		}
	}
	if err := validateServingModel("gpt-5.6-terra", "gpt-5.6-terra", "openai"); err != nil {
		t.Fatal(err)
	}
}

func TestPersistedAuthReadyRequiresNonEmptyAuthJSON(t *testing.T) {
	home := t.TempDir()
	ready, err := persistedAuthReady(home)
	if err != nil || ready {
		t.Fatalf("missing auth.json: ready=%v err=%v, want false and nil", ready, err)
	}
	if err := os.WriteFile(filepath.Join(home, "auth.json"), nil, 0o600); err != nil {
		t.Fatalf("write empty auth.json: %v", err)
	}
	ready, err = persistedAuthReady(home)
	if err != nil || ready {
		t.Fatalf("empty auth.json: ready=%v err=%v, want false and nil", ready, err)
	}
	if err := os.WriteFile(filepath.Join(home, "auth.json"), []byte("{}"), 0o600); err != nil {
		t.Fatalf("write auth.json: %v", err)
	}
	ready, err = persistedAuthReady(home)
	if err != nil || !ready {
		t.Fatalf("non-empty auth.json: ready=%v err=%v, want true and nil", ready, err)
	}
}

func TestProcessRunnerInvokeRejectsMissingPersistedAuthentication(t *testing.T) {
	runner := NewProcessRunner("command-that-must-not-run")
	_, err := runner.Invoke(t.Context(), t.TempDir(), InvokeRequest{
		ConnectionID:    "conn-1",
		Model:           "gpt-5",
		Messages:        []ChatMessage{{Role: "user", Content: "hello"}},
		ReasoningEffort: "low",
	})
	if !errors.Is(err, ErrReauthenticationRequired) {
		t.Fatalf("Invoke error = %v, want ErrReauthenticationRequired", err)
	}
}

func TestProcessRunnerLogoutIsIdempotentWhenAuthenticationIsMissing(t *testing.T) {
	runner := NewProcessRunner("command-that-must-not-run")
	if err := runner.Logout(t.Context(), t.TempDir()); err != nil {
		t.Fatalf("Logout: %v", err)
	}
}

// fakeTurnClient is an app-server client whose notifications come from the
// test rather than from a process; waitForTurn reads only events and done.
func fakeTurnClient(notifications ...rpcNotification) *appServerClient {
	events := make(chan rpcNotification, len(notifications))
	for _, notification := range notifications {
		events <- notification
	}
	return &appServerClient{events: events, done: make(chan struct{})}
}

func agentDelta(threadID, turnID, delta string) rpcNotification {
	return rpcNotification{Method: "item/agentMessage/delta", Params: json.RawMessage(`{"threadId":"` + threadID + `","turnId":"` + turnID + `","itemId":"item-1","delta":"` + delta + `"}`)}
}

// tokenUsageUpdated is the Codex 0.153.4 thread/tokenUsage/updated shape:
// `total` is the thread's running total, `last` the latest model response.
func tokenUsageUpdated(threadID, turnID, total string) rpcNotification {
	return rpcNotification{Method: "thread/tokenUsage/updated", Params: json.RawMessage(`{"threadId":"` + threadID + `","turnId":"` + turnID + `","tokenUsage":{"total":` + total + `,"last":{"inputTokens":1,"cachedInputTokens":0,"outputTokens":1,"reasoningOutputTokens":0,"totalTokens":2},"modelContextWindow":null}}`)}
}

func turnCompleted(threadID, turnID string) rpcNotification {
	return rpcNotification{Method: "turn/completed", Params: json.RawMessage(`{"threadId":"` + threadID + `","turn":{"id":"` + turnID + `","status":"completed","items":[]}}`)}
}

func TestWaitForTurnKeepsTheLatestUsageTotalOfItsOwnTurn(t *testing.T) {
	client := fakeTurnClient(
		agentDelta("thread-1", "turn-1", "Hei "),
		// cacheWriteInputTokens is optional in the schema (default 0).
		tokenUsageUpdated("thread-1", "turn-1", `{"inputTokens":900,"cachedInputTokens":512,"outputTokens":60,"reasoningOutputTokens":20,"totalTokens":960}`),
		tokenUsageUpdated("thread-1", "turn-2", `{"inputTokens":99999,"cachedInputTokens":0,"outputTokens":99999,"reasoningOutputTokens":0,"totalTokens":199998}`),
		agentDelta("thread-1", "turn-1", "verden"),
		tokenUsageUpdated("thread-1", "turn-1", `{"inputTokens":1800,"cachedInputTokens":1024,"cacheWriteInputTokens":256,"outputTokens":120,"reasoningOutputTokens":40,"totalTokens":1920}`),
		turnCompleted("thread-1", "turn-1"),
	)
	var streamed strings.Builder
	result, err := client.waitForTurn(t.Context(), "thread-1", "turn-1", func(delta string) error {
		streamed.WriteString(delta)
		return nil
	})
	if err != nil {
		t.Fatalf("waitForTurn: %v", err)
	}
	if result.content != "Hei verden" || streamed.String() != "Hei verden" {
		t.Fatalf("content = %q, streamed = %q", result.content, streamed.String())
	}
	want := &TokenUsage{InputTokens: 1800, CachedInputTokens: 1024, CacheWriteInputTokens: 256, OutputTokens: 120, ReasoningOutputTokens: 40}
	if !reflect.DeepEqual(result.usage, want) {
		t.Fatalf("usage = %+v, want the latest total of this turn %+v", result.usage, want)
	}
	if result.firstText.IsZero() {
		t.Fatal("the first answer text was not timed")
	}
}

func TestWaitForTurnWithoutUsageLeavesItUnknown(t *testing.T) {
	// A release that streams only completion items still yields text, and a
	// turn whose usage never arrived reports none rather than zero.
	client := fakeTurnClient(
		rpcNotification{Method: "item/completed", Params: json.RawMessage(`{"threadId":"thread-1","turnId":"turn-1","item":{"type":"agentMessage","id":"item-1","text":"Svar"}}`)},
		turnCompleted("thread-1", "turn-1"),
	)
	result, err := client.waitForTurn(t.Context(), "thread-1", "turn-1", nil)
	if err != nil {
		t.Fatalf("waitForTurn: %v", err)
	}
	if result.content != "Svar" || result.usage != nil || result.firstText.IsZero() {
		t.Fatalf("result = %+v, want the item text, nil usage and a first-text time", result)
	}
}

func TestWaitForTurnIgnoresUsageOfAnotherTurnOrThread(t *testing.T) {
	client := fakeTurnClient(
		tokenUsageUpdated("thread-1", "turn-2", `{"inputTokens":10,"cachedInputTokens":0,"outputTokens":5,"reasoningOutputTokens":0,"totalTokens":15}`),
		tokenUsageUpdated("thread-2", "turn-1", `{"inputTokens":10,"cachedInputTokens":0,"outputTokens":5,"reasoningOutputTokens":0,"totalTokens":15}`),
		agentDelta("thread-1", "turn-1", "ok"),
		turnCompleted("thread-1", "turn-1"),
	)
	result, err := client.waitForTurn(t.Context(), "thread-1", "turn-1", nil)
	if err != nil {
		t.Fatalf("waitForTurn: %v", err)
	}
	if result.usage != nil {
		t.Fatalf("usage = %+v, want nil: every report belonged to another turn", result.usage)
	}
}

func TestInvokeResponseCarriesUsageAndPhasesOnlyWhenReported(t *testing.T) {
	encoded, err := json.Marshal(InvokeResponse{
		RequestID: "req-1",
		Content:   "Hei",
		ModelUsed: "gpt-5.6-terra",
		Usage:     &TokenUsage{InputTokens: 1800, CachedInputTokens: 1024, CacheWriteInputTokens: 256, OutputTokens: 120, ReasoningOutputTokens: 40},
		PhasesMs:  map[string]int64{PhaseSpawn: 41, PhaseTurnTotal: 9000},
	})
	if err != nil {
		t.Fatal(err)
	}
	var decoded map[string]json.RawMessage
	if err := json.Unmarshal(encoded, &decoded); err != nil {
		t.Fatal(err)
	}
	if got := string(decoded["usage"]); got != `{"inputTokens":1800,"cachedInputTokens":1024,"cacheWriteInputTokens":256,"outputTokens":120,"reasoningOutputTokens":40}` {
		t.Fatalf("usage = %s", got)
	}
	if got := string(decoded["phasesMs"]); got != `{"spawn":41,"turn_total":9000}` {
		t.Fatalf("phasesMs = %s", got)
	}

	// Without a report the envelope keeps exactly its earlier shape, which is
	// what an older Model Plane build parses.
	encoded, err = json.Marshal(InvokeResponse{RequestID: "req-1", Content: "Hei", ModelUsed: "gpt-5.6-terra"})
	if err != nil {
		t.Fatal(err)
	}
	if string(encoded) != `{"requestId":"req-1","content":"Hei","modelUsed":"gpt-5.6-terra"}` {
		t.Fatalf("unreported response = %s", encoded)
	}
}
