//go:build integration

package runwatch

import (
	"context"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// The owner check is one SQL predicate over cron_schedules. A fake cannot see a mistyped column, or a predicate
// that drops the org or the deleted_at guard, so the predicate is pinned on a real Postgres here (F-f05-06).
//
// The scratch table carries the columns the predicate reads, typed as session-core's migrations 0004 and 0028 give
// them: id, org_id, creator_subject_id and deleted_at.
//
// Run with: go test -tags integration -run TestScheduleCreatedBy ./internal/runwatch/

const scheduleOwnerSchema = `
CREATE TABLE cron_schedules (
    id                 TEXT PRIMARY KEY,
    org_id             TEXT NOT NULL DEFAULT '',
    creator_subject_id TEXT NOT NULL DEFAULT '',
    deleted_at         TIMESTAMPTZ
);
INSERT INTO cron_schedules (id, org_id, creator_subject_id, deleted_at) VALUES
    ('sched-live', 'org-1', 'subject-7', NULL),
    ('sched-deleted', 'org-1', 'subject-7', now()),
    ('sched-other-org', 'org-2', 'subject-7', NULL);
`

// setupScheduleOwnerStore spins a throwaway Postgres, creates the scratch cron_schedules and returns the store over it.
func setupScheduleOwnerStore(t *testing.T) *PostgresSubscriptionStore {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("capabilities"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		c, cc := context.WithTimeout(context.Background(), 30*time.Second)
		defer cc()
		_ = container.Terminate(c)
	})

	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("connection string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)

	if _, err := pool.Exec(ctx, scheduleOwnerSchema); err != nil {
		t.Fatalf("create scratch cron_schedules: %v", err)
	}
	return NewPostgresSubscriptionStore(pool)
}

// TestScheduleCreatedByMatchesOnlyTheLiveCreatorInTheOrg pins the predicate: the creator of a live schedule in the
// schedule's own org is true, and every other combination is false.
func TestScheduleCreatedByMatchesOnlyTheLiveCreatorInTheOrg(t *testing.T) {
	store := setupScheduleOwnerStore(t)
	ctx := context.Background()
	for _, tc := range []struct {
		name       string
		orgID      string
		scheduleID string
		subjectID  string
		want       bool
	}{
		{"the creator of a live schedule", "org-1", "sched-live", "subject-7", true},
		{"another subject", "org-1", "sched-live", "subject-8", false},
		{"a deleted schedule", "org-1", "sched-deleted", "subject-7", false},
		{"another org's schedule", "org-2", "sched-live", "subject-7", false},
		{"a schedule that was never written", "org-1", "sched-missing", "subject-7", false},
		{"the creator in the schedule's own org", "org-2", "sched-other-org", "subject-7", true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			got, err := store.ScheduleCreatedBy(ctx, tc.orgID, tc.scheduleID, tc.subjectID)
			if err != nil {
				t.Fatalf("ScheduleCreatedBy: %v", err)
			}
			if got != tc.want {
				t.Fatalf("ScheduleCreatedBy(%s, %s, %s) = %v, want %v", tc.orgID, tc.scheduleID, tc.subjectID, got, tc.want)
			}
		})
	}
}
