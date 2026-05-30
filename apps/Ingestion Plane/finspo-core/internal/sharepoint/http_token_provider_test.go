package sharepoint

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func serveTokenBroker(t *testing.T, statusCode int, accessToken string) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Errorf("expected POST, got %s", r.Method)
		}
		if r.Header.Get("X-Internal-API-Key") == "" {
			t.Error("X-Internal-API-Key header missing")
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(statusCode)
		if accessToken != "" {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"data": map[string]string{"accessToken": accessToken},
			})
		}
	}))
}

func TestHttpAccessTokenProvider_AccessToken_Success(t *testing.T) {
	srv := serveTokenBroker(t, http.StatusOK, "tok-abc")
	defer srv.Close()

	p := NewHttpAccessTokenProvider(srv.URL, "internal-key")
	got, err := p.AccessToken(context.Background(), "org-123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "tok-abc" {
		t.Fatalf("token = %q, want %q", got, "tok-abc")
	}
}

func TestHttpAccessTokenProvider_AccessToken_NonOKStatus(t *testing.T) {
	srv := serveTokenBroker(t, http.StatusUnauthorized, "")
	defer srv.Close()

	p := NewHttpAccessTokenProvider(srv.URL, "internal-key")
	_, err := p.AccessToken(context.Background(), "org-123")
	if err == nil {
		t.Fatal("expected error for non-200 status, got nil")
	}
}

func TestHttpAccessTokenProvider_AccessToken_EmptyToken(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]any{
			"data": map[string]string{"accessToken": ""},
		})
	}))
	defer srv.Close()

	p := NewHttpAccessTokenProvider(srv.URL, "internal-key")
	_, err := p.AccessToken(context.Background(), "org-123")
	if err == nil {
		t.Fatal("expected error for empty access token, got nil")
	}
}

func TestHttpAccessTokenProvider_AccessToken_Unreachable(t *testing.T) {
	p := NewHttpAccessTokenProvider("http://127.0.0.1:0", "internal-key")
	_, err := p.AccessToken(context.Background(), "org-123")
	if err == nil {
		t.Fatal("expected error when server is unreachable, got nil")
	}
}
