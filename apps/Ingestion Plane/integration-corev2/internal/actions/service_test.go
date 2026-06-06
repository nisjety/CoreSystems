package actions

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestExecuteSlackChannelsList(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/conversations.list" {
			t.Fatalf("path = %s, want /conversations.list", r.URL.Path)
		}
		if got := r.URL.Query().Get("types"); got != "public_channel" {
			t.Fatalf("types = %q, want public_channel", got)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token" {
			t.Fatalf("Authorization = %q, want Bearer token", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
	}))
	defer server.Close()

	service := NewService(config.Config{SlackAPIBaseURL: server.URL}, server.Client())
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "slack"},
		AccessToken: "token",
		Operation:   "channels.list",
		Params:      map[string]any{"types": "public_channel"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	if result.ProviderKey != "slack" || result.Operation != "channels.list" {
		t.Fatalf("result = %#v", result)
	}
}

func TestExecuteGitHubRepoEscapesPath(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/repos/triodelab/velion" {
			t.Fatalf("path = %s, want /repos/triodelab/velion", r.URL.Path)
		}
		if got := r.Header.Get("X-GitHub-Api-Version"); got == "" {
			t.Fatalf("missing GitHub API version header")
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"full_name": "triodelab/velion"})
	}))
	defer server.Close()

	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "github"},
		AccessToken: "token",
		Operation:   "repo",
		Params:      map[string]any{"owner": "triodelab", "repo": "velion"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteSlackUserInfo(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/users.info" {
			t.Fatalf("path = %s, want /users.info", r.URL.Path)
		}
		if got := r.URL.Query().Get("user"); got != "U123" {
			t.Fatalf("user = %q, want U123", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
	}))
	defer server.Close()

	service := NewService(config.Config{SlackAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "slack"},
		AccessToken: "token",
		Operation:   "user",
		Params:      map[string]any{"userId": "U123"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteGitHubTeams(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/orgs/triodelab/teams" {
			t.Fatalf("path = %s, want /orgs/triodelab/teams", r.URL.Path)
		}
		_ = json.NewEncoder(w).Encode([]map[string]any{})
	}))
	defer server.Close()

	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "github"},
		AccessToken: "token",
		Operation:   "teams",
		Params:      map[string]any{"org": "triodelab"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteOktaUsersUsesAPIToken(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/users" {
			t.Fatalf("path = %s, want /api/v1/users", r.URL.Path)
		}
		if got := r.Header.Get("Authorization"); got != "SSWS okta-token" {
			t.Fatalf("Authorization = %q, want SSWS okta-token", got)
		}
		_ = json.NewEncoder(w).Encode([]map[string]any{{"id": "00u1"}})
	}))
	defer server.Close()

	service := NewService(config.Config{OktaAPIBaseURL: server.URL, OktaAPIToken: "okta-token"}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{
			ProviderKey:  "okta",
			Capabilities: []string{"directory.read"},
		},
		Operation: "users",
		Params:    map[string]any{"limit": 10},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteRejectsUnsupportedOperation(t *testing.T) {
	service := NewService(config.Config{}, http.DefaultClient)
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "slack"},
		AccessToken: "token",
		Operation:   "admin.openProxy",
	})
	if err == nil {
		t.Fatalf("expected unsupported operation error")
	}
}
