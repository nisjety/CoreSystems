package codexsubscription

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestManagerCompletesDeviceLoginWithoutPersistingTokens(t *testing.T) {
	runner := &fakeRunner{login: &fakeLogin{states: []ProcessLoginStatus{{Status: "pending"}, {Status: "connected"}}}}
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir(), LoginTTL: time.Minute}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	login, err := manager.StartLogin(t.Context(), "conn-1")
	if err != nil {
		t.Fatalf("StartLogin: %v", err)
	}
	if login.ConnectionID != "conn-1" || login.UserCode != "ABCD-EFGH" {
		t.Fatalf("login = %+v", login)
	}
	pending, err := manager.PollLogin(t.Context(), login.LoginID)
	if err != nil || pending.Status != "pending" {
		t.Fatalf("first PollLogin = %+v, %v", pending, err)
	}
	connected, err := manager.PollLogin(t.Context(), login.LoginID)
	if err != nil || connected.Status != "connected" {
		t.Fatalf("second PollLogin = %+v, %v", connected, err)
	}
	if !runner.login.closed {
		t.Fatal("completed login process was not closed")
	}
}

func TestManagerRejectsConnectionIDsThatEscapeItsCredentialRoot(t *testing.T) {
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir()}, &fakeRunner{})
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	_, err = manager.StartLogin(t.Context(), "../outside")
	if !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("StartLogin error = %v, want ErrInvalidRequest", err)
	}
}

func TestManagerInvokesOnlyInsideTheConnectionHome(t *testing.T) {
	runner := &fakeRunner{}
	root := t.TempDir()
	manager, err := NewManager(Config{Enabled: true, Home: root}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	_, err = manager.Invoke(t.Context(), InvokeRequest{ConnectionID: "conn-1", Model: "gpt-5", Messages: []ChatMessage{{Role: "user", Content: "hello"}}, ReasoningEffort: "low"})
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	want := filepath.Join(root, "conn-1")
	if runner.invokeHome != want {
		t.Fatalf("runner code home = %q, want %q", runner.invokeHome, want)
	}
	if runner.invokeRequest.ReasoningEffort != "low" {
		t.Fatalf("runner reasoning effort = %q, want low", runner.invokeRequest.ReasoningEffort)
	}
}

func TestManagerRejectsUnsupportedReasoningEffort(t *testing.T) {
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir()}, &fakeRunner{})
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	_, err = manager.Invoke(t.Context(), InvokeRequest{
		ConnectionID:    "conn-1",
		Model:           "gpt-5",
		Messages:        []ChatMessage{{Role: "user", Content: "hello"}},
		ReasoningEffort: "maximum",
	})
	if !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("Invoke error = %v, want ErrInvalidRequest", err)
	}
}

func TestManagerRejectsUnsupportedServiceTier(t *testing.T) {
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir()}, &fakeRunner{})
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	_, err = manager.Invoke(t.Context(), InvokeRequest{
		ConnectionID:    "conn-1",
		Model:           "gpt-5",
		Messages:        []ChatMessage{{Role: "user", Content: "hello"}},
		ReasoningEffort: "low",
		ServiceTier:     "unlimited",
	})
	if !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("Invoke error = %v, want ErrInvalidRequest", err)
	}
}

func TestManagerInvokeStreamNormalizesDefaultsAndForwardsDeltas(t *testing.T) {
	runner := &fakeRunner{}
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir()}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	var streamed strings.Builder
	response, err := manager.InvokeStream(t.Context(), InvokeRequest{
		ConnectionID: "conn-1",
		RequestID:    "req-1",
		Model:        "gpt-5",
		Messages:     []ChatMessage{{Role: "user", Content: "hello"}},
	}, func(delta string) error {
		streamed.WriteString(delta)
		return nil
	})
	if err != nil {
		t.Fatalf("InvokeStream: %v", err)
	}
	if streamed.String() != "streamed answer" || response.Content != "streamed answer" {
		t.Fatalf("streamed = %q, response = %#v", streamed.String(), response)
	}
	if runner.invokeRequest.ReasoningEffort != "low" {
		t.Fatalf("runner reasoning effort = %q, want low", runner.invokeRequest.ReasoningEffort)
	}
}

type fakeRunner struct {
	login         *fakeLogin
	invokeHome    string
	invokeRequest InvokeRequest
}

func TestManagerValidatesStructuredOutputBeforeStartingRunner(t *testing.T) {
	runner := &fakeRunner{}
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir()}, runner)
	if err != nil {
		t.Fatal(err)
	}
	request := InvokeRequest{ConnectionID: "conn-1", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "Propose a document"}}}
	for _, invalid := range []string{`null`, `[]`, `{"type":"array"}`, `{`, strings.Repeat(" ", 256*1024+1)} {
		request.OutputSchema = []byte(invalid)
		if _, err := manager.Invoke(t.Context(), request); !errors.Is(err, ErrInvalidRequest) {
			t.Fatalf("expected invalid schema, got %v", err)
		}
		if runner.invokeHome != "" {
			t.Fatal("invalid schema invoked runner")
		}
	}
	request.OutputSchema = []byte(`{"type":"object"}`)
	if _, err := manager.Invoke(t.Context(), request); err != nil {
		t.Fatal(err)
	}
	if string(runner.invokeRequest.OutputSchema) != string(request.OutputSchema) {
		t.Fatal("output schema lost")
	}
}

func (f *fakeRunner) BeginDeviceLogin(_ context.Context, _ string) (LoginProcess, DeviceCode, error) {
	if f.login == nil {
		f.login = &fakeLogin{}
	}
	return f.login, DeviceCode{VerificationURL: "https://auth.openai.com/codex/device", UserCode: "ABCD-EFGH"}, nil
}

func (f *fakeRunner) Invoke(_ context.Context, codeHome string, request InvokeRequest) (InvokeResponse, error) {
	f.invokeHome = codeHome
	f.invokeRequest = request
	return InvokeResponse{RequestID: request.RequestID, Content: "ok", ModelUsed: request.Model}, nil
}

func (f *fakeRunner) InvokeStream(_ context.Context, codeHome string, request InvokeRequest, onDelta func(string) error) (InvokeResponse, error) {
	f.invokeHome = codeHome
	f.invokeRequest = request
	if err := onDelta("streamed "); err != nil {
		return InvokeResponse{}, err
	}
	if err := onDelta("answer"); err != nil {
		return InvokeResponse{}, err
	}
	return InvokeResponse{RequestID: request.RequestID, Content: "streamed answer", ModelUsed: request.Model}, nil
}

func (f *fakeRunner) Logout(context.Context, string) error { return nil }

type fakeLogin struct {
	states []ProcessLoginStatus
	closed bool
}

func (f *fakeLogin) Poll(context.Context) (ProcessLoginStatus, error) {
	if len(f.states) == 0 {
		return ProcessLoginStatus{Status: "pending"}, nil
	}
	state := f.states[0]
	f.states = f.states[1:]
	return state, nil
}

func (f *fakeLogin) Close() error {
	f.closed = true
	return nil
}
