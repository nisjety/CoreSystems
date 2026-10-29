package resources

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// capturedNotification is what a faithful stand-in for notification-core's
// intake (POST /api/v1/notification-requests) records.
type capturedNotification struct {
	RecipientID    string         `json:"recipient_id"`
	Type           string         `json:"type"`
	Payload        map[string]any `json:"payload"`
	IdempotencyKey string         `json:"idempotency_key"`
	Source         string         `json:"source"`
	InternalKey    string         `json:"-"`
}

// TestChangeToInProductNotification_E2E exercises the full W2 monitor→notify
// leg through real wiring:
//
//	POST /v1/runs/{id}/events  (a `change_detected` event with a creator)
//	  → notifyOnChange → notify.HTTPSink.NotifyChange
//	  → POST notification-core /api/v1/notification-requests
//
// By default it runs HERMETICALLY against an httptest stand-in for
// notification-core, so it is a real end-to-end of the producing side (event
// intake → sink → HTTP POST) without external infra and is NOT skipped in CI.
//
// Set QUARRY_E2E_NOTIFY_URL to a live notification-core base URL (and optionally
// QUARRY_E2E_NOTIFY_KEY for x-internal-api-key) to instead drive the real stack.
func TestChangeToInProductNotification_E2E(t *testing.T) {
	const recipient = "user_creator_1"
	const internalKey = "test-internal-key"

	var notifyBaseURL string
	var sentKeyHeader string
	captured := make(chan capturedNotification, 4)

	if live := strings.TrimSpace(os.Getenv("QUARRY_E2E_NOTIFY_URL")); live != "" {
		// Live-stack mode: post against the real notification-core intake.
		notifyBaseURL = live
		sentKeyHeader = os.Getenv("QUARRY_E2E_NOTIFY_KEY")
		t.Logf("change→notify e2e: LIVE mode against %s", live)
	} else {
		// Hermetic mode: a stand-in that records the POST notification-core
		// would have received, then 201s like the real intake.
		stub := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if r.URL.Path != "/api/v1/notification-requests" || r.Method != http.MethodPost {
				http.Error(w, "unexpected route", http.StatusNotFound)
				return
			}
			body, _ := io.ReadAll(r.Body)
			var n capturedNotification
			_ = json.Unmarshal(body, &n)
			n.InternalKey = r.Header.Get("x-internal-api-key")
			captured <- n
			w.WriteHeader(http.StatusCreated)
		}))
		defer stub.Close()
		notifyBaseURL = stub.URL
		sentKeyHeader = internalKey
		t.Logf("change→notify e2e: HERMETIC mode (httptest notification-core stand-in)")
	}

	// Stand up control's event intake wired to the REAL HTTPSink → notify base.
	sink := notify.NewHTTPSink(notifyBaseURL, sentKeyHeader)
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountEvents(r, db, "", sink) // empty apiKey → bearer auth disabled for the test
	srv := httptest.NewServer(r)
	defer srv.Close()

	// Post a change_detected event for a run, carrying the creator (the value
	// ChangeMonitorWF stamps so the notification reaches the schedule's owner).
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	batch := []map[string]any{{
		"type": string(quarrycontracts.EvtChangeDetected),
		"payload": map[string]any{
			"org_id":     "org_a",
			"created_by": recipient,
			"source_url": "https://a.example/pricing",
			"status":     "changed",
		},
	}}
	body, _ := json.Marshal(batch)
	resp, err := http.Post(srv.URL+"/v1/runs/"+string(runID)+"/events", "application/json", strings.NewReader(string(body)))
	if err != nil {
		t.Fatalf("post events: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusCreated {
		rb, _ := io.ReadAll(resp.Body)
		t.Fatalf("event intake status=%d body=%s", resp.StatusCode, string(rb))
	}

	if notifyBaseURL != "" && strings.HasPrefix(notifyBaseURL, "http") && len(captured) == 0 && os.Getenv("QUARRY_E2E_NOTIFY_URL") != "" {
		// Live mode: we can only assert the intake accepted the POST (the sink
		// returns nil on 2xx). A non-2xx would have been logged by notifyOnChange.
		t.Log("change→notify e2e (live): event accepted; sink delivery is best-effort and asserted via control logs")
		return
	}

	// Hermetic mode: assert the notification-core stand-in received exactly the
	// change_detected notification, addressed to the creator, idempotency-keyed
	// on the event id, sourced from quarry, with the internal key forwarded.
	select {
	case n := <-captured:
		if n.Type != "change_detected" {
			t.Errorf("notification type=%q want=change_detected", n.Type)
		}
		if n.RecipientID != recipient {
			t.Errorf("recipient=%q want=%q", n.RecipientID, recipient)
		}
		if n.Source != "quarry" {
			t.Errorf("source=%q want=quarry", n.Source)
		}
		if n.IdempotencyKey == "" {
			t.Error("expected idempotency_key (the event id) to be set")
		}
		if n.InternalKey != internalKey {
			t.Errorf("x-internal-api-key=%q want=%q", n.InternalKey, internalKey)
		}
		if got, _ := n.Payload["status"].(string); got != "changed" {
			t.Errorf("payload.status=%q want=changed", got)
		}
	default:
		t.Fatal("notification-core stand-in received no change notification")
	}
}

// TestChangeNotify_SkipsWhenNoCreator locks the honesty rule: a change event
// with no creator (legacy / non-user schedule) produces NO notification rather
// than fabricating a recipient.
func TestChangeNotify_SkipsWhenNoCreator(t *testing.T) {
	t.Parallel()

	got := make(chan struct{}, 1)
	stub := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		got <- struct{}{}
		w.WriteHeader(http.StatusCreated)
	}))
	defer stub.Close()

	sink := notify.NewHTTPSink(stub.URL, "")
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountEvents(r, db, "", sink)
	srv := httptest.NewServer(r)
	defer srv.Close()

	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	batch := []map[string]any{{
		"type":    string(quarrycontracts.EvtChangeDetected),
		"payload": map[string]any{"org_id": "org_a", "status": "changed"}, // no created_by
	}}
	body, _ := json.Marshal(batch)
	resp, err := http.Post(srv.URL+"/v1/runs/"+string(runID)+"/events", "application/json", strings.NewReader(string(body)))
	if err != nil {
		t.Fatalf("post: %v", err)
	}
	resp.Body.Close()

	select {
	case <-got:
		t.Fatal("a creator-less change event must NOT produce a notification")
	default:
	}
}
