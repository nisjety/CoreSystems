package resources

// Fan-out tenancy (INNH-5): a delivery is created only for an active subscription of the org
// that owns the event. These are the tests that would have failed when fanoutWebhooks listed
// every webhook of every org.

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// newChangeWebhookServerWithGone is newChangeWebhookServer plus the events routes, the
// retired-route mount and the jobs routes, wired the way cmd/control/main.go wires them.
func newChangeWebhookServerWithGone(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	// No HMAC middleware here: the route precedence is what is under test, and the
	// middleware sits above both mounts in main.go.
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountWebhooksGone(r)
	MountChangeWebhook(r, db, nil)
	return r, db
}

func seedSub(t *testing.T, db store.DB, id, org string, events []string, active bool) {
	t.Helper()
	if err := db.Webhooks().Create(store.Webhook{
		ID: quarrycontracts.ID(id), OrgID: org, URL: "https://hooks.example.com/" + id, Secret: "s",
		Events: events, Active: active,
	}); err != nil {
		t.Fatalf("seed %s: %v", id, err)
	}
}

func deliveriesFor(db store.DB, webhookID string) []store.WebhookDelivery {
	var out []store.WebhookDelivery
	all, _ := db.WebhookDeliveries().List(1000, "")
	for _, d := range all {
		if string(d.WebhookID) == webhookID {
			out = append(out, d)
		}
	}
	return out
}

func TestFanout_ChangeEventReachesOnlyItsOrg(t *testing.T) {
	t.Parallel()
	h, db := newChangeWebhookServer(t)
	seedSub(t, db, "whk_a", "o1", []string{"*"}, true)
	seedSub(t, db, "whk_b", "o2", []string{"*"}, true)
	seedSub(t, db, "whk_a_exact", "o1", []string{"change_detected"}, true)
	seedSub(t, db, "whk_a_off", "o1", []string{"*"}, false)
	seedSub(t, db, "whk_a_run", "o1", []string{"run_completed"}, true)
	seedSub(t, db, "whk_a_none", "o1", nil, true)
	seedSub(t, db, "whk_legacy", "", []string{"*"}, true)
	seedSub(t, db, "whk_legacy_all", "", nil, true)

	code, ack := doChange(t, h, signedChangePost(t, "o1", changedBody, "idem-iso", 0, "nonce-iso"))
	if code != http.StatusOK {
		t.Fatalf("status = %d", code)
	}
	want := map[string]int{"whk_a": 1, "whk_a_exact": 1}
	for _, id := range []string{"whk_a", "whk_b", "whk_a_exact", "whk_a_off", "whk_a_run", "whk_a_none", "whk_legacy", "whk_legacy_all"} {
		if got := len(deliveriesFor(db, id)); got != want[id] {
			t.Errorf("%s got %d deliveries, want %d", id, got, want[id])
		}
	}
	d := deliveriesFor(db, "whk_a")[0]
	if d.OrgID != "o1" || d.EventType != "change_detected" || d.EventID != ack.EventID || d.Status != "pending" {
		t.Fatalf("delivery = %+v", d)
	}
}

func TestFanout_UnknownOwnerDeliversToNobody(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	seedSub(t, db, "whk_a", "o1", []string{"*"}, true)
	seedSub(t, db, "whk_legacy", "", []string{"*"}, true)
	ev := quarrycontracts.Event{EventID: "evt_1", Type: quarrycontracts.EvtRunCompleted}
	fanoutWebhooks(db, ev, "")
	if all, _ := db.WebhookDeliveries().List(10, ""); len(all) != 0 {
		t.Fatalf("an event with no known org created %d deliveries", len(all))
	}
}

func eventsServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	return newTestServer(t)
}

