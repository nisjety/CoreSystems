package mailtask

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

// TestGatewayIngestClientForwardsTheThreadDecisionInTheForwardedBody is the
// delegated-identity design's own gateway_client.go exact test: the field
// rides in the forwarded body, verbatim (Gap B §2.10).
func TestGatewayIngestClientForwardsTheThreadDecisionInTheForwardedBody(t *testing.T) {
	var gotBody map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := json.NewDecoder(r.Body).Decode(&gotBody); err != nil {
			t.Errorf("decode forwarded body: %v", err)
		}
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	client := &HTTPGatewayIngestClient{BaseURL: server.URL, ServiceToken: "test-secret-at-least-32-bytes-long"}
	err := client.Ingest(context.Background(), GatewayIngestRequest{
		OrgID:             "org-1",
		UserID:            "user-1",
		ProviderMessageID: "provider-msg-1",
		OccurredAt:        time.Now().UTC(),
		ThreadDecision:    "mail-task-v1.key-1.payload-1.sig-1",
	})
	if err != nil {
		t.Fatalf("Ingest() error = %v", err)
	}
	if gotBody["thread_decision"] != "mail-task-v1.key-1.payload-1.sig-1" {
		t.Fatalf("forwarded thread_decision = %v, want it passed through verbatim", gotBody["thread_decision"])
	}
}

// TestMailTaskIngestDelegationHeadersAreDeterministic pins the header shape
// for a fixed input so the gateway's own verification tests (Rust side) can
// be written against a known-good header set without a live cross-service
// call -- the same "cross-language contract" pattern
// conversation-ingest-rs/src/lib.rs's
// conversation_delegation_matches_go_cross_language_contract test uses.
func TestMailTaskIngestDelegationHeadersAreDeterministic(t *testing.T) {
	body := []byte(`{"org_id":"org-1"}`)
	timestamp := time.Date(2026, 7, 13, 12, 0, 0, 0, time.UTC)

	headers := mailTaskIngestDelegationHeaders(
		"0123456789abcdef0123456789abcdef",
		"POST",
		"/internal/mail-task/ingest",
		body,
		"org-1",
		timestamp,
		"fixed-nonce-1234567890",
	)

	if headers["x-service-id"] != gatewayIngestServiceID {
		t.Fatalf("x-service-id = %q, want %q", headers["x-service-id"], gatewayIngestServiceID)
	}
	if headers["x-org-id"] != "org-1" {
		t.Fatalf("x-org-id = %q", headers["x-org-id"])
	}
	if headers["x-delegation-nonce"] != "fixed-nonce-1234567890" {
		t.Fatalf("x-delegation-nonce = %q", headers["x-delegation-nonce"])
	}
	if headers["x-delegation-body-sha256"] == "" || headers["x-delegation-signature"] == "" {
		t.Fatal("expected non-empty digest and signature")
	}
	if _, hasLegacyKey := headers["x-internal-api-key"]; hasLegacyKey {
		t.Fatal("must never fall back to the legacy shared internal key")
	}
}

func TestMailTaskIngestDelegationHeadersChangeWithTheBody(t *testing.T) {
	timestamp := time.Date(2026, 7, 13, 12, 0, 0, 0, time.UTC)
	a := mailTaskIngestDelegationHeaders("secret-at-least-32-bytes-long!!!", "POST", "/internal/mail-task/ingest", []byte(`{"a":1}`), "org-1", timestamp, "nonce-1")
	b := mailTaskIngestDelegationHeaders("secret-at-least-32-bytes-long!!!", "POST", "/internal/mail-task/ingest", []byte(`{"a":2}`), "org-1", timestamp, "nonce-1")

	if a["x-delegation-body-sha256"] == b["x-delegation-body-sha256"] {
		t.Fatal("different bodies must produce different digests")
	}
	if a["x-delegation-signature"] == b["x-delegation-signature"] {
		t.Fatal("different bodies must produce different signatures")
	}
}
