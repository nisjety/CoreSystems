package org

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// TestControlLifecyclePurgeSkipsHeldOrganizations proves F-f10-06 against a disposable
// Postgres (migration 037): with ERASURE_HOLD_CHECK on, the 30-day purge skips an
// organisation with an active org or thread hold, purges one whose only hold was
// released, names the skipped ones in held_org_ids, and leaves one content-free refusal
// row per skipped organisation in the retention outbox. With the switch off the purge
// is 003's: every due organisation goes and the receipt has 003's keys only.
func TestControlLifecyclePurgeSkipsHeldOrganizations(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(
		ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID"),
	); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	repo := NewRepository(db)
	holds := retention.NewRepository(db)
	compliance := retention.Actor{ID: "u-comp", Role: "compliance"}
	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	const holdRef = "Matter purge-hold"

	seed := func(label string) string {
		t.Helper()
		orgID := "org-purge-hold-" + label + "-" + suffix
		if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
			ID: orgID, Name: "Purge Hold " + label, Plan: "free", Status: "active",
		}, "owner-purge-hold-"+label+"-"+suffix); err != nil {
			t.Fatalf("provision %s: %v", label, err)
		}
		return orgID
	}
	place := func(orgID, scope, target string) retention.Hold {
		t.Helper()
		var raw [16]byte
		if _, err := rand.Read(raw[:]); err != nil {
			t.Fatalf("hold id: %v", err)
		}
		id := "lh_" + hex.EncodeToString(raw[:])
		result, err := holds.PlaceHold(ctx, orgID, compliance, retention.Hold{
			ID: id, OrganizationID: orgID, Scope: scope, TargetRef: target, HoldRef: holdRef, CreatedBy: compliance.ID,
		})
		if err != nil {
			t.Fatalf("place %s hold on %s: %v", scope, orgID, err)
		}
		return result.Hold
	}
	backdateDeletion := func(ids ...string) {
		t.Helper()
		if _, err := db.Pool.Exec(ctx, `
UPDATE organizations SET deleted_at = NOW() - INTERVAL '40 days', status = 'deleted'
WHERE id = ANY($1)`, ids); err != nil {
			t.Fatalf("backdate soft delete: %v", err)
		}
	}
	exists := func(orgID string) bool {
		t.Helper()
		var found bool
		if err := db.Pool.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM organizations WHERE id = $1)`, orgID).Scan(&found); err != nil {
			t.Fatalf("read organization %s: %v", orgID, err)
		}
		return found
	}
	receiptOf := func(raw json.RawMessage) map[string]json.RawMessage {
		t.Helper()
		var fields map[string]json.RawMessage
		if err := json.Unmarshal(raw, &fields); err != nil {
			t.Fatalf("decode receipt %s: %v", raw, err)
		}
		if string(fields["success"]) != "true" {
			t.Fatalf("purge did not succeed: %s", raw)
		}
		return fields
	}
	idsOf := func(raw json.RawMessage) map[string]bool {
		t.Helper()
		var ids []string
		if len(raw) > 0 && string(raw) != "null" {
			if err := json.Unmarshal(raw, &ids); err != nil {
				t.Fatalf("decode ids %s: %v", raw, err)
			}
		}
		set := map[string]bool{}
		for _, id := range ids {
			set[id] = true
		}
		return set
	}

	orgHeld := seed("org")
	threadHeld := seed("thread")
	released := seed("released")
	place(orgHeld, retention.ScopeOrg, orgHeld)
	place(threadHeld, retention.ScopeThread, "thr-1")
	gone := place(released, retention.ScopeOrg, released)
	if _, err := holds.ReleaseHold(ctx, released, gone.ID, compliance); err != nil {
		t.Fatalf("release hold: %v", err)
	}
	backdateDeletion(orgHeld, threadHeld, released)

	// Switch on.
	t.Setenv(envErasureHoldCheck, "true")
	svc := NewService(repo, nil)
	svc.SetErasureRefusalAuditor(retention.NewService(holds, nil, nil))
	raw, err := svc.PurgeDeletedOrganizations(ctx, 30)
	if err != nil {
		t.Fatalf("purge with the hold check on: %v", err)
	}
	fields := receiptOf(raw)
	held, purged := idsOf(fields["held_org_ids"]), idsOf(fields["org_ids"])
	if !held[orgHeld] || !held[threadHeld] || held[released] {
		t.Fatalf("held_org_ids must name the org- and thread-held orgs only: %s", fields["held_org_ids"])
	}
	if !purged[released] || purged[orgHeld] || purged[threadHeld] {
		t.Fatalf("org_ids must name the released org and no held one: %s", fields["org_ids"])
	}
	if !exists(orgHeld) || !exists(threadHeld) || exists(released) {
		t.Fatalf("after the purge: org-held=%v thread-held=%v released=%v; want true true false",
			exists(orgHeld), exists(threadHeld), exists(released))
	}
	var activeHolds int
	if err := db.Pool.QueryRow(ctx, `
