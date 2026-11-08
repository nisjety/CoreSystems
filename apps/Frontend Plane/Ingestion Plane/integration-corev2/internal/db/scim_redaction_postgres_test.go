package db_test

import (
	"context"
	"os"
	"testing"

	"github.com/triodelab/integration-corev2/internal/store/storetest"
)

// WS05-S2: migration 0014 against a real database. Needs INTEGRATION_TEST_DATABASE_URL (a
// disposable database whose name contains test or tmp); skips without it, and FAILS under
// REQUIRE_CONTRACT or CI. It runs in its own schema.
func TestSCIMStubPayloadRedactionMigration0014OnDisposablePostgres(t *testing.T) {
	ctx := context.Background()
	pool := storetest.IsolatedPostgres(t, true)
	raw, err := os.ReadFile("migrations/0014_scim_stub_payload_redaction.sql")
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}

	for _, row := range []struct {
		id, provider, payload string
	}{
		{"scim-1", "scim", `{"userName":"ada@example.com","externalId":"u-1","members":[{"value":"u-2"}]}`},
		{"scim-2", "scim", `{"userName":"grace@example.com"}`},
		{"scim-empty", "scim", `{}`},
		{"stripe-1", "stripe", `{"id":"evt_1","object":"event"}`},
		{"slack-1", "slack", `{"user":"U1","text":"keep me"}`},
	} {
		if _, err := pool.Exec(ctx, `
			INSERT INTO integration_webhook_events (id, organization_id, provider_key, event_type, payload)
			VALUES ($1, 'org-1', $2, 'webhook_received', $3::jsonb)`, row.id, row.provider, row.payload); err != nil {
			t.Fatalf("seed %s: %v", row.id, err)
		}
	}

	payloads := func() map[string]string {
		rows, err := pool.Query(ctx, `SELECT id, payload::text FROM integration_webhook_events ORDER BY id`)
		if err != nil {
			t.Fatalf("read payloads: %v", err)
		}
		defer rows.Close()
		out := map[string]string{}
		for rows.Next() {
			var id, payload string
			if err := rows.Scan(&id, &payload); err != nil {
				t.Fatalf("scan: %v", err)
			}
			out[id] = payload
		}
		return out
	}

	// Apply twice: the second run must change nothing (idempotent).
	var first map[string]string
	for run := 1; run <= 2; run++ {
		if _, err := pool.Exec(ctx, string(raw)); err != nil {
			t.Fatalf("apply 0014 (run %d): %v", run, err)
		}
		got := payloads()
		if run == 1 {
			first = got
		}
		for _, id := range []string{"scim-1", "scim-2", "scim-empty"} {
			if got[id] != "{}" {
				t.Fatalf("run %d: %s payload = %s, want {}", run, id, got[id])
			}
		}
		if got["stripe-1"] != `{"id": "evt_1", "object": "event"}` || got["slack-1"] != `{"text": "keep me", "user": "U1"}` {
			t.Fatalf("run %d: another provider's payload was touched: %v", run, got)
		}
		if len(got) != len(first) {
			t.Fatalf("run %d: rows were deleted: %d -> %d", run, len(first), len(got))
		}
	}

	// The event identity stays as an inert trace.
	var kept int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM integration_webhook_events WHERE provider_key = 'scim' AND organization_id = 'org-1' AND event_type = 'webhook_received'`).Scan(&kept); err != nil || kept != 3 {
		t.Fatalf("scim rows kept = %d, err = %v, want 3", kept, err)
	}
}
