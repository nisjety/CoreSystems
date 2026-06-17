package channel

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/bridge-core/internal/delivery"
)

func TestWebhookAdapter_RequiresDestinationAndStore(t *testing.T) {
	if _, err := NewWebhookAdapter(WebhookConfig{}, delivery.NewMemoryStore()); err == nil {
		t.Fatal("expected error for missing destination")
	}
	if _, err := NewWebhookAdapter(WebhookConfig{Destination: "http://x"}, nil); err == nil {
		t.Fatal("expected error for nil store")
	}
}

func TestWebhookAdapter_IngestNormalisesJSON(t *testing.T) {
	a, err := NewWebhookAdapter(WebhookConfig{ChannelName: "web", Destination: "http://x"}, delivery.NewMemoryStore())
	if err != nil {
		t.Fatal(err)
	}
	out, err := a.Ingest(context.Background(), "sess-1", []byte(`{"hello":"world"}`))
	if err != nil {
		t.Fatal(err)
	}
	var evt struct {
		Channel   string          `json:"channel"`
		SessionID string          `json:"session_id"`
		Payload   json.RawMessage `json:"payload"`
		Text      string          `json:"text"`
	}
	if err := json.Unmarshal(out, &evt); err != nil {
		t.Fatalf("ingest output not valid JSON: %v", err)
	}
	if evt.Channel != "web" || evt.SessionID != "sess-1" {
		t.Fatalf("unexpected envelope: %+v", evt)
	}
	if string(evt.Payload) != `{"hello":"world"}` {
		t.Fatalf("expected payload embedded verbatim, got %s", evt.Payload)
	}
	if evt.Text != "" {
		t.Fatalf("expected empty text for JSON payload, got %q", evt.Text)
	}
}

func TestWebhookAdapter_IngestNonJSONGoesToText(t *testing.T) {
	a, _ := NewWebhookAdapter(WebhookConfig{ChannelName: "api", Destination: "http://x"}, delivery.NewMemoryStore())
	out, err := a.Ingest(context.Background(), "s", []byte("plain text"))
	if err != nil {
		t.Fatal(err)
	}
	var evt struct {
		Text string `json:"text"`
	}
	_ = json.Unmarshal(out, &evt)
	if evt.Text != "plain text" {
		t.Fatalf("expected text fallback, got %q", evt.Text)
	}
}

func TestWebhookAdapter_DeliverEnqueues(t *testing.T) {
	store := delivery.NewMemoryStore()
	a, _ := NewWebhookAdapter(WebhookConfig{ChannelName: "web", Destination: "http://x", MaxAttempts: 4}, store)

	if err := a.Deliver(context.Background(), "sess-9", []byte(`{"ok":true}`)); err != nil {
		t.Fatal(err)
	}
	n, _ := store.PendingCount(context.Background())
	if n != 1 {
		t.Fatalf("expected 1 pending delivery, got %d", n)
	}
}

// End-to-end: adapter enqueues -> worker drains -> real HTTP POST hits a test
// server with the expected headers and body.
func TestWebhook_EndToEndDelivery(t *testing.T) {
	var hits int32
	var gotBody []byte
	var gotSession, gotChannel string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddInt32(&hits, 1)
		gotSession = r.Header.Get("X-Bridge-Session")
		gotChannel = r.Header.Get("X-Bridge-Channel")
		gotBody, _ = io.ReadAll(r.Body)
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()

	store := delivery.NewMemoryStore()
	a, _ := NewWebhookAdapter(WebhookConfig{ChannelName: "web", Destination: srv.URL, MaxAttempts: 3}, store)
	worker := delivery.NewWorker(store, NewHTTPSender(nil), delivery.DefaultConfig())

	if err := a.Deliver(context.Background(), "sess-e2e", []byte(`{"msg":"hi"}`)); err != nil {
		t.Fatal(err)
	}
	worker.RunOnce(context.Background())

	if atomic.LoadInt32(&hits) != 1 {
		t.Fatalf("expected 1 webhook hit, got %d", hits)
	}
	if string(gotBody) != `{"msg":"hi"}` {
		t.Fatalf("unexpected webhook body: %s", gotBody)
	}
	if gotSession != "sess-e2e" || gotChannel != "web" {
		t.Fatalf("missing routing headers: session=%q channel=%q", gotSession, gotChannel)
	}
	pending, _ := store.PendingCount(context.Background())
	if pending != 0 {
		t.Fatalf("expected outbox drained, got %d pending", pending)
	}
}

// End-to-end retry: first response 503, second 200 -> delivered after retry.
func TestWebhook_EndToEndRetryOn5xx(t *testing.T) {
	var hits int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		if atomic.AddInt32(&hits, 1) == 1 {
			w.WriteHeader(http.StatusServiceUnavailable)
			return
		}
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()

	store := delivery.NewMemoryStore()
	a, _ := NewWebhookAdapter(WebhookConfig{ChannelName: "api", Destination: srv.URL, MaxAttempts: 3}, store)
	worker := delivery.NewWorker(store, NewHTTPSender(nil), delivery.Config{
		BaseBackoff: time.Nanosecond, MaxBackoff: time.Nanosecond,
	})

	if err := a.Deliver(context.Background(), "s", []byte(`{}`)); err != nil {
		t.Fatal(err)
	}

	// First pass: 503 -> retry re-arm (backoff ~0).
	worker.RunOnce(context.Background())
	time.Sleep(2 * time.Millisecond)
	// Second pass: 200 -> delivered.
	worker.RunOnce(context.Background())

	if atomic.LoadInt32(&hits) != 2 {
		t.Fatalf("expected 2 hits (1 fail + 1 success), got %d", hits)
	}
	pending, _ := store.PendingCount(context.Background())
	if pending != 0 {
		t.Fatalf("expected delivered after retry, got %d pending", pending)
	}
}
