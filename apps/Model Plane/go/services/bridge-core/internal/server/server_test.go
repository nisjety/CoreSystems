package server

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/triodelab/model-plane/services/bridge-core/internal/channel"
	"github.com/triodelab/model-plane/services/bridge-core/internal/delivery"
	"github.com/triodelab/model-plane/services/bridge-core/internal/session"
)

// newTestServer builds a Server with a webhook adapter on the "web" channel
// backed by an in-memory outbox, and returns the server, store, and an httptest
// server.
func newTestServer(t *testing.T) (*httptest.Server, delivery.Store) {
	t.Helper()
	reg := session.NewRegistry()
	adapters := channel.NewAdapterRegistry()
	store := delivery.NewMemoryStore()
	wa, err := channel.NewWebhookAdapter(channel.WebhookConfig{
		ChannelName: "web", Destination: "http://example.invalid/hook", MaxAttempts: 3,
	}, store)
	if err != nil {
		t.Fatal(err)
	}
	adapters.Register("web", wa)
	srv := NewServer(reg, adapters)
	ts := httptest.NewServer(srv.Handler())
	t.Cleanup(ts.Close)
	return ts, store
}

func TestIngest_RegisterThenIngestEnqueuesDelivery(t *testing.T) {
	ts, store := newTestServer(t)

	// Register a "web" session.
	regBody, _ := json.Marshal(map[string]string{"org_id": "o1", "user_id": "u1", "channel": "web"})
	resp, err := http.Post(ts.URL+"/api/v1/sessions", "application/json", bytes.NewReader(regBody))
	if err != nil {
		t.Fatal(err)
	}
	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("register status: %d", resp.StatusCode)
	}
	var sess session.Session
	_ = json.NewDecoder(resp.Body).Decode(&sess)
	_ = resp.Body.Close()
	if sess.ID == "" {
		t.Fatal("no session id returned")
	}

	// Ingest a payload (Go []byte JSON = base64 string).
	ingBody, _ := json.Marshal(map[string][]byte{"payload": []byte(`{"prompt":"hello"}`)})
	resp2, err := http.Post(ts.URL+"/api/v1/sessions/"+sess.ID+"/ingest", "application/json", bytes.NewReader(ingBody))
	if err != nil {
		t.Fatal(err)
	}
	if resp2.StatusCode != http.StatusOK {
		t.Fatalf("ingest status: %d", resp2.StatusCode)
	}
	var ingResp struct {
		SessionID string `json:"session_id"`
		Delivered bool   `json:"delivered"`
	}
	_ = json.NewDecoder(resp2.Body).Decode(&ingResp)
	_ = resp2.Body.Close()

	if ingResp.SessionID != sess.ID {
		t.Fatalf("ingest echoed wrong session: %s", ingResp.SessionID)
	}
	if !ingResp.Delivered {
		t.Fatal("expected delivered=true (enqueue succeeded)")
	}

	// The webhook adapter should have enqueued exactly one durable delivery.
	n, _ := store.PendingCount(context.Background())
	if n != 1 {
		t.Fatalf("expected 1 pending outbox record, got %d", n)
	}
}

func TestIngest_UnknownSession404(t *testing.T) {
	ts, _ := newTestServer(t)
	ingBody, _ := json.Marshal(map[string][]byte{"payload": []byte("x")})
	resp, err := http.Post(ts.URL+"/api/v1/sessions/nope/ingest", "application/json", bytes.NewReader(ingBody))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusNotFound {
		t.Fatalf("expected 404, got %d", resp.StatusCode)
	}
}
