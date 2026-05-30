package store

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

func TestMemoryDB_JobsCRUDAndList(t *testing.T) {
	t.Parallel()
	db := NewMemory()

	for i := 0; i < 3; i++ {
		j := Job{
			ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
			Kind:      "scrape",
			Status:    "accepted",
			Policy:    quarrycontracts.DefaultRunPolicy(),
			CreatedAt: time.Now().UnixMilli(),
		}
		if err := db.Jobs().Create(j); err != nil {
			t.Fatalf("create: %v", err)
		}
	}

	items, _ := db.Jobs().List(10, "")
	if len(items) != 3 {
		t.Fatalf("list len = %d, want 3", len(items))
	}

	got, ok := db.Jobs().Get(items[0].ID)
	if !ok || got.ID != items[0].ID {
		t.Fatalf("get mismatch: %v", got)
	}
}

func TestMemoryDB_DeleteRemovesItem(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	j := Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob), Kind: "scrape"}
	_ = db.Jobs().Create(j)
	if err := db.Jobs().Delete(j.ID); err != nil {
		t.Fatalf("delete: %v", err)
	}
	if _, ok := db.Jobs().Get(j.ID); ok {
		t.Fatal("expected missing after delete")
	}
	if err := db.Jobs().Delete(j.ID); err != ErrNotFound {
		t.Fatalf("expected ErrNotFound, got %v", err)
	}
}

func TestMemoryDB_CreateConflict(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	j := Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob)}
	if err := db.Jobs().Create(j); err != nil {
		t.Fatal(err)
	}
	if err := db.Jobs().Create(j); err != ErrConflict {
		t.Fatalf("expected ErrConflict, got %v", err)
	}
}

func TestEventLog_NextSeq_IsMonotonicPerRun(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	runA := quarrycontracts.NewID(quarrycontracts.KindRun)
	runB := quarrycontracts.NewID(quarrycontracts.KindRun)

	if n := db.Events().NextSeq(runA); n != 1 {
		t.Fatalf("first NextSeq on empty run = %d, want 1", n)
	}
	// Append two events for runA with seq 1 and 2.
	for _, seq := range []uint64{1, 2} {
		_ = db.Events().Append(quarrycontracts.Event{
			EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
			RunID:   &runA,
			Type:    quarrycontracts.EvtPageFetched,
			Seq:     seq,
		})
	}
	if n := db.Events().NextSeq(runA); n != 3 {
		t.Fatalf("NextSeq after seq=2 = %d, want 3", n)
	}
	// runB's counter should be independent.
	if n := db.Events().NextSeq(runB); n != 1 {
		t.Fatalf("NextSeq on untouched runB = %d, want 1", n)
	}
}

func TestMemoryDB_JobsUpdate(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	j := Job{
		ID:     quarrycontracts.NewID(quarrycontracts.KindJob),
		Kind:   "crawl",
		Status: "accepted",
	}
	if err := db.Jobs().Create(j); err != nil {
		t.Fatal(err)
	}
	// Update happy path: transition to running with a run_id.
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	j.Status = "running"
	j.RunID = &runID
	if err := db.Jobs().Update(j); err != nil {
		t.Fatalf("update: %v", err)
	}
	got, _ := db.Jobs().Get(j.ID)
	if got.Status != "running" || got.RunID == nil || *got.RunID != runID {
		t.Fatalf("after update: status=%s run_id=%v", got.Status, got.RunID)
	}
	// Update on unknown id returns ErrNotFound.
	unknown := Job{ID: quarrycontracts.NewID(quarrycontracts.KindJob), Status: "x"}
	if err := db.Jobs().Update(unknown); err != ErrNotFound {
		t.Fatalf("expected ErrNotFound for unknown id, got %v", err)
	}
}

