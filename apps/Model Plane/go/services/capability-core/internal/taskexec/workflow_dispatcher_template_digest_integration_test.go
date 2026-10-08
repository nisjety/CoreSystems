//go:build integration

package taskexec

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// Why this test needs a real Postgres: the template Control commits to is
// stored in a JSONB column, read back by the sweeper, copied into
// tasks.config_json (also JSONB) and read back again by the dispatcher. JSONB
// does not keep the text it was given: it decodes escapes, re-renders strings
// and stores object keys shortest first. Only a real database says whether the
// digest taken before all that equals the digest taken after it.
//
// The stages below are the production ones, in order: the create handler's
// decode-and-marshal of the body (replicated, it is inline in the handler and
// pinned by internal/api/workplane_apis_template_digest_test.go), the real
// cron.Sweeper.RunOnce, and the real canonicalScheduledTemplate. The two tables
// carry only the columns those stages touch, with the column types of
// session-core migrations 0004 and 0028.
//
// Run with: go test -tags integration -run TestScheduleTemplateDigestSurvivesJSONB ./internal/taskexec/

const templateRoundTripSchema = `
CREATE TABLE cron_schedules (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL DEFAULT '',
    space_ref TEXT NOT NULL DEFAULT '',
    creator_subject_id TEXT NOT NULL DEFAULT '',
    template_digest TEXT NOT NULL DEFAULT '',
    schedule_expr TEXT NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    task_template JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_fire_at TIMESTAMPTZ,
    next_fire_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL DEFAULT 'agent',
    title TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    assignee TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'created',
    priority INT NOT NULL DEFAULT 0,
    config_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    scheduled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE cron_fires (
    id TEXT PRIMARY KEY,
    schedule_id TEXT NOT NULL,
    task_id TEXT,
    fired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT
);
`

func setupTemplateRoundTripDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("template_round_trip"),
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
		t.Fatalf("connect postgres: %v", err)
	}
	t.Cleanup(pool.Close)
	if _, err := pool.Exec(ctx, templateRoundTripSchema); err != nil {
		t.Fatalf("create scratch schema: %v", err)
	}
	return pool
}

type allowEveryFire struct{}

func (allowEveryFire) AuthorizeFire(context.Context, cron.FireIntent) error { return nil }

// roundTripTemplate takes a template through create, JSONB, a real sweeper fire
// and the dispatcher's canonicalisation, and returns the digest taken at each
// end together with the task config as Postgres renders it.
func roundTripTemplate(t *testing.T, pool *pgxpool.Pool, id, templateJSON string) (createDigest, fireDigest, storedConfig string, canonical []byte) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	// Create: CronHandler.listOrCreate decodes TaskTemplate into `any`,
	// marshals it and hashes the bytes.
	var decoded any
	if err := json.Unmarshal([]byte(templateJSON), &decoded); err != nil {
		t.Fatalf("decode template: %v", err)
	}
	tplJSON, _ := json.Marshal(decoded)
	createDigest = fmt.Sprintf("sha256:%x", sha256.Sum256(tplJSON))
	if _, err := pool.Exec(ctx, `
		INSERT INTO cron_schedules (id, org_id, space_ref, creator_subject_id, template_digest,
		    schedule_expr, timezone, task_template, enabled, next_fire_at)
		VALUES ($1, 'org-1', 'space-1', 'user-1', $2, '0 9 * * *', 'UTC', $3, true, now() - interval '1 minute')
	`, id, createDigest, tplJSON); err != nil {
		t.Fatalf("insert schedule: %v", err)
	}

	// Fire: the real sweeper reads the JSONB, writes tasks.config_json.
	if _, err := pool.Exec(ctx, `TRUNCATE tasks, cron_fires`); err != nil {
		t.Fatalf("reset tasks: %v", err)
	}
	fired, err := cron.NewSweeper(pool, allowEveryFire{}).RunOnce(ctx)
	if err != nil || fired != 1 {
		t.Fatalf("sweeper fired %d schedules, err = %v; want exactly 1", fired, err)
	}

	// Dispatch: the dispatcher reads tasks.config_json and canonicalises it.
	var config json.RawMessage
	if err := pool.QueryRow(ctx, `SELECT config_json FROM tasks`).Scan(&config); err != nil {
		t.Fatalf("read task config: %v", err)
	}
	canonical, fireDigest, err = canonicalScheduledTemplate(config)
	if err != nil {
		t.Fatalf("canonicalScheduledTemplate: %v", err)
	}
	return createDigest, fireDigest, string(config), canonical
}

func TestScheduleTemplateDigestSurvivesJSONB(t *testing.T) {
	pool := setupTemplateRoundTripDB(t)

	cases := []struct {
		name     string
		template string
		// stable is whether the digest taken at create equals the one the
		// dispatcher takes at fire. It mirrors the BFF rule
		// check_digest_stable in spaces/schedule_template.rs, which refuses the
		// templates marked false here before Control is asked about them.
		stable bool
	}{
		{"flat template with HTML characters, U+2028 and controls", dispatcherDigestVectorTemplate, true},
		{"nested workflow_input the Settings form sends", `{"kind":"cron","title":"t","workflow_input":{"goal":"g","policy":"p"}}`, true},
		{"nested keys of equal length", `{"o":{"bb":1,"aa":2}}`, true},
		{"nested keys ordered by UTF-8 byte length", `{"o":{"ab":1,"é":2}}`, true},
		{"nested array of ordered objects", `{"a":[{"x":1},{"a":1,"bb":2,"cc":3}]}`, true},
		{"top-level keys are re-sorted by Go either way", `{"b":1,"ab":2,"kind":"cron","a":3}`, true},
		{"nested object whose shorter key sorts later", `{"o":{"ab":1,"b":2}}`, false},
		{"nested object with a longer key before a two-byte key", `{"o":{"zzz":1,"é":2}}`, false},
		{"unordered object inside an array", `{"a":[{"x":1},{"ab":1,"b":2}]}`, false},
		{"unordered object two levels down", `{"a":{"ok":{"ab":1,"b":2}}}`, false},
	}
	for index, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			created, fired, stored, canonical := roundTripTemplate(t, pool, fmt.Sprintf("cron-%d", index), tc.template)
			t.Logf("task config as Postgres renders it: %s", stored)
			t.Logf("canonical at fire: %s", canonical)
			switch {
			case tc.stable && created != fired:
				t.Fatalf("digest changed across the JSONB round trip\n create: %s\n   fire: %s", created, fired)
			case !tc.stable && created == fired:
				t.Fatalf("expected the digest to change across the JSONB round trip, but it stayed %s", created)
			}
		})
	}

	t.Run("the shared vector keeps the digest the BFF computes", func(t *testing.T) {
		created, fired, _, canonical := roundTripTemplate(t, pool, "cron-vector", dispatcherDigestVectorTemplate)
		if created != dispatcherDigestVectorDigest || fired != dispatcherDigestVectorDigest {
			t.Fatalf("create = %s, fire = %s, want both %s", created, fired, dispatcherDigestVectorDigest)
		}
		if string(canonical) != dispatcherDigestVectorCanonical {
			t.Fatalf("canonical bytes at fire drifted\n got: %q\nwant: %q", canonical, dispatcherDigestVectorCanonical)
		}
	})
}
