package store_test

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// A stuck Model Plane audit row is replayed by requeueing it in session-core's
// outbox: session-core republishes the same body (same event_id) as a NEW
// JetStream message, so audit-core sees it again under a new stream sequence.
// This is the safety property the replay procedure rests on: audit-core stores
// an event once per (source bus, producer, event_id), ignores an identical repeat
// and refuses a different payload under a used id.
//
// The bodies are session-core's real wire fixtures, not hand-written events.
func TestReplayedSessionCoreAuditEventIsStoredExactlyOnce(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping Postgres integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	if err := store.Migrate(ctx, pool); err != nil {
		t.Fatalf("migrate: %v", err)
	}
	st := store.New(pool)

	type fixture struct {
		Subject string          `json:"subject"`
		Body    json.RawMessage `json:"body"`
	}
	// The fixtures are vendored into this module (internal/testdata/session-core-wire,
	// pinned and compared with session-core's canonical copy by the subscriber
	// package's TestSessionCoreWireFixtures* tests), so this test reads nothing
	// outside the module's build context.
	dir := filepath.Join("..", "testdata", "session-core-wire")
	load := func(name string) (*events.AuditEvent, string) {
		raw, err := os.ReadFile(filepath.Join(dir, name))
		if err != nil {
			t.Fatalf("read fixture %s: %v", name, err)
		}
		var parsed fixture
		if err := json.Unmarshal(raw, &parsed); err != nil {
			t.Fatalf("fixture %s: %v", name, err)
		}
		event, err := events.DecodeAudit(parsed.Body)
		if err != nil {
			t.Fatalf("audit-core refuses fixture %s: %v", name, err)
		}
		return event, parsed.Subject
	}
	final, finalSubject := load("tool_action_final.json")
	reserved, reservedSubject := load("tool_action_reserved.json")

	const bus = "model"
	cleanup := func() {
		_, _ = pool.Exec(ctx, `DELETE FROM audit_events WHERE org_id = $1`, final.OrgID)
	}
	cleanup()
	t.Cleanup(cleanup)

	// First delivery of both phases.
	for _, delivery := range []struct {
		event   *events.AuditEvent
		subject string
		seq     uint64
	}{{final, finalSubject, 11}, {reserved, reservedSubject, 12}} {
		inserted, err := st.InsertAuditFromStream(ctx, delivery.event, bus, delivery.subject, delivery.seq)
		if err != nil || !inserted {
			t.Fatalf("first delivery of %s: inserted=%v err=%v", delivery.event.Event, inserted, err)
		}
	}

	// The replay: the same bodies as new stream messages (new sequences).
	for _, delivery := range []struct {
		event   *events.AuditEvent
		subject string
		seq     uint64
	}{{final, finalSubject, 5011}, {reserved, reservedSubject, 5012}} {
		inserted, err := st.InsertAuditFromStream(ctx, delivery.event, bus, delivery.subject, delivery.seq)
		if err != nil || inserted {
			t.Fatalf("replay of %s must be an acknowledged duplicate: inserted=%v err=%v", delivery.event.Event, inserted, err)
		}
	}
	var rows int
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM audit_events WHERE org_id = $1`, final.OrgID).Scan(&rows); err != nil {
		t.Fatal(err)
	}
	if rows != 2 {
		t.Fatalf("rows after first delivery and replay = %d, want 2 (one per phase)", rows)
	}

	// A different payload under a used event_id is refused, never overwritten.
	altered := *final
	altered.Outcome = "denied"
	if _, err := st.InsertAuditFromStream(ctx, &altered, bus, finalSubject, 5013); !errors.Is(err, store.ErrAuditEventConflict) {
		t.Fatalf("a changed payload under a used event_id returned %v, want ErrAuditEventConflict", err)
	}

	// The unique indexes that make this hold are part of the schema.
	for _, index := range []string{"uq_audit_events_source_producer_event", "uq_audit_events_source_subject_event", "uq_audit_events_jetstream_inbox"} {
		var present bool
		if err := pool.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM pg_indexes WHERE tablename = 'audit_events' AND indexname = $1)`, index).Scan(&present); err != nil {
			t.Fatal(err)
		}
		if !present {
			t.Fatalf("audit_events lacks the unique index %s", index)
		}
	}
}
