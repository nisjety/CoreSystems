package codexsubscription

import (
	"bytes"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/google/uuid"
)

var userAgentPattern = regexp.MustCompile(`^verevon/\S+ \([a-z0-9]+; [a-z0-9]+\) integration-core$`)

func TestDirectHeadersExact(t *testing.T) {
	h := newDirectHarness(t, nil)
	freshAuth(t, h.home, h.now)
	for range 2 {
		if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err != nil {
			t.Fatalf("Invoke: %v", err)
		}
	}
	requests := h.upstream.recorded()
	if len(requests) != 2 {
		t.Fatalf("upstream requests = %d, want 2", len(requests))
	}
	first := requests[0]
	if first.method != http.MethodPost || first.path != "/backend-api/codex/responses" {
		t.Fatalf("request = %s %s", first.method, first.path)
	}
	header := first.header
	exact := map[string]string{
		"Originator":      "verevon",
		"Accept":          "text/event-stream",
		"Accept-Encoding": "identity",
		"Content-Type":    "application/json",
	}
	for key, want := range exact {
		if got := header.Get(key); got != want {
			t.Fatalf("%s = %q, want %q", key, got, want)
		}
	}
	if !strings.HasPrefix(header.Get("Authorization"), "Bearer ") {
		t.Fatal("Authorization is not a bearer token")
	}
	if !userAgentPattern.MatchString(header.Get("User-Agent")) {
		t.Fatalf("User-Agent = %q", header.Get("User-Agent"))
	}
	var body map[string]any
	if err := json.Unmarshal(first.body, &body); err != nil {
		t.Fatal(err)
	}
	if header.Get("Session-Id") == "" || header.Get("Session-Id") != body["prompt_cache_key"] {
		t.Fatalf("session-id %q must equal prompt_cache_key %v", header.Get("Session-Id"), body["prompt_cache_key"])
	}
	firstID, err := uuid.Parse(header.Get("X-Client-Request-Id"))
	if err != nil || firstID.Version() != 4 {
		t.Fatalf("x-client-request-id = %q is not a UUIDv4", header.Get("X-Client-Request-Id"))
	}
	if requests[1].header.Get("X-Client-Request-Id") == header.Get("X-Client-Request-Id") {
		t.Fatal("x-client-request-id repeated across calls")
	}
	for _, forbidden := range []string{"OpenAI-Beta", "Version", "Session_id", "Conversation_id", "Conversation-Id", "Cookie", "Content-Encoding", "OpenAI-Organization", "OpenAI-Project", "X-Oai-Attestation"} {
		if _, present := header[http.CanonicalHeaderKey(forbidden)]; present {
			t.Fatalf("forbidden header %s was sent", forbidden)
		}
	}
	for key := range header {
		if strings.HasPrefix(strings.ToLower(key), "x-codex-") {
			t.Fatalf("telemetry header %s was sent", key)
		}
	}
}

