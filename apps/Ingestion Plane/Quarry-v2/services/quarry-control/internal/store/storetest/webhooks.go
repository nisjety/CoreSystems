package storetest

import (
	"errors"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// WebhooksConformance runs the tenancy contract of the webhook tables (INNH-5) against any
// store.DB, so the in-memory and the Postgres stores answer the same questions the same way:
// every tenant read and write is scoped to the org, a row of another org looks missing, an
// empty org matches nothing, the per-org cap and the duplicate check are atomic with the
// insert, a delivery dedupe key is unique per subscription, the retention sweep deletes only
// old terminal rows, and org erasure removes an org's subscriptions and deliveries.
func WebhooksConformance(t *testing.T, newDB func(t *testing.T) store.DB) {
	t.Helper()
	db := newDB(t)
	suffix := string(quarrycontracts.NewID(quarrycontracts.KindRun))
	orgA, orgB := "org_wh_a_"+suffix, "org_wh_b_"+suffix
	now := time.Now().Unix()

	mk := func(org, url string, events []string, active bool, at int64) store.Webhook {
		return store.Webhook{
			ID: quarrycontracts.NewID(quarrycontracts.KindWebhook), OrgID: org, URL: url, Secret: "whsec_x", SecretHint: "whsec_...abcd",
			Events: events, Active: active, Description: "d", CreatedBy: "usr_1", CreatedAt: at,
		}
	}
	ev := []string{"change_detected"}

	// --- create: cap, duplicate, org required --------------------------------------------
	a1 := mk(orgA, "https://a.example/1", ev, true, now-30)
	a2 := mk(orgA, "https://a.example/2", ev, true, now-20)
	a3 := mk(orgA, "https://a.example/3", ev, false, now-10)
	for _, w := range []store.Webhook{a1, a2} {
		if err := db.Webhooks().CreateCapped(w, 3); err != nil {
			t.Fatalf("CreateCapped: %v", err)
		}
	}
	if err := db.Webhooks().CreateCapped(mk(orgA, "https://a.example/1", ev, true, now), 3); !errors.Is(err, store.ErrConflict) {
		t.Fatalf("same url and events = %v, want ErrConflict", err)
	}
	if err := db.Webhooks().CreateCapped(mk(orgA, "https://a.example/1", []string{"*"}, true, now), 3); err != nil {
		t.Fatalf("same url, other events must be allowed: %v", err)
	}
	if err := db.Webhooks().CreateCapped(mk(orgA, "https://a.example/over", ev, true, now), 3); !errors.Is(err, store.ErrQuotaExceeded) {
		t.Fatalf("over the cap = %v, want ErrQuotaExceeded", err)
	}
	if err := db.Webhooks().CreateCapped(mk("", "https://nobody.example", ev, true, now), 3); err == nil {
		t.Fatal("CreateCapped accepted a subscription with no org")
	}
	// The cap counts one org: org B is empty.
	b1 := mk(orgB, "https://b.example/1", ev, true, now-5)
	if err := db.Webhooks().CreateCapped(b1, 3); err != nil {
		t.Fatalf("org B create: %v", err)
	}
	// An inactive row still counts toward the cap (3 rows in org A now).
	if n := db.Webhooks().CountByOrg(orgA); n != 3 {
		t.Fatalf("CountByOrg(A) = %d, want 3", n)
	}
	if n := db.Webhooks().CountByOrg(""); n != 0 {
		t.Fatalf("CountByOrg(\"\") = %d, want 0", n)
	}
	_ = a3

	// --- reads are scoped ------------------------------------------------------------------
	if got, ok := db.Webhooks().GetByOrg(a1.ID, orgA); !ok || got.URL != a1.URL || got.Secret != "whsec_x" || got.SecretHint != "whsec_...abcd" || got.CreatedBy != "usr_1" {
		t.Fatalf("GetByOrg(A) = %+v %v", got, ok)
	}
	if _, ok := db.Webhooks().GetByOrg(a1.ID, orgB); ok {
		t.Fatal("org B read org A's subscription")
	}
	if _, ok := db.Webhooks().GetByOrg(a1.ID, ""); ok {
		t.Fatal("an empty org read a subscription")
	}
	page1, next := db.Webhooks().ListByOrg(orgA, 2, "")
	if len(page1) != 2 || next == "" {
		t.Fatalf("page 1 = %d rows, next %q", len(page1), next)
	}
	page2, next2 := db.Webhooks().ListByOrg(orgA, 2, next)
	if len(page2) != 1 || next2 != "" {
		t.Fatalf("page 2 = %d rows, next %q", len(page2), next2)
	}
	for _, w := range append(page1, page2...) {
		if w.OrgID != orgA {
			t.Fatalf("ListByOrg(A) returned a row of %q", w.OrgID)
		}
	}
	if rows, _ := db.Webhooks().ListByOrg("", 10, ""); len(rows) != 0 {
		t.Fatal("ListByOrg(\"\") returned rows")
	}

	// --- active list is the fan-out read --------------------------------------------------
	legacy := store.Webhook{ID: quarrycontracts.NewID(quarrycontracts.KindWebhook), URL: "https://legacy.example", Secret: "s", Events: []string{"*"}, Active: true, CreatedAt: now}
	if err := db.Webhooks().Create(legacy); err != nil {
		t.Fatalf("legacy create: %v", err)
	}
	active := db.Webhooks().ListActiveByOrg(orgA, 100)
	if len(active) != 3 { // a1, a2 and the wildcard twin of a1; a3 was never created (cap)
		t.Fatalf("ListActiveByOrg(A) = %d rows, want 3", len(active))
	}
	for _, w := range active {
		if w.OrgID != orgA || !w.Active {
			t.Fatalf("ListActiveByOrg returned %+v", w)
		}
	}
	if rows := db.Webhooks().ListActiveByOrg("", 100); len(rows) != 0 {
		t.Fatalf("ListActiveByOrg(\"\") returned %d rows: a legacy row would receive events", len(rows))
	}

	// --- update and delete are scoped -----------------------------------------------------
	upd := a1
	upd.Active, upd.Secret, upd.SecretHint, upd.Description, upd.Events = false, "whsec_new", "whsec_...wxyz", "changed", []string{"change_detected"}
	if err := db.Webhooks().UpdateByOrg(upd); err != nil {
		t.Fatalf("UpdateByOrg: %v", err)
	}
	if got, _ := db.Webhooks().GetByOrg(a1.ID, orgA); got.Active || got.Secret != "whsec_new" || got.Description != "changed" || got.URL != a1.URL {
		t.Fatalf("after update = %+v", got)
	}
	foreign := a1
	foreign.OrgID = orgB
	if err := db.Webhooks().UpdateByOrg(foreign); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("UpdateByOrg with the wrong org = %v, want ErrNotFound", err)
	}
	if err := db.Webhooks().DeleteByOrg(a1.ID, orgB); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("DeleteByOrg with the wrong org = %v, want ErrNotFound", err)
	}
	if _, ok := db.Webhooks().GetByOrg(a1.ID, orgA); !ok {
		t.Fatal("a foreign DeleteByOrg removed the row")
	}

	// --- deliveries ------------------------------------------------------------------------
	dl := func(wh store.Webhook, org, status string, at int64, dedupe string) store.WebhookDelivery {
		return store.WebhookDelivery{
			ID: quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery), WebhookID: wh.ID, OrgID: org,
			EventID: quarrycontracts.NewID(quarrycontracts.KindEvent), EventType: "change_detected", DedupeKey: dedupe,
			Payload: `{"x":1}`, Status: status, NextAttemptAt: at, CreatedAt: at,
		}
	}
	old := now - 40*86400
	dOldSuccess := dl(a2, orgA, "success", old, "")
	dOldDlq := dl(a2, orgA, "dlq", old, "")
	dOldPending := dl(a2, orgA, "pending", old, "")
	dNewSuccess := dl(a2, orgA, "success", now, "k1")
	dB := dl(b1, orgB, "dlq", old, "")
	for _, d := range []store.WebhookDelivery{dOldSuccess, dOldDlq, dOldPending, dNewSuccess, dB} {
		if err := db.WebhookDeliveries().Create(d); err != nil {
			t.Fatalf("delivery create: %v", err)
		}
	}
	if err := db.WebhookDeliveries().Create(dl(a2, orgA, "pending", now, "k1")); !errors.Is(err, store.ErrConflict) {
		t.Fatalf("same dedupe key on the same subscription = %v, want ErrConflict", err)
	}
	if err := db.WebhookDeliveries().Create(dl(b1, orgB, "pending", now, "k1")); err != nil {
		t.Fatalf("the same dedupe key on another subscription must be allowed: %v", err)
	}
	if err := db.WebhookDeliveries().Create(dl(a2, orgA, "pending", now, "")); err != nil {
		t.Fatalf("empty dedupe keys must never collide: %v", err)
	}

	if _, ok := db.WebhookDeliveries().GetByOrg(dOldDlq.ID, orgB); ok {
		t.Fatal("org B read org A's delivery")
	}
	if got, ok := db.WebhookDeliveries().GetByOrg(dOldDlq.ID, orgA); !ok || got.EventType != "change_detected" {
		t.Fatalf("GetByOrg delivery = %+v %v", got, ok)
	}
	all, _ := db.WebhookDeliveries().ListByWebhook(orgA, a2.ID, "", 50, "")
	if len(all) != 5 { // old success, old dlq, old pending, new success k1, new pending
		t.Fatalf("ListByWebhook = %d rows, want 5", len(all))
	}
	if rows, _ := db.WebhookDeliveries().ListByWebhook(orgA, a2.ID, "dlq", 50, ""); len(rows) != 1 || rows[0].ID != dOldDlq.ID {
		t.Fatalf("status filter = %+v", rows)
	}
	if rows, _ := db.WebhookDeliveries().ListByWebhook(orgB, a2.ID, "", 50, ""); len(rows) != 0 {
		t.Fatal("org B listed org A's subscription's deliveries")
	}
	if rows, _ := db.WebhookDeliveries().ListByWebhook("", a2.ID, "", 50, ""); len(rows) != 0 {
		t.Fatal("an empty org listed deliveries")
	}

	// Retention: terminal rows older than the cutoff go; pending rows and new rows stay.
	removed, err := db.WebhookDeliveries().DeleteTerminalBefore(now - 14*86400)
	if err != nil {
		t.Fatalf("DeleteTerminalBefore: %v", err)
	}
	if removed != 3 { // old success, old dlq (A) and org B's old dlq
		t.Fatalf("DeleteTerminalBefore removed %d, want 3", removed)
	}
	if _, ok := db.WebhookDeliveries().Get(dOldPending.ID); !ok {
		t.Fatal("the sweep deleted a pending delivery")
	}
	if _, ok := db.WebhookDeliveries().Get(dNewSuccess.ID); !ok {
		t.Fatal("the sweep deleted a recent delivery")
	}
	if _, ok := db.WebhookDeliveries().Get(dOldSuccess.ID); ok {
		t.Fatal("the sweep kept an old terminal delivery")
	}

	// --- erasure ---------------------------------------------------------------------------
	res, err := db.PurgeOrg(orgA)
	if err != nil {
		t.Fatalf("PurgeOrg: %v", err)
	}
	if res.WebhooksDeleted != 3 {
		t.Errorf("WebhooksDeleted = %d, want 3", res.WebhooksDeleted)
	}
	if res.WebhookDeliveriesDeleted != 3 { // old pending, new success k1, new pending: the other two were swept
		t.Errorf("WebhookDeliveriesDeleted = %d, want 3", res.WebhookDeliveriesDeleted)
	}
	if n := db.Webhooks().CountByOrg(orgA); n != 0 {
		t.Fatalf("org A still holds %d subscriptions after erasure", n)
	}
	if _, ok := db.Webhooks().GetByOrg(b1.ID, orgB); !ok {
		t.Fatal("erasing org A removed org B's subscription")
	}
	if rows, _ := db.WebhookDeliveries().ListByWebhook(orgB, b1.ID, "", 50, ""); len(rows) != 1 {
		t.Fatalf("erasing org A touched org B's deliveries: %d left, want 1", len(rows))
	}
	if _, ok := db.Webhooks().Get(legacy.ID); !ok {
		t.Fatal("erasure removed a legacy row it does not own")
	}
	again, err := db.PurgeOrg(orgA)
	if err != nil || again.Total() != 0 {
		t.Fatalf("second erasure = %+v %v, want a no-op", again, err)
	}
	_, _ = db.PurgeOrg(orgB)
	_ = db.Webhooks().Delete(legacy.ID)
}