func TestMemoryDB_JobsFindByIdempotencyKey(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	key := "velion-crawl-deadbeef"
	j := Job{
		ID:             quarrycontracts.NewID(quarrycontracts.KindJob),
		Kind:           "crawl",
		Status:         "accepted",
		IdempotencyKey: &key,
	}
	if err := db.Jobs().Create(j); err != nil {
		t.Fatal(err)
	}
	got, ok := db.Jobs().FindByIdempotencyKey(key)
	if !ok || got.ID != j.ID {
		t.Fatalf("FindByIdempotencyKey: ok=%v got=%v want=%v", ok, got.ID, j.ID)
	}
	if _, ok := db.Jobs().FindByIdempotencyKey("missing"); ok {
		t.Fatal("expected miss for unknown key")
	}
	// Empty key never matches.
	if _, ok := db.Jobs().FindByIdempotencyKey(""); ok {
		t.Fatal("expected empty key to never match")
	}
}

func TestEventLog_AppendAndForRun(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	run := quarrycontracts.NewID(quarrycontracts.KindRun)
	other := quarrycontracts.NewID(quarrycontracts.KindRun)

	for i, r := range []quarrycontracts.ID{run, run, other, run} {
		evt := quarrycontracts.Event{
			EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
			RunID:   &r,
			Type:    quarrycontracts.EvtPageFetched,
			Seq:     uint64(i + 1),
		}
		if err := db.Events().Append(evt); err != nil {
			t.Fatal(err)
		}
	}

	got := db.Events().ForRun(run, 0, 10)
	if len(got) != 3 {
		t.Fatalf("expected 3 events for run, got %d", len(got))
	}

	// after_seq filter
	after := db.Events().ForRun(run, got[0].Seq, 10)
	if len(after) != 2 {
		t.Fatalf("expected 2 after seq=%d, got %d", got[0].Seq, len(after))
	}
}

func TestMemoryDB_WebhooksCRUDAndList(t *testing.T) {
	t.Parallel()
	db := NewMemory()

	for i := 0; i < 3; i++ {
		w := Webhook{
			ID:        quarrycontracts.NewID(quarrycontracts.KindWebhook),
			URL:       "https://example.com/hook",
			Secret:    "s3cr3t",
			Events:    []string{"run.completed"},
			Active:    true,
			CreatedAt: time.Now().UnixMilli(),
		}
		if err := db.Webhooks().Create(w); err != nil {
			t.Fatalf("create: %v", err)
		}
	}

	items, _ := db.Webhooks().List(10, "")
	if len(items) != 3 {
		t.Fatalf("list len = %d, want 3", len(items))
	}

	got, ok := db.Webhooks().Get(items[0].ID)
	if !ok || got.ID != items[0].ID {
		t.Fatalf("get mismatch: %v", got)
	}
}

func TestMemoryDB_WebhooksCreateConflict(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	w := Webhook{ID: quarrycontracts.NewID(quarrycontracts.KindWebhook), URL: "https://x", Active: true}
	if err := db.Webhooks().Create(w); err != nil {
		t.Fatal(err)
	}
	if err := db.Webhooks().Create(w); err != ErrConflict {
		t.Fatalf("expected ErrConflict, got %v", err)
	}
}

func TestMemoryDB_WebhooksDeleteRemovesItem(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	w := Webhook{ID: quarrycontracts.NewID(quarrycontracts.KindWebhook), URL: "https://x"}
	_ = db.Webhooks().Create(w)
	if err := db.Webhooks().Delete(w.ID); err != nil {
		t.Fatalf("delete: %v", err)
	}
	if _, ok := db.Webhooks().Get(w.ID); ok {
		t.Fatal("expected missing after delete")
	}
	if err := db.Webhooks().Delete(w.ID); err != ErrNotFound {
		t.Fatalf("expected ErrNotFound, got %v", err)
	}
}