func TestDirectAccountAndOptionalHeaders(t *testing.T) {
	cases := []struct {
		name          string
		opts          authOptions
		shape         string
		model         string
		wantAccount   string
		wantResidency string
		wantFedRAMP   bool
		wantLite      bool
	}{
		{name: "file account wins", opts: authOptions{accountID: strPtr("acct-file"), idClaims: anyMap{openAIAuthClaim: anyMap{"chatgpt_account_id": "acct-id"}}}, wantAccount: "acct-file"},
		{name: "id token account", opts: authOptions{idClaims: anyMap{openAIAuthClaim: anyMap{"chatgpt_account_id": "acct-id"}}, accessClaims: anyMap{openAIAuthClaim: anyMap{"chatgpt_account_id": "acct-access"}}}, wantAccount: "acct-id"},
		{name: "access token account", opts: authOptions{accessClaims: anyMap{openAIAuthClaim: anyMap{"chatgpt_account_id": "acct-access"}}}, wantAccount: "acct-access"},
		{name: "unknown account", opts: authOptions{}},
		{name: "residency us", opts: authOptions{accessClaims: anyMap{openAIAuthClaim: anyMap{"chatgpt_compute_residency": "us"}}}, wantResidency: "us"},
		{name: "residency top-level fallback", opts: authOptions{accessClaims: anyMap{"chatgpt_compute_residency": "eu"}}, wantResidency: "eu"},
		{name: "residency no constraint", opts: authOptions{accessClaims: anyMap{openAIAuthClaim: anyMap{"chatgpt_compute_residency": "no_constraint"}}}},
		{name: "fedramp", opts: authOptions{idClaims: anyMap{openAIAuthClaim: anyMap{"chatgpt_account_is_fedramp": true}}}, wantFedRAMP: true},
		{name: "fedramp false", opts: authOptions{idClaims: anyMap{openAIAuthClaim: anyMap{"chatgpt_account_is_fedramp": false}}}},
		{name: "lite on lite model", shape: RequestShapeLite, wantLite: true},
		{name: "lite on classic-only model", shape: RequestShapeLite, model: "gpt-5.5"},
		{name: "classic never lite"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			model := tc.model
			if model == "" {
				model = "gpt-5.6-terra"
			}
			h := newDirectHarness(t, []fakeTurn{{frames: successFrames(model, "ok")}}, func(o *harnessOptions) { o.direct.RequestShape = tc.shape })
			opts := tc.opts
			opts.accessExp = h.now.Add(time.Hour)
			writeAuthJSON(t, h.home, opts)
			request := testRequest()
			request.Model = model
			if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
				t.Fatalf("Invoke: %v", err)
			}
			header := h.upstream.recorded()[0].header
			if got := header.Get("Chatgpt-Account-Id"); got != tc.wantAccount {
				t.Fatalf("ChatGPT-Account-ID = %q, want %q", got, tc.wantAccount)
			}
			if _, present := header["Chatgpt-Account-Id"]; present != (tc.wantAccount != "") {
				t.Fatalf("ChatGPT-Account-ID presence = %v", present)
			}
			if got := header.Get("X-Openai-Internal-Codex-Residency"); got != tc.wantResidency {
				t.Fatalf("residency = %q, want %q", got, tc.wantResidency)
			}
			if got := header.Get("X-Openai-Fedramp"); (got == "true") != tc.wantFedRAMP || (!tc.wantFedRAMP && got != "") {
				t.Fatalf("fedramp = %q, want %v", got, tc.wantFedRAMP)
			}
			if got := header.Get("X-Openai-Internal-Codex-Responses-Lite"); (got == "true") != tc.wantLite || (!tc.wantLite && got != "") {
				t.Fatalf("lite header = %q, want %v", got, tc.wantLite)
			}
		})
	}
}

func goldenRolesMessages() []ChatMessage {
	return []ChatMessage{
		{Role: "system", Content: "You are the Verevon reviewer."},
		{Role: " Developer ", Content: "Answer in English.", Name: "policy"},
		{Role: "user", Content: "Review this change."},
		{Role: "assistant", Content: "Which file?"},
		{Role: "", Content: "The config loader.", Name: "ima"},
		{Role: "user", Content: "   "},
		{Role: "tool", Content: "{\"ok\":true}", Name: "lookup"},
		{Role: "system", Content: "Return JSON tool proposals only."},
	}
}

