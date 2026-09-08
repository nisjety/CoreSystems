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
