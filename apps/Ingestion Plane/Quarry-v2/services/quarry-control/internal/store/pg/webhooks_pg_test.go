package pg

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// The Postgres store answers the same webhook tenancy contract as the in-memory one. Needs
// QUARRY_TEST_DSN (see openIsolatedOrSkip); the schema is private to the test.
func TestPostgres_WebhooksConformance(t *testing.T) {
	storetest.WebhooksConformance(t, func(t *testing.T) store.DB { return openIsolatedOrSkip(t) })
}

// Deleting a subscription removes its deliveries (webhook_deliveries.webhook_id ON DELETE CASCADE).
func TestPostgres_DeleteSubscriptionCascadesDeliveries(t *testing.T) {
	db := openIsolatedOrSkip(t)
	wh := store.Webhook{
		ID: quarrycontracts.NewID(quarrycontracts.KindWebhook), OrgID: "org_c", URL: "https://c.example", Secret: "s",
		Events: []string{"change_detected"}, Active: true, CreatedAt: time.Now().Unix(),
	}
	if err := db.Webhooks().CreateCapped(wh, 10); err != nil {
		t.Fatal(err)
	}
	d := store.WebhookDelivery{
		ID: quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery), WebhookID: wh.ID, OrgID: "org_c",
		EventID: quarrycontracts.NewID(quarrycontracts.KindEvent), Status: "pending", CreatedAt: time.Now().Unix(),
	}
	if err := db.WebhookDeliveries().Create(d); err != nil {
		t.Fatal(err)
	}
	if err := db.Webhooks().DeleteByOrg(wh.ID, "org_c"); err != nil {
		t.Fatal(err)
	}
	if _, ok := db.WebhookDeliveries().Get(d.ID); ok {
		t.Fatal("a delivery outlived its subscription")
	}
}

// TestMigration025_KeepsLegacyRows re-applies 025 over a database that holds a legacy
// (org-less) webhook and a delivery: nothing is lost, the legacy row stays org-less, and it
// is still invisible to every tenant read. Re-running is a no-op.
func TestMigration025_KeepsLegacyRows(t *testing.T) {
	db := openIsolatedOrSkip(t)
	p := db.(*postgresDB)
	ctx := context.Background()
	if _, err := p.pool.Exec(ctx,
		`INSERT INTO webhooks(id, url, secret, events, active, created_at) VALUES ('whk_legacy025','https://legacy.example','s','{*}',true,1)`); err != nil {
		t.Fatalf("legacy insert: %v", err)
	}
	if _, err := p.pool.Exec(ctx,
		`INSERT INTO webhook_deliveries(id, webhook_id, event_id, status, created_at) VALUES ('whkd_legacy025','whk_legacy025','evt_1','pending',1)`); err != nil {
		t.Fatalf("legacy delivery insert: %v", err)
	}
	sqlBytes, err := os.ReadFile("migrations/025_webhook_subscriptions.sql")
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		if _, err := p.pool.Exec(ctx, strings.ReplaceAll(string(sqlBytes), "\r\n", "\n")); err != nil {
			t.Fatalf("re-apply %d: %v", i+1, err)
		}
	}
	got, ok := db.Webhooks().Get("whk_legacy025")
	if !ok || got.OrgID != "" || got.URL != "https://legacy.example" {
		t.Fatalf("legacy row after 025 = %+v %v", got, ok)
	}
	if rows := db.Webhooks().ListActiveByOrg("", 10); len(rows) != 0 {
		t.Fatalf("a legacy row is visible to the fan-out read: %+v", rows)
	}
	if _, ok := db.Webhooks().GetByOrg("whk_legacy025", ""); ok {
		t.Fatal("a legacy row is readable with an empty org")
	}
	if d, ok := db.WebhookDeliveries().Get("whkd_legacy025"); !ok || d.OrgID != "" {
		t.Fatalf("legacy delivery after 025 = %+v %v", d, ok)
	}
}
