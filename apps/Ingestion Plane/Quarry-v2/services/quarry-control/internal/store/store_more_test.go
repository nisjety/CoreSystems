package store

import (
	"encoding/base64"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

func TestScheduleValidate_AssignsDefaults(t *testing.T) {
	t.Parallel()

	s := &Schedule{OrgID: "org_test", Cron: "*/5 * * * *", TargetRef: "https://example.com"}
	if err := s.Validate(); err != nil {
		t.Fatalf("validate: %v", err)
	}
	if s.ID == "" {
		t.Fatal("expected generated schedule id")
	}
	if s.CreatedAt == 0 {
		t.Fatal("expected created_at to be assigned")
	}
}

func TestScheduleValidate_RejectsInvalidCron(t *testing.T) {
	t.Parallel()

	s := &Schedule{Cron: "not-cron"}
	if err := s.Validate(); err == nil {
		t.Fatal("expected cron validation error")
	}
}

func TestMemoryDB_SchedulesUpdateEnabled(t *testing.T) {
	t.Parallel()

	db := NewMemory()
	id := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	s := Schedule{ID: id, Cron: "*/5 * * * *", Enabled: true, CreatedAt: time.Now().Unix()}
	if err := db.Schedules().Create(s); err != nil {
		t.Fatalf("create schedule: %v", err)
	}

	if err := db.Schedules().UpdateEnabled(id, false); err != nil {
		t.Fatalf("update enabled: %v", err)
	}
	got, ok := db.Schedules().Get(id)
	if !ok {
		t.Fatal("expected schedule to exist")
	}
	if got.Enabled {
		t.Fatal("expected schedule to be disabled")
	}

	missing := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	if err := db.Schedules().UpdateEnabled(missing, true); err != ErrNotFound {
		t.Fatalf("expected ErrNotFound, got %v", err)
	}
}

func TestMemoryDB_JobsListBySchedule_CursorAndLimit(t *testing.T) {
	t.Parallel()

	db := NewMemory()
	sid := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	other := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	now := time.Now().UnixMilli()

	for i := 0; i < 4; i++ {
		s := sid
		if i == 3 {
			s = other
		}
		j := Job{
			ID:         quarrycontracts.NewID(quarrycontracts.KindJob),
			Kind:       "crawl",
			Status:     "accepted",
			Policy:     quarrycontracts.DefaultRunPolicy(),
			ScheduleID: &s,
			CreatedAt:  now + int64(i),
		}
		if err := db.Jobs().Create(j); err != nil {
			t.Fatalf("create job %d: %v", i, err)
		}
	}

	page1, cursor := db.Jobs().ListBySchedule(sid, 2, "")
	if len(page1) != 2 {
		t.Fatalf("page1 len=%d want=2", len(page1))
	}
	if cursor == "" {
		t.Fatal("expected non-empty cursor for paged result")
	}

	page2, _ := db.Jobs().ListBySchedule(sid, 2, cursor)
	if len(page2) != 1 {
		t.Fatalf("page2 len=%d want=1", len(page2))
	}

	raw, err := base64.RawURLEncoding.DecodeString(cursor)
	if err != nil {
		t.Fatalf("decode cursor: %v", err)
	}
	if len(raw) == 0 {
		t.Fatal("cursor payload is empty")
	}
}

func TestMemoryDB_WebhookDeliveriesClaimDue_SortsAndLimits(t *testing.T) {
	t.Parallel()

	db := NewMemory()
	base := time.Now().Unix()
	whID := quarrycontracts.NewID(quarrycontracts.KindWebhook)

	fixtures := []WebhookDelivery{
		{ID: quarrycontracts.NewID(quarrycontracts.KindWebhook), WebhookID: whID, EventID: quarrycontracts.NewID(quarrycontracts.KindEvent), Status: "pending", NextAttemptAt: base + 20},
		{ID: quarrycontracts.NewID(quarrycontracts.KindWebhook), WebhookID: whID, EventID: quarrycontracts.NewID(quarrycontracts.KindEvent), Status: "pending", NextAttemptAt: base + 10},
		{ID: quarrycontracts.NewID(quarrycontracts.KindWebhook), WebhookID: whID, EventID: quarrycontracts.NewID(quarrycontracts.KindEvent), Status: "success", NextAttemptAt: base + 5},
	}
	for i, d := range fixtures {
		d.CreatedAt = base + int64(i)
		if err := db.WebhookDeliveries().Create(d); err != nil {
			t.Fatalf("create delivery %d: %v", i, err)
		}
	}

	claimed, err := db.WebhookDeliveries().ClaimDue(base+30, 1)
	if err != nil {
		t.Fatalf("claim due: %v", err)
	}
	if len(claimed) != 1 {
		t.Fatalf("claimed len=%d want=1", len(claimed))
	}
	if claimed[0].Status != "pending" {
		t.Fatalf("expected pending status, got %q", claimed[0].Status)
	}

	allClaimed, err := db.WebhookDeliveries().ClaimDue(base+30, 10)
	if err != nil {
		t.Fatalf("claim due all: %v", err)
	}
	if len(allClaimed) != 2 {
		t.Fatalf("claimed due len=%d want=2", len(allClaimed))
	}
	if allClaimed[0].NextAttemptAt > allClaimed[1].NextAttemptAt {
		t.Fatalf("expected sorted by next attempt: %v > %v", allClaimed[0].NextAttemptAt, allClaimed[1].NextAttemptAt)
	}
}