func TestDirectBodyGoldenRoles(t *testing.T) {
	h := newDirectHarness(t, nil)
	freshAuth(t, h.home, h.now)
	request := testRequest()
	request.Messages = goldenRolesMessages()
	body, err := h.transport.requestBody(request, directModelProfiles["gpt-5.6-terra"], true, false, false, promptCacheKey(request.ConnectionID, request.Model))
	if err != nil {
		t.Fatal(err)
	}
	var pretty bytes.Buffer
	if err := json.Indent(&pretty, body, "", "  "); err != nil {
		t.Fatal(err)
	}
	pretty.WriteByte('\n')
	golden := filepath.Join("testdata", "direct_body_roles.golden.json")
	if os.Getenv("UPDATE_GOLDEN") == "1" {
		if err := os.WriteFile(golden, pretty.Bytes(), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	want, err := os.ReadFile(golden)
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	if got := pretty.String(); got != strings.ReplaceAll(string(want), "\r\n", "\n") {
		t.Fatalf("roles body differs from %s:\n%s", golden, got)
	}
	// The trailing protocol message stays last, as a developer item.
	var decoded directRequestBody
	if err := json.Unmarshal(body, &decoded); err != nil {
		t.Fatal(err)
	}
	last := decoded.Input[len(decoded.Input)-1]
	if last.Role != "developer" || last.Content[0].Text != "Return JSON tool proposals only." {
		t.Fatalf("last input item = %+v", last)
	}
	if !strings.HasPrefix(decoded.Instructions, codexBaseInstructions+"\n\n") {
		t.Fatal("instructions must start with the guard")
	}
}

func TestDirectBodyFlattenedParity(t *testing.T) {
	messages := goldenRolesMessages()
	instructions, input := mapPrompt(messages, PromptModeFlattened)
	if instructions != codexBaseInstructions {
		t.Fatalf("flattened instructions = %q", instructions)
	}
	if len(input) != 1 || input[0].Role != "user" || input[0].Content[0].Type != "input_text" || input[0].Content[0].Text != renderPrompt(messages) {
		t.Fatalf("flattened input = %+v", input)
	}
	// Roles mode falls back to flattened when every message is a system one.
	systemOnly := []ChatMessage{{Role: "system", Content: "a"}, {Role: "developer", Content: "b"}}
	instructions, input = mapPrompt(systemOnly, PromptModeRoles)
	if instructions != codexBaseInstructions || len(input) != 1 || input[0].Content[0].Text != renderPrompt(systemOnly) {
		t.Fatalf("system-only roles = %q %+v, want the flattened form", instructions, input)
	}
	// Leading system text is joined after the guard.
	instructions, _ = mapPrompt([]ChatMessage{{Role: "system", Content: "one"}, {Role: "system", Content: "two"}, {Role: "user", Content: "hi"}}, PromptModeRoles)
	if instructions != codexBaseInstructions+"\n\none\n\ntwo" {
		t.Fatalf("instructions = %q", instructions)
	}
}

func TestDirectBodyInvariants(t *testing.T) {
	h := newDirectHarness(t, nil)
	freshAuth(t, h.home, h.now)
	request := testRequest()
	request.MaxTokens = 4096
	if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
		t.Fatal(err)
	}
	body := h.upstream.lastBody(t)
	if body["store"] != false || body["stream"] != true {
		t.Fatalf("store/stream = %v/%v", body["store"], body["stream"])
	}
	if !reflect.DeepEqual(body["include"], []any{"reasoning.encrypted_content"}) {
		t.Fatalf("include = %v", body["include"])
	}
	for _, absent := range []string{"max_output_tokens", "temperature", "top_p", "previous_response_id", "tools", "tool_choice", "parallel_tool_calls", "client_metadata", "stream_options", "prompt_cache_retention", "service_tier"} {
		if _, present := body[absent]; present {
			t.Fatalf("%s must not be sent", absent)
		}
	}
	reasoning := body["reasoning"].(map[string]any)
	if _, present := reasoning["summary"]; present || reasoning["effort"] != "low" {
		t.Fatalf("reasoning = %v", reasoning)
	}
	if body["text"].(map[string]any)["verbosity"] != "low" {
		t.Fatalf("text = %v", body["text"])
	}
}

func TestDirectOutputSchema(t *testing.T) {
	h := newDirectHarness(t, nil)
	freshAuth(t, h.home, h.now)
	// The inference-core tool-proposal shape, key order and all.
	schema := `{ "type": "object", "properties": { "content": {"type":"string"}, "tool_calls": {"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"arguments":{"type":"string"}},"required":["name","arguments"],"additionalProperties":false}} }, "required": ["content","tool_calls"], "additionalProperties": false, "description": "a < b & c > d" }`
	request := testRequest()
	request.OutputSchema = json.RawMessage(schema)
	if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
		t.Fatal(err)
	}
	raw := h.upstream.recorded()[0].body
	var compact bytes.Buffer
	if err := json.Compact(&compact, []byte(schema)); err != nil {
		t.Fatal(err)
	}
	want := `"format":{"type":"json_schema","name":"codex_output_schema","strict":true,"schema":` + compact.String() + `}`
	if !bytes.Contains(raw, []byte(want)) {
		t.Fatalf("body %s lacks %s", raw, want)
	}
	// Plain text sends no format.
	request.OutputSchema = nil
	if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
		t.Fatal(err)
	}
	if text := h.upstream.lastBody(t)["text"].(map[string]any); text["format"] != nil {
		t.Fatalf("text = %v", text)
	}
}

