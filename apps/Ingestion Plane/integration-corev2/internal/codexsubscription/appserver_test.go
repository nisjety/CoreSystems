package codexsubscription

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestCodexAppServerForcesFileCredentialStore(t *testing.T) {
	want := []string{"app-server", "-c", `cli_auth_credentials_store="file"`}
	if got := codexAppServerArgs(); !reflect.DeepEqual(got, want) {
		t.Fatalf("codexAppServerArgs() = %q, want %q", got, want)
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
	})
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
	thread := threadStartParams("/isolated/workspace", request)
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
