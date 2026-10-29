package eval

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

func TestPostgresEvalStoreLifecycleIdempotencyAndTenantIsolation(t *testing.T) {
	pool := qualityIntegrationPool(t)
	store := NewPostgresEvalStore(pool)
	ctx := context.Background()
	now := time.Now().UTC()
	request := model.EvalRun{
		EvalID: uuid.NewString(), OrgID: "org-quality-a", Strategy: "hybrid",
		Status: model.EvalPending, IdempotencyKey: "quality-integration-1",
		CreatedAt: now, UpdatedAt: now,
	}

	created, wasCreated, err := store.Create(ctx, request)
	if err != nil || !wasCreated {
		t.Fatalf("create: created=%v err=%v", wasCreated, err)
	}
	replay := request
	replay.EvalID = uuid.NewString()
	existing, replayCreated, err := store.Create(ctx, replay)
	if err != nil || replayCreated || existing.EvalID != created.EvalID {
		t.Fatalf("idempotent replay = %+v created=%v err=%v", existing, replayCreated, err)
	}
	conflictingReplay := request
	conflictingReplay.EvalID = uuid.NewString()
	conflictingReplay.Strategy = "dense"
	if _, _, err := store.Create(ctx, conflictingReplay); !errors.Is(err, ErrIdempotencyConflict) {
		t.Fatalf("conflicting replay error = %v, want ErrIdempotencyConflict", err)
	}
	otherTenant := request
	otherTenant.EvalID, otherTenant.OrgID = uuid.NewString(), "org-quality-b"
	if _, otherCreated, err := store.Create(ctx, otherTenant); err != nil || !otherCreated {
		t.Fatalf("tenant-local idempotency: created=%v err=%v", otherCreated, err)
	}
	if _, err := store.Get(ctx, "org-quality-b", created.EvalID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-tenant lookup error = %v, want ErrNotFound", err)
	}

	running, err := store.Start(ctx, request.OrgID, created.EvalID)
	if err != nil || running.Status != model.EvalRunning || running.StartedAt == nil {
		t.Fatalf("start = %+v err=%v", running, err)
	}
	scorecard, _ := json.Marshal(model.Scorecard{Strategy: "hybrid", QueriesRun: 1})
	completed, err := store.Complete(ctx, request.OrgID, created.EvalID, scorecard)
	if err != nil || completed.Status != model.EvalCompleted || completed.FinishedAt == nil {
		t.Fatalf("complete = %+v err=%v", completed, err)
	}
	if _, err := store.Start(ctx, request.OrgID, created.EvalID); !errors.Is(err, ErrInvalidTransition) {
		t.Fatalf("terminal replay error = %v, want ErrInvalidTransition", err)
	}
	if _, err := store.Start(ctx, request.OrgID, uuid.NewString()); !errors.Is(err, ErrNotFound) {
		t.Fatalf("missing eval transition error = %v, want ErrNotFound", err)
	}

	failedRequest := request
	failedRequest.EvalID = uuid.NewString()
	failedRequest.IdempotencyKey = "quality-integration-failed"
	failedEval, _, err := store.Create(ctx, failedRequest)
	if err != nil {
		t.Fatalf("create failed eval: %v", err)
	}
	failedEval, err = store.Fail(ctx, request.OrgID, failedEval.EvalID, "isolated failure")
	if err != nil || failedEval.Status != model.EvalFailed || failedEval.FinishedAt == nil {
		t.Fatalf("fail = %+v err=%v", failedEval, err)
	}

	invalid := request
	invalid.EvalID, invalid.IdempotencyKey = uuid.NewString(), "short"
	if _, _, err := store.Create(ctx, invalid); err == nil {
		t.Fatal("short idempotency key unexpectedly passed database constraint")
	}
}