func postEvents(t *testing.T, h http.Handler, runID quarrycontracts.ID, evs ...quarrycontracts.Event) {
	t.Helper()
	raw, _ := json.Marshal(evs)
	req := httptest.NewRequest(http.MethodPost, "/v1/runs/"+string(runID)+"/events", bytes.NewReader(raw))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusCreated {
		t.Fatalf("post events = %d: %s", w.Code, w.Body.String())
	}
}

// TestFanout_RunEventUsesTheRunsOrg: a job event of org B reaches B's subscription only,
// and an event posted for a run no job owns reaches nobody.
func TestFanout_RunEventUsesTheRunsOrg(t *testing.T) {
	t.Parallel()
	h, db := eventsServer(t)
	runB := quarrycontracts.NewID(quarrycontracts.KindRun)
	if err := db.Jobs().Create(store.Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob), OrgID: "org_b", Kind: "crawl", Status: "running", RunID: &runB}); err != nil {
		t.Fatal(err)
	}
	seedSub(t, db, "whk_a", "org_a", []string{"*"}, true)
	seedSub(t, db, "whk_b", "org_b", []string{"*"}, true)
	seedSub(t, db, "whk_legacy", "", []string{"*"}, true)

	postEvents(t, h, runB, quarrycontracts.Event{Type: quarrycontracts.EvtRunCompleted})
	for id, want := range map[string]int{"whk_a": 0, "whk_b": 1, "whk_legacy": 0} {
		if got := len(deliveriesFor(db, id)); got != want {
			t.Errorf("%s got %d deliveries, want %d", id, got, want)
		}
	}

	orphan := quarrycontracts.NewID(quarrycontracts.KindRun)
	postEvents(t, h, orphan, quarrycontracts.Event{Type: quarrycontracts.EvtRunCompleted})
	if got := len(deliveriesFor(db, "whk_b")) + len(deliveriesFor(db, "whk_a")); got != 1 {
		t.Fatalf("an event of a run no job owns created deliveries: total %d, want 1", got)
	}
}

// TestFanout_OneChangeReachedByTwoPathsIsDeliveredOnce: the same change arrives as the
// edge's signed webhook and as a run event with another idempotency key and event id.
func TestFanout_OneChangeReachedByTwoPathsIsDeliveredOnce(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	verifier := httpx.NewHMACVerifier(testSecret, true)
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountEvents(r, db, "", nil)
	r.Group(func(r chi.Router) {
		r.Use(verifier.Middleware)
		MountChangeWebhook(r, db, nil)
	})
	seedSub(t, db, "whk_a", "o1", []string{"change_detected"}, true)
	run := quarrycontracts.NewID(quarrycontracts.KindRun)
	if err := db.Jobs().Create(store.Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob), OrgID: "o1", Kind: "crawl", Status: "running", RunID: &run}); err != nil {
		t.Fatal(err)
	}

	if code, _ := doChange(t, r, signedChangePost(t, "o1", changedBody, "idem-two-paths", 0, "nonce-two")); code != http.StatusOK {
		t.Fatalf("webhook path = %d", code)
	}
	// The run-event path carries the same url and baseline under the orchestrator's shape.
	postEvents(t, r, run, quarrycontracts.Event{
		Type:           quarrycontracts.EvtChangeDetected,
		IdempotencyKey: "a-different-key",
		Payload:        map[string]any{"url": "https://example.com/pricing", "org_id": "o1", "baseline_id": "bln_01NEW"},
	})
	if got := len(deliveriesFor(db, "whk_a")); got != 1 {
		t.Fatalf("one change produced %d deliveries, want 1", got)
	}
	// A different baseline is a different change.
	postEvents(t, r, run, quarrycontracts.Event{
		Type:           quarrycontracts.EvtChangeDetected,
		IdempotencyKey: "another-key",
		Payload:        map[string]any{"url": "https://example.com/pricing", "org_id": "o1", "baseline_id": "bln_02NEXT"},
	})
	if got := len(deliveriesFor(db, "whk_a")); got != 2 {
		t.Fatalf("a second change produced %d deliveries in total, want 2", got)
	}
}