SELECT count(*) FROM org_legal_holds WHERE org_id = ANY($1) AND released_at IS NULL`,
		[]string{orgHeld, threadHeld}).Scan(&activeHolds); err != nil || activeHolds != 2 {
		t.Fatalf("the held orgs' holds must survive the purge: %d (err=%v)", activeHolds, err)
	}
	for orgID, counts := range map[string][2]string{orgHeld: {"1", "0"}, threadHeld: {"0", "1"}} {
		var rows, leaked int
		var actor, op, orgHolds, threadHolds string
		if err := db.Pool.QueryRow(ctx, `
SELECT count(*), count(*) FILTER (WHERE payload::text LIKE '%' || $3 || '%' OR payload::text LIKE '%hold_ref%'),
       COALESCE(max(payload->>'user_id'), ''), COALESCE(max(payload->'details'->>'op'), ''),
       COALESCE(max(payload->'details'->>'org_holds'), ''), COALESCE(max(payload->'details'->>'thread_holds'), '')
FROM org_retention_audit_outbox
WHERE org_id = $1 AND subject = $2 AND payload->'details'->>'kind' = 'erasure_refused'`,
			orgID, retention.AuditSubject, holdRef).Scan(&rows, &leaked, &actor, &op, &orgHolds, &threadHolds); err != nil {
			t.Fatalf("read refusal rows for %s: %v", orgID, err)
		}
		if rows != 1 || leaked != 0 || actor != gdprPurgeCronActorID || op != retention.ErasureOpPurge ||
			orgHolds != counts[0] || threadHolds != counts[1] {
			t.Fatalf("%s: rows=%d leaked=%d actor=%q op=%q counts=%s/%s; want one content-free cron refusal with %v",
				orgID, rows, leaked, actor, op, orgHolds, threadHolds, counts)
		}
	}

	// Switch off: 003's purge. The held orgs go, holds and all, and the receipt has
	// 003's keys only.
	t.Setenv(envErasureHoldCheck, "")
	raw, err = repo.PurgeOldDeletedOrganizations(ctx, 30)
	if err != nil {
		t.Fatalf("purge with the hold check off: %v", err)
	}
	fields = receiptOf(raw)
	keys := make([]string, 0, len(fields))
	for key := range fields {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	if got := strings.Join(keys, ","); got != "days_threshold,org_ids,purged_at,purged_count,success" {
		t.Fatalf("switch off: receipt keys = %s; want 003's", got)
	}
	if purged := idsOf(fields["org_ids"]); !purged[orgHeld] || !purged[threadHeld] {
		t.Fatalf("switch off: the purge must be 003's and take the held orgs: %s", fields["org_ids"])
	}
	if exists(orgHeld) || exists(threadHeld) {
		t.Fatal("switch off: the held orgs must be purged as before")
	}
}