func TestDirectEffortMapping(t *testing.T) {
	h := newDirectHarness(t, nil)
	freshAuth(t, h.home, h.now)
	manager, err := NewManager(Config{Enabled: true, Home: filepath.Dir(h.home)}, NewRunnerWithTransport(nil, h.transport, h.store))
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct{ in, want string }{{"low", "low"}, {"HIGH", "high"}, {"", "low"}} {
		request := testRequest()
		request.ReasoningEffort = tc.in
		if _, err := manager.Invoke(t.Context(), request); err != nil {
			t.Fatalf("effort %q: %v", tc.in, err)
		}
		if got := h.upstream.lastBody(t)["reasoning"].(map[string]any)["effort"]; got != tc.want {
			t.Fatalf("effort %q sent %v, want %s", tc.in, got, tc.want)
		}
	}
	before := h.upstream.count()
	request := testRequest()
	request.ReasoningEffort = "maximum"
	if _, err := manager.Invoke(t.Context(), request); !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("invalid effort error = %v", err)
	}
	if h.upstream.count() != before {
		t.Fatal("an invalid effort reached the upstream")
	}
}

func TestDirectServiceTier(t *testing.T) {
	for model := range directModelProfiles {
		h := newDirectHarness(t, []fakeTurn{{frames: successFrames(model, "ok")}})
		freshAuth(t, h.home, h.now)
		request := testRequest()
		request.Model = model
		request.ServiceTier = "priority"
		if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
			t.Fatalf("%s: %v", model, err)
		}
		if got := h.upstream.lastBody(t)["service_tier"]; got != "priority" {
			t.Fatalf("%s service_tier = %v", model, got)
		}
	}
	h := newDirectHarness(t, []fakeTurn{{frames: successFrames("gpt-unknown", "ok")}})
	freshAuth(t, h.home, h.now)
	request := testRequest()
	request.Model = "gpt-unknown"
	request.ServiceTier = "priority"
	if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
		t.Fatal(err)
	}
	body := h.upstream.lastBody(t)
	if _, present := body["service_tier"]; present {
		t.Fatal("unknown model must not send a service tier")
	}
	if _, present := body["text"]; present {
		t.Fatal("unknown model must not send verbosity")
	}
	if !strings.Contains(h.logs.String(), `"service_tier_dropped":true`) {
		t.Fatalf("dropped tier not logged: %s", h.logs.String())
	}
	request.ServiceTier = ""
	if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
		t.Fatal(err)
	}
	if _, present := h.upstream.lastBody(t)["service_tier"]; present {
		t.Fatal("service tier sent although not requested")
	}
}