func TestMemoryDB_WebhookDeliveriesCRUDAndList(t *testing.T) {
	t.Parallel()
	db := NewMemory()

	whID := quarrycontracts.NewID(quarrycontracts.KindWebhook)
	for i := 0; i < 3; i++ {
		d := WebhookDelivery{
			ID:            quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery),
			WebhookID:     whID,
			EventID:       quarrycontracts.NewID(quarrycontracts.KindEvent),
			Attempt:       1,
			Status:        "pending",
			NextAttemptAt: time.Now().UnixMilli(),
			CreatedAt:     time.Now().UnixMilli(),
		}
		if err := db.WebhookDeliveries().Create(d); err != nil {
			t.Fatalf("create: %v", err)
		}
	}

	items, _ := db.WebhookDeliveries().List(10, "")
	if len(items) != 3 {
		t.Fatalf("list len = %d, want 3", len(items))
	}

	got, ok := db.WebhookDeliveries().Get(items[0].ID)
	if !ok || got.ID != items[0].ID {
		t.Fatalf("get mismatch: %v", got)
	}
}

func TestMemoryDB_WebhookDeliveriesCreateConflict(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	d := WebhookDelivery{
		ID:        quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery),
		WebhookID: quarrycontracts.NewID(quarrycontracts.KindWebhook),
		EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
		Status:    "pending",
	}
	if err := db.WebhookDeliveries().Create(d); err != nil {
		t.Fatal(err)
	}
	if err := db.WebhookDeliveries().Create(d); err != ErrConflict {
		t.Fatalf("expected ErrConflict, got %v", err)
	}
}

func TestMemoryDB_WebhookDeliveriesDeleteRemovesItem(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	d := WebhookDelivery{
		ID:        quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery),
		WebhookID: quarrycontracts.NewID(quarrycontracts.KindWebhook),
		EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
		Status:    "pending",
	}
	_ = db.WebhookDeliveries().Create(d)
	if err := db.WebhookDeliveries().Delete(d.ID); err != nil {
		t.Fatalf("delete: %v", err)
	}
	if _, ok := db.WebhookDeliveries().Get(d.ID); ok {
		t.Fatal("expected missing after delete")
	}
	if err := db.WebhookDeliveries().Delete(d.ID); err != ErrNotFound {
		t.Fatalf("expected ErrNotFound, got %v", err)
	}
}

func TestMemoryDB_BlocklistsCRUDAndList(t *testing.T) {
	t.Parallel()
	db := NewMemory()

	for i := 0; i < 3; i++ {
		b := BlocklistEntry{
			ID:        quarrycontracts.NewID(quarrycontracts.KindBlocklist),
			Pattern:   "*.evil.com",
			IsRegex:   false,
			CreatedAt: time.Now().UnixMilli(),
		}
		if err := db.Blocklists().Create(b); err != nil {
			t.Fatalf("create: %v", err)
		}
	}

	items, _ := db.Blocklists().List(10, "")
	if len(items) != 3 {
		t.Fatalf("list len = %d, want 3", len(items))
	}

	got, ok := db.Blocklists().Get(items[0].ID)
	if !ok || got.ID != items[0].ID {
		t.Fatalf("get mismatch: %v", got)
	}
}

func TestMemoryDB_BlocklistsCreateConflict(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	b := BlocklistEntry{ID: quarrycontracts.NewID(quarrycontracts.KindBlocklist), Pattern: "*.evil.com"}
	if err := db.Blocklists().Create(b); err != nil {
		t.Fatal(err)
	}
	if err := db.Blocklists().Create(b); err != ErrConflict {
		t.Fatalf("expected ErrConflict, got %v", err)
	}
}

func TestMemoryDB_BlocklistsDeleteRemovesItem(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	b := BlocklistEntry{ID: quarrycontracts.NewID(quarrycontracts.KindBlocklist), Pattern: "*.evil.com"}
	_ = db.Blocklists().Create(b)
	if err := db.Blocklists().Delete(b.ID); err != nil {
		t.Fatalf("delete: %v", err)
	}
	if _, ok := db.Blocklists().Get(b.ID); ok {
		t.Fatal("expected missing after delete")
	}
	if err := db.Blocklists().Delete(b.ID); err != ErrNotFound {
		t.Fatalf("expected ErrNotFound, got %v", err)
	}
}
