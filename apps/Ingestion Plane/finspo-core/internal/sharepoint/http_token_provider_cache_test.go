package sharepoint

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"
)

func TestHttpAccessTokenProviderCachesUnexpiredTokenPerOrganization(t *testing.T) {
	var requests atomic.Int32
	expiresAt := time.Now().UTC().Add(time.Hour).Format(time.RFC3339)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		requests.Add(1)
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"data": map[string]string{
				"accessToken": "test-token",
				"expiresAt":   expiresAt,
			},
		})
	}))
	defer server.Close()

	provider := NewHttpAccessTokenProvider(server.URL, "internal-key")
	first, err := provider.AccessToken(context.Background(), "org-123")
	if err != nil {
		t.Fatalf("first AccessToken error: %v", err)
	}
	second, err := provider.AccessToken(context.Background(), "org-123")
	if err != nil {
		t.Fatalf("second AccessToken error: %v", err)
	}

	if first != second {
		t.Fatalf("cached token = %q, want %q", second, first)
	}
	if got := requests.Load(); got != 1 {
		t.Fatalf("token broker calls = %d, want 1", got)
	}
}
