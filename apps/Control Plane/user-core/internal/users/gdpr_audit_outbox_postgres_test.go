package users

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

func TestGDPRUserAuditOutboxPostgresLifecycle(t *testing.T) {
	pool := newIsolatedSchemaPool(t, "user_audit_outbox_")
	ctx := context.Background()

	for _, migrationPath := range []string{
		"../../migrations/014_gdpr_audit_outbox.up.sql",
		"../../migrations/015_gdpr_erasure_saga.up.sql",
		"../../migrations/016_gdpr_erasure_fanout.up.sql",
	} {
		migration, err := os.ReadFile(migrationPath)
		if err != nil {
			t.Fatalf("read migration %s: %v", migrationPath, err)
		}
		if _, err := pool.Exec(ctx, string(migration)); err != nil {
			t.Fatalf("apply migration %s: %v", migrationPath, err)
		}
	}
	repo := NewRepository(&database.DB{Pool: pool})
	original := AuditOutboxRow{EventID: "gdpr:user-core:postgres-fixture", Subject: ErasureAuditSubject, Payload: []byte(`{"event_id":"gdpr:user-core:postgres-fixture"}`)}
	if err := repo.EnqueueAudit(ctx, original); err != nil {
		t.Fatalf("enqueue: %v", err)
	}

	claimed, found, err := repo.ClaimAudit(ctx)
	if err != nil || !found {
		t.Fatalf("first claim found=%v err=%v", found, err)
	}
	if claimed.EventID != original.EventID || claimed.Attempts != 1 {
		t.Fatalf("first claim = %+v", claimed)
	}
	if err := repo.FailAudit(ctx, claimed.EventID, claimed.Attempts, time.Now().Add(-time.Second), "fixture failure", false); err != nil {
		t.Fatalf("record retry: %v", err)
	}

	claimed, found, err = repo.ClaimAudit(ctx)
	if err != nil || !found || claimed.Attempts != 2 {
		t.Fatalf("retry claim = %+v found=%v err=%v", claimed, found, err)
	}
	if err := repo.CompleteAudit(ctx, claimed.EventID, claimed.Attempts); err != nil {
		t.Fatalf("complete: %v", err)
	}
	var payload string
	var purged bool
	if err := pool.QueryRow(ctx, `SELECT payload::text, payload_purged_at IS NOT NULL FROM user_audit_outbox WHERE event_id=$1`, claimed.EventID).Scan(&payload, &purged); err != nil {
		t.Fatalf("read completed payload lifecycle: %v", err)
	}
	if !strings.Contains(payload, "postgres-fixture") || purged {
		t.Fatalf("completed audit evidence must remain queryable after PubAck: payload=%s purged=%v", payload, purged)
	}
	if _, found, err := repo.ClaimAudit(ctx); err != nil || found {
		t.Fatalf("completed row was reclaimed: found=%v err=%v", found, err)
	}

	terminal := AuditOutboxRow{EventID: "gdpr:user-core:terminal-fixture", Subject: ErasureAuditSubject, Payload: []byte(`{"subject_id":"sensitive-user"}`)}
	if err := repo.EnqueueAudit(ctx, terminal); err != nil {
		t.Fatalf("enqueue terminal fixture: %v", err)
	}
	claimed, found, err = repo.ClaimAudit(ctx)
	if err != nil || !found {
		t.Fatalf("claim terminal fixture found=%v err=%v", found, err)
	}
	if err := repo.FailAudit(ctx, claimed.EventID, claimed.Attempts, time.Now(), "permanent failure", true); err != nil {
		t.Fatalf("terminal failure: %v", err)
	}
	if err := pool.QueryRow(ctx, `SELECT payload::text, payload_purged_at IS NOT NULL FROM user_audit_outbox WHERE event_id=$1`, claimed.EventID).Scan(&payload, &purged); err != nil {
		t.Fatalf("read terminal payload lifecycle: %v", err)
	}
	if !strings.Contains(payload, "sensitive-user") || purged {
		t.Fatalf("terminal audit evidence must remain reconstructable: payload=%s purged=%v", payload, purged)
	}
}
