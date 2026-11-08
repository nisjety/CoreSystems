package janitor

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func TestWebhookDeliveryRetentionDaysFromEnv(t *testing.T) {
	t.Parallel()
	env := func(v string) func(string) string {
		return func(k string) string {
			if k == "QUARRY_CONTROL__WEBHOOK_DELIVERY_RETENTION_DAYS" {
				return v
			}
			return ""
		}
	}
	for v, want := range map[string]struct {
		days    int
		invalid bool
	}{
		"":      {14, false},
		"30":    {30, false},
		" 7 ":   {7, false},
		"0":     {14, true},
		"-3":    {14, true},
		"abc":   {14, true},
		"99999": {14, true},
	} {
		days, invalid := WebhookDeliveryRetentionDaysFromEnv(env(v))
		if days != want.days || invalid != want.invalid {
			t.Errorf("%q = %d,%v want %d,%v", v, days, invalid, want.days, want.invalid)
		}
	}
}

func TestSweepWebhookDeliveries_DeletesOnlyOldTerminalRows(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	now := time.Now()
	add := func(status string, age time.Duration) quarrycontracts.ID {
		d := store.WebhookDelivery{
			ID: quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery), WebhookID: "whk_1", OrgID: "o1",
			EventID: quarrycontracts.NewID(quarrycontracts.KindEvent), Status: status, CreatedAt: now.Add(-age).Unix(),
		}
		if err := db.WebhookDeliveries().Create(d); err != nil {
			t.Fatal(err)
		}
		return d.ID
	}
	day := 24 * time.Hour
	oldSuccess, oldDlq, oldFailed := add("success", 20*day), add("dlq", 20*day), add("failed", 20*day)
	oldPending, oldInflight := add("pending", 20*day), add("in_flight", 20*day)
	newSuccess := add("success", 2*day)

	n, err := SweepWebhookDeliveries(db, 14, now)
	if err != nil || n != 3 {
		t.Fatalf("sweep = %d, %v; want 3 removed", n, err)
	}
	for _, id := range []quarrycontracts.ID{oldSuccess, oldDlq, oldFailed} {
		if _, ok := db.WebhookDeliveries().Get(id); ok {
			t.Errorf("%s survived the sweep", id)
		}
	}
	for _, id := range []quarrycontracts.ID{oldPending, oldInflight, newSuccess} {
		if _, ok := db.WebhookDeliveries().Get(id); !ok {
			t.Errorf("%s was deleted by the sweep", id)
		}
	}
}
