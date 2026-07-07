package oauth

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"
)

func TestMicrosoftClientDefaultsToPublicClientAuth(t *testing.T) {
	type tokenRequest struct {
		form   url.Values
		origin string
	}
	requests := []tokenRequest{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil {
			t.Fatalf("ParseForm error: %v", err)
		}
		requests = append(requests, tokenRequest{form: cloneValues(r.Form), origin: r.Header.Get("Origin")})
		if got := r.Form.Get("client_secret"); got != "" {
			t.Fatalf("client_secret = %q, want omitted for public Microsoft client", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token": "access-token",
			"token_type":   "Bearer",
			"expires_in":   3600,
		})
	}))
	defer server.Close()

	client := NewMicrosoftClient(MicrosoftClientConfig{
		ClientID:     "microsoft-client",
		ClientSecret: "must-not-be-sent",
		TokenOrigin:  "https://app.test",
		TokenURL:     server.URL,
	})

	if _, err := client.ExchangeCode(context.Background(), "code", "https://app.test/oauth/callback/microsoft", "verifier", []string{"User.Read", "offline_access"}); err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
	if _, err := client.Refresh(context.Background(), "refresh-token", []string{"User.Read", "offline_access"}); err != nil {
		t.Fatalf("Refresh error: %v", err)
	}
	if len(requests) != 2 {
		t.Fatalf("token endpoint calls = %d, want 2", len(requests))
	}
	if got := requests[0].form.Get("code_verifier"); got != "verifier" {
		t.Fatalf("code_verifier = %q, want verifier", got)
	}
	for i, request := range requests {
		if request.origin != "https://app.test" {
			t.Fatalf("request %d Origin = %q, want https://app.test", i, request.origin)
		}
	}
}

func TestMicrosoftClientPublicAuthDerivesOriginFromRedirectURI(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("Origin"); got != "https://tenant.example.com" {
			t.Fatalf("Origin = %q, want redirect origin", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token": "access-token",
			"token_type":   "Bearer",
			"expires_in":   3600,
		})
	}))
	defer server.Close()

	client := NewMicrosoftClient(MicrosoftClientConfig{
		ClientID: "microsoft-client",
		TokenURL: server.URL,
	})

	if _, err := client.ExchangeCode(context.Background(), "code", "https://tenant.example.com/oauth/callback/microsoft", "verifier", nil); err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
}

func TestMicrosoftClientConfidentialAuthSendsClientSecret(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil {
			t.Fatalf("ParseForm error: %v", err)
		}
		if got := r.Form.Get("client_secret"); got != "microsoft-secret" {
			t.Fatalf("client_secret = %q, want microsoft-secret", got)
		}
		if got := r.Header.Get("Origin"); got != "" {
			t.Fatalf("Origin = %q, want omitted for confidential Microsoft client", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token": "access-token",
			"token_type":   "Bearer",
			"expires_in":   3600,
		})
	}))
	defer server.Close()

	client := NewMicrosoftClient(MicrosoftClientConfig{
		ClientID:       "microsoft-client",
		ClientSecret:   "microsoft-secret",
		ClientAuthMode: "confidential",
		TokenOrigin:    "https://app.test",
		TokenURL:       server.URL,
	})

	if _, err := client.ExchangeCode(context.Background(), "code", "https://app.test/oauth/callback/microsoft", "verifier", nil); err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
}