func TestDirectServiceTierDegrade(t *testing.T) {
	var calls atomic.Int32
	h := newDirectHarness(t, nil)
	h.upstream.setHandler(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		h.upstream.mu.Lock()
		h.upstream.requests = append(h.upstream.requests, recordedRequest{header: r.Header.Clone(), body: mustJSON(body)})
		h.upstream.mu.Unlock()
		if body["service_tier"] == "priority" && body["model"] == "gpt-5.6-terra" {
			w.WriteHeader(http.StatusBadRequest)
			_, _ = w.Write([]byte(`{"error":{"type":"invalid_request_error","param":"service_tier","message":"SENTINEL-upstream-message"}}`))
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		for _, frame := range successFrames(body["model"].(string), "ok") {
			_, _ = w.Write([]byte(frame))
		}
	})
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
	if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(h.upstream.recorded()[2].body, []byte("service_tier")) {
		t.Fatal("the next call for the same model still sent priority")
	}
	other := testRequest()
	other.Model = "gpt-5.6-sol"
	other.ServiceTier = "priority"
	if _, err := h.transport.Invoke(t.Context(), h.home, other); err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(h.upstream.recorded()[3].body, []byte(`"service_tier":"priority"`)) {
		t.Fatal("another model lost its priority tier")
	}
}

func mustJSON(value any) []byte {
	encoded, _ := json.Marshal(value)
	return encoded
}

func TestPromptCacheKeyScoping(t *testing.T) {
	key := promptCacheKey("conn-1", "gpt-5.6-terra")
	if key != promptCacheKey("conn-1", "gpt-5.6-terra") {
		t.Fatal("key is not stable")
	}
	if key == promptCacheKey("conn-2", "gpt-5.6-terra") || key == promptCacheKey("conn-1", "gpt-5.6-sol") {
		t.Fatal("key is shared across connections or models")
	}
	if strings.Contains(key, "conn-1") {
		t.Fatal("key leaks the connection id")
	}
	namespace := uuid.NewSHA1(uuid.NameSpaceURL, []byte("urn:verevon:integration-core:codex-subscription:prompt-cache"))
	if want := uuid.NewSHA1(namespace, []byte("conn-1\x00gpt-5.6-terra")).String(); key != want {
		t.Fatalf("key = %s, want %s", key, want)
	}
	if parsed, err := uuid.Parse(key); err != nil || parsed.Version() != 5 {
		t.Fatalf("key %s is not a UUIDv5", key)
	}
}

func TestLiteShape(t *testing.T) {
	h := newDirectHarness(t, nil, func(o *harnessOptions) { o.direct.RequestShape = RequestShapeLite })
	freshAuth(t, h.home, h.now)
	request := testRequest()
	request.Messages = []ChatMessage{{Role: "system", Content: "Be brief."}, {Role: "user", Content: "hello"}}
	if _, err := h.transport.Invoke(t.Context(), h.home, request); err != nil {
		t.Fatal(err)
	}
	body := h.upstream.lastBody(t)
	if _, present := body["instructions"]; present {
		t.Fatal("lite shape must not send top-level instructions")
	}
	input := body["input"].([]any)
	first := input[0].(map[string]any)
	if first["role"] != "developer" || first["content"].([]any)[0].(map[string]any)["text"] != codexBaseInstructions+"\n\nBe brief." {
		t.Fatalf("first lite item = %v", first)
	}
	if body["reasoning"].(map[string]any)["context"] != "all_turns" || body["parallel_tool_calls"] != false {
		t.Fatalf("lite reasoning/parallel = %v/%v", body["reasoning"], body["parallel_tool_calls"])
	}
}

func TestCredentialsOnlyToConfiguredHost(t *testing.T) {
	var elsewhere atomic.Int32
	other := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		elsewhere.Add(1)
		w.WriteHeader(http.StatusOK)
	}))
	defer other.Close()
	h := newDirectHarness(t, []fakeTurn{{status: http.StatusFound, headers: map[string]string{"Location": other.URL + "/steal"}, body: "moved"}})
	freshAuth(t, h.home, h.now)
	_, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindHTTPStatus || upstream.HTTPStatus != http.StatusFound {
		t.Fatalf("redirect error = %v, want http_status 302", err)
	}
	if elsewhere.Load() != 0 {
		t.Fatal("the credential followed a redirect to another host")
	}
}
