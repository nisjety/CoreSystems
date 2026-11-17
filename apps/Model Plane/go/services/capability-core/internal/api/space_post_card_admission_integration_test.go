//go:build integration

package api

import (
	"context"
	"maps"
	"os"
	"path/filepath"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
)

// applyAdmissionMigration runs one migration file from capability-core's migrations folder on pool.
func applyAdmissionMigration(t *testing.T, pool *pgxpool.Pool, name string) {
	t.Helper()
	sql, err := os.ReadFile(filepath.Join("..", "..", "migrations", name))
	if err != nil {
		t.Fatalf("read migration %s: %v", name, err)
	}
	if _, err := pool.Exec(context.Background(), string(sql)); err != nil {
		t.Fatalf("apply migration %s: %v", name, err)
	}
}

// admissionStatuses reads the status of every agent action admission row, keyed by action id.
func admissionStatuses(t *testing.T, pool *pgxpool.Pool) map[string]string {
	t.Helper()
	rows, err := pool.Query(context.Background(), `SELECT action_id, status FROM agent_action_admissions`)
	if err != nil {
		t.Fatalf("read admissions: %v", err)
	}
	defer rows.Close()
	statuses := map[string]string{}
	for rows.Next() {
		var id, status string
		if err := rows.Scan(&id, &status); err != nil {
			t.Fatalf("scan admission: %v", err)
		}
		statuses[id] = status
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("read admissions: %v", err)
	}
	return statuses
}

// W3-DEC-CARD: migration 0047 flips the post_card admission row from declared to enabled and nothing else. A second apply
// is a no-op, a retired row stays retired, and the down returns the row to declared. Needs Docker for testcontainers, so
// it runs only with -tags integration and is not run in the code-phase gate.
func TestSpacePostCardAdmissionMigrationFlipsOnlyPostCard(t *testing.T) {
	pool := setupAvailabilityDB(t)
	applyAdmissionMigration(t, pool, "0022_agent_action_admissions.up.sql")
	applyAdmissionMigration(t, pool, "0045_space_self_api_capabilities.up.sql")
	before := admissionStatuses(t, pool)
	if before["space.thread.post_card"] != "declared" {
		t.Fatalf("0045 must seed post_card declared, got %q", before["space.thread.post_card"])
	}

	const up = "0047_space_thread_post_card_admission.up.sql"
	const down = "0047_space_thread_post_card_admission.down.sql"
	applyAdmissionMigration(t, pool, up)
	after := admissionStatuses(t, pool)
	if after["space.thread.post_card"] != "enabled" {
		t.Fatalf("0047 up left post_card %q, want enabled", after["space.thread.post_card"])
	}
	for _, id := range []string{"space.thread.rename", "space.thread.pin", "space.agent.pause_self", "space.schedule.follow_up"} {
		if after[id] != before[id] {
			t.Fatalf("0047 up changed %s from %q to %q", id, before[id], after[id])
		}
	}

	applyAdmissionMigration(t, pool, up)
	if again := admissionStatuses(t, pool); !maps.Equal(again, after) {
		t.Fatalf("a second 0047 up changed the rows:\n first  %v\n second %v", after, again)
	}

	ctx := context.Background()
	if _, err := pool.Exec(ctx, `UPDATE agent_action_admissions SET status = 'retired' WHERE action_id = 'space.thread.post_card'`); err != nil {
		t.Fatal(err)
	}
	applyAdmissionMigration(t, pool, up)
	if got := admissionStatuses(t, pool)["space.thread.post_card"]; got != "retired" {
		t.Fatalf("0047 up revived a retired row: %q", got)
	}

	if _, err := pool.Exec(ctx, `UPDATE agent_action_admissions SET status = 'enabled' WHERE action_id = 'space.thread.post_card'`); err != nil {
		t.Fatal(err)
	}
	applyAdmissionMigration(t, pool, down)
	if got := admissionStatuses(t, pool)["space.thread.post_card"]; got != "declared" {
		t.Fatalf("0047 down left post_card %q, want declared", got)
	}
}
