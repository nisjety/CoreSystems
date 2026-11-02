package users

import (
	"context"
	"os"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// WS03-S1: EnqueueAuditTx writes the audit outbox row on the caller's
// transaction. Validation is the dispatching path's validateAuditOutboxRow, and
// it runs before the transaction is touched, so a foreign subject can never be
// queued from a Space policy change.

const spacePolicyAuditSubject = "verevon.audit.v2.control.user-core.space_effect_policy_updated"

func TestEnqueueAuditTxRejectsASubjectOutsideUserCoreAuthority(t *testing.T) {
	// A nil tx proves validation runs first: a tx would panic on Exec.
	repo := &Repository{}
	for name, row := range map[string]AuditOutboxRow{
		"another producer":      {EventID: "e1", Subject: "verevon.audit.v2.control.auth-core.login", Payload: []byte(`{}`)},
		"another plane":         {EventID: "e1", Subject: "verevon.audit.v2.model.session-core.thing", Payload: []byte(`{}`)},
		"unprefixed":            {EventID: "e1", Subject: "space_effect_policy_updated", Payload: []byte(`{}`)},
		"empty subject":         {EventID: "e1", Subject: "", Payload: []byte(`{}`)},
		"blank event id":        {EventID: "  ", Subject: spacePolicyAuditSubject, Payload: []byte(`{}`)},
		"oversized event id":    {EventID: strings.Repeat("e", 129), Subject: spacePolicyAuditSubject, Payload: []byte(`{}`)},
		"payload is not json":   {EventID: "e1", Subject: spacePolicyAuditSubject, Payload: []byte(`not json`)},
		"payload empty":         {EventID: "e1", Subject: spacePolicyAuditSubject, Payload: nil},
		"subject prefix prefix": {EventID: "e1", Subject: "verevon.audit.v2.control.user-core", Payload: []byte(`{}`)},
	} {
		if err := repo.EnqueueAuditTx(context.Background(), nil, row); err == nil {
			t.Errorf("%s: EnqueueAuditTx accepted the row", name)
		}
	}
}

func TestSpaceAuditEnqueuerFailsClosedWhenUnwired(t *testing.T) {
	for name, enqueuer := range map[string]*SpaceAuditEnqueuer{
		"nil adapter":    nil,
		"nil repository": NewSpaceAuditEnqueuer(nil),
	} {
		err := enqueuer.EnqueueAuditTx(context.Background(), nil, "space-effect-policy:1", spacePolicyAuditSubject, []byte(`{}`))
		if err == nil || !strings.Contains(err.Error(), "unavailable") {
			t.Errorf("%s: err = %v, want an unavailable error", name, err)
		}
	}
}

func TestSpaceAuditEnqueuerValidatesLikeTheDispatchingPath(t *testing.T) {
	enqueuer := NewSpaceAuditEnqueuer(&Repository{})
	err := enqueuer.EnqueueAuditTx(context.Background(), nil, "space-effect-policy:1", "verevon.audit.v2.control.auth-core.x", []byte(`{}`))
	if err == nil || !strings.Contains(err.Error(), "outside user-core authority") {
		t.Fatalf("foreign subject via the Space adapter: err = %v", err)
	}
}

// Live: joins the caller's transaction (rolls back with it) and is idempotent
// on event_id. Gated on TEST_DATABASE_URL (isolated schema, migrations 014-016).
func TestEnqueueAuditTxJoinsTheCallersTransactionLive(t *testing.T) {
	pool := newIsolatedSchemaPool(t, "audit_enqueue_tx_")
	ctx := context.Background()
	for _, migrationPath := range []string{"../../migrations/014_gdpr_audit_outbox.up.sql"} {
		migration, err := os.ReadFile(migrationPath)
		if err != nil {
			t.Fatalf("read migration %s: %v", migrationPath, err)
		}
		if _, err := pool.Exec(ctx, string(migration)); err != nil {
			t.Fatalf("apply migration %s: %v", migrationPath, err)
		}
	}
	repo := NewRepository(&database.DB{Pool: pool})
	count := func(eventID string) int {
		var n int
		if err := pool.QueryRow(ctx, `SELECT count(*) FROM user_audit_outbox WHERE event_id = $1`, eventID).Scan(&n); err != nil {
			t.Fatalf("count: %v", err)
		}
		return n
	}
	row := AuditOutboxRow{EventID: "space-effect-policy:live-1", Subject: spacePolicyAuditSubject, Payload: []byte(`{"event_id":"space-effect-policy:live-1"}`)}

	// Rolled back with the caller's transaction.
	tx, err := pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin: %v", err)
	}
	if err := repo.EnqueueAuditTx(ctx, tx, row); err != nil {
		t.Fatalf("EnqueueAuditTx: %v", err)
	}
	if err := tx.Rollback(ctx); err != nil {
		t.Fatalf("rollback: %v", err)
	}
	if got := count(row.EventID); got != 0 {
		t.Fatalf("a rolled back caller transaction left %d outbox rows", got)
	}

	// Committed with it, and a replay of the same event id inserts nothing more.
	for i := 0; i < 2; i++ {
		tx, err := pool.Begin(ctx)
		if err != nil {
			t.Fatalf("begin: %v", err)
		}
		if err := repo.EnqueueAuditTx(ctx, tx, row); err != nil {
			t.Fatalf("EnqueueAuditTx: %v", err)
		}
		if err := tx.Commit(ctx); err != nil {
			t.Fatalf("commit: %v", err)
		}
	}
	if got := count(row.EventID); got != 1 {
		t.Fatalf("committed outbox rows = %d, want exactly 1 (ON CONFLICT DO NOTHING)", got)
	}
}
