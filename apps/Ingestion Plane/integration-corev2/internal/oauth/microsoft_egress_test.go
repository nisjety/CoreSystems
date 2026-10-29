package oauth

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/egress"
)

// TestMicrosoftClientHTTPClientRoutesThroughEgressGuard proves that
// constructing a MicrosoftClient with an egress.SafeClient — the exact
// pattern cmd/api/main.go and cmd/email-worker/main.go now use — actually
// routes outbound calls through the guard, rather than merely compiling.
//
// This is deliberately a negative-direction test: a raw &http.Client{} would
// happily reach the loopback httptest.Server started below, so this only
// passes because SafeClient's pinned dial refuses to connect to it. That
// makes it a mechanical proof that the two call-site changes are the right
// shape, without needing to make main() itself testable.
func TestMicrosoftClientHTTPClientRoutesThroughEgressGuard(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		t.Fatal("token endpoint must not be reachable: the egress guard should block the loopback dial before any request is sent")
	}))
	defer server.Close()

	client := NewMicrosoftClient(MicrosoftClientConfig{
		ClientID:   "microsoft-client",
		TokenURL:   server.URL, // httptest.Server binds to 127.0.0.1 — loopback, blocked by default policy
		HTTPClient: egress.SafeClient(egress.ClientConfig{RequestTimeout: 5 * time.Second}),
	})

	_, err := client.ExchangeCode(context.Background(), "code", "https://app.test/oauth/callback/microsoft", "verifier", nil)
	if err == nil {
		t.Fatal("ExchangeCode error = nil, want an error wrapping egress.ErrBlocked")
	}
	if !errors.Is(err, egress.ErrBlocked) {
		t.Fatalf("ExchangeCode error = %v, want an error wrapping egress.ErrBlocked", err)
	}
}