func qualityIntegrationPool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	databaseURL := os.Getenv("TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("TEST_DATABASE_URL is required for disposable PostgreSQL integration tests")
	}
	ctx := context.Background()
	adminConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse TEST_DATABASE_URL: %v", err)
	}
	adminConfig.ConnConfig.DefaultQueryExecMode = pgx.QueryExecModeSimpleProtocol
	admin, err := pgxpool.NewWithConfig(ctx, adminConfig)
	if err != nil {
		t.Fatalf("connect disposable PostgreSQL: %v", err)
	}
	schema := "quality_eval_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	identifier := pgx.Identifier{schema}.Sanitize()
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+identifier); err != nil {
		admin.Close()
		t.Fatalf("create disposable schema: %v", err)
	}

	config, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse schema config: %v", err)
	}
	config.ConnConfig.DefaultQueryExecMode = pgx.QueryExecModeSimpleProtocol
	config.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, config)
	if err != nil {
		t.Fatalf("connect disposable schema: %v", err)
	}

	up := readDurabilityMigration(t, "20260711160000_quality_orchestrator_durability.sql")
	if _, err := pool.Exec(ctx, up); err != nil {
		t.Fatalf("apply durability migration: %v", err)
	}
	if _, err := pool.Exec(ctx, up); err != nil {
		t.Fatalf("reapply idempotent durability migration: %v", err)
	}
	grantScopedRuntimeRoleIn(ctx, t, pool, schema)
	t.Cleanup(func() {
		down := readDurabilityMigration(t, "20260711160000_quality_orchestrator_durability.down.sql")
		if _, err := pool.Exec(context.Background(), down); err != nil {
			t.Errorf("apply durability rollback: %v", err)
		}
		pool.Close()
		if _, err := admin.Exec(context.Background(), "DROP SCHEMA "+identifier+" CASCADE"); err != nil {
			t.Errorf("drop disposable schema: %v", err)
		}
		admin.Close()
	})
	return pool
}

// grantScopedRuntimeRoleIn makes `dataplane_app` usable inside this test's
// isolated schema.
//
// Why a test fixture needs a database ROLE at all: PostgresEvalStore now runs
// Create/Get/Start/Complete/Fail through orgscope.InOrgScope, which issues
// `SET LOCAL ROLE dataplane_app` on every scoped transaction. That role is
// created by infra/postgres/migrations/20260809120000_org_rls_isolation.sql,
// which this fixture never runs — it applies only the durability migration the
// store depends on. Without this helper the first scoped call fails with
// `role "dataplane_app" does not exist`, and because the test is gated on
// TEST_DATABASE_URL a plain `go test ./...` would never surface it.
//
// Grants only — no policies. Enabling RLS here would test the migration rather
// than the store, and this fixture deliberately builds a reduced schema the
// real policies do not match.
//
// Both exception codes are required on the CREATE ROLE. Roles are cluster-wide,
// so parallel packages/binaries sharing one server race here; with the role
// absent, the losing backend raises unique_violation on pg_authid_rolname_index
// before duplicate_object is ever considered.
func grantScopedRuntimeRoleIn(ctx context.Context, t *testing.T, pool *pgxpool.Pool, schema string) {
	t.Helper()
	// schema is a locally generated "quality_eval_<uuid hex>" identifier, never
	// caller input; %I quotes it regardless.
	if _, err := pool.Exec(ctx, `
		DO $role$
		BEGIN
		    CREATE ROLE dataplane_app NOLOGIN NOSUPERUSER NOBYPASSRLS;
		EXCEPTION WHEN duplicate_object OR unique_violation THEN
		    NULL;
		END
		$role$;

		DO $grants$
		BEGIN
		    EXECUTE format('GRANT USAGE ON SCHEMA %I TO dataplane_app', `+quoteSQLLiteral(schema)+`);
		    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA %I TO dataplane_app', `+quoteSQLLiteral(schema)+`);
		    EXECUTE format('GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA %I TO dataplane_app', `+quoteSQLLiteral(schema)+`);
		    EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA %I GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO dataplane_app', `+quoteSQLLiteral(schema)+`);
		    EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA %I GRANT USAGE, SELECT ON SEQUENCES TO dataplane_app', `+quoteSQLLiteral(schema)+`);
		END
		$grants$;
	`); err != nil {
		t.Fatalf("grant scoped runtime role in %s: %v", schema, err)
	}
}

// quoteSQLLiteral renders a Go string as a single-quoted SQL literal. Needed
// because the grants above run inside a DO block, which cannot take bind
// parameters.
func quoteSQLLiteral(s string) string {
	return "'" + strings.ReplaceAll(s, "'", "''") + "'"
}

func readDurabilityMigration(t *testing.T, name string) string {
	t.Helper()
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("locate integration test source")
	}
	path := filepath.Join(filepath.Dir(source), "..", "..", "..", "..", "infra", "postgres", "migrations", name)
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read migration %s: %v", name, err)
	}
	return string(contents)
}
