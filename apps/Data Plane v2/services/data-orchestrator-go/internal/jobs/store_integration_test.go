package jobs

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

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

func TestPostgresJobStoreLifecycleIdempotencyAndTenantIsolation(t *testing.T) {
	pool := orchestratorIntegrationPool(t)
	store := NewPostgresJobStore(pool)
	ctx := context.Background()
	now := time.Now().UTC()
	request := model.Job{
		JobID: uuid.NewString(), OrgID: "org-job-a", JobType: model.JobReindex,
		Status: model.StatusPending, DocumentIDs: []string{"doc-1", "doc-2"},
		Total: 2, IdempotencyKey: "orchestrator-integration-1", CreatedAt: now, UpdatedAt: now,
	}

	created, wasCreated, err := store.Create(ctx, request)
	if err != nil || !wasCreated {
		t.Fatalf("create: created=%v err=%v", wasCreated, err)
	}
	replay := request
	replay.JobID = uuid.NewString()
	existing, replayCreated, err := store.Create(ctx, replay)
	if err != nil || replayCreated || existing.JobID != created.JobID {
		t.Fatalf("idempotent replay = %+v created=%v err=%v", existing, replayCreated, err)
	}
	conflictingReplay := request
	conflictingReplay.JobID = uuid.NewString()
	conflictingReplay.DocumentIDs = []string{"different-document"}
	if _, _, err := store.Create(ctx, conflictingReplay); !errors.Is(err, ErrIdempotencyConflict) {
		t.Fatalf("conflicting replay error = %v, want ErrIdempotencyConflict", err)
	}
	otherTenant := request
	otherTenant.JobID, otherTenant.OrgID = uuid.NewString(), "org-job-b"
	if _, otherCreated, err := store.Create(ctx, otherTenant); err != nil || !otherCreated {
		t.Fatalf("tenant-local idempotency: created=%v err=%v", otherCreated, err)
	}
	if _, err := store.Get(ctx, "org-job-b", created.JobID); !errors.Is(err, ErrJobNotFound) {
		t.Fatalf("cross-tenant lookup error = %v, want ErrJobNotFound", err)
	}

	running, err := store.Start(ctx, request.OrgID, created.JobID)
	if err != nil || running.Status != model.StatusRunning || running.StartedAt == nil {
		t.Fatalf("start = %+v err=%v", running, err)
	}
	if _, err := store.SetProgress(ctx, request.OrgID, created.JobID, 1); err != nil {
		t.Fatalf("set progress: %v", err)
	}
	if _, err := store.SetProgress(ctx, request.OrgID, created.JobID, 0); !errors.Is(err, ErrInvalidJobTransition) {
		t.Fatalf("progress regression error = %v, want ErrInvalidJobTransition", err)
	}
	result, _ := json.Marshal(map[string]int{"published": 2})
	completed, err := store.Complete(ctx, request.OrgID, created.JobID, result)
	if err != nil || completed.Status != model.StatusCompleted || completed.Progress != 2 || completed.CompletedAt == nil {
		t.Fatalf("complete = %+v err=%v", completed, err)
	}
	if _, err := store.Fail(ctx, request.OrgID, created.JobID, "late failure"); !errors.Is(err, ErrInvalidJobTransition) {
		t.Fatalf("terminal rewrite error = %v, want ErrInvalidJobTransition", err)
	}
	if _, err := store.Start(ctx, request.OrgID, uuid.NewString()); !errors.Is(err, ErrJobNotFound) {
		t.Fatalf("missing job transition error = %v, want ErrJobNotFound", err)
	}

	defaultResultRequest := request
	defaultResultRequest.JobID = uuid.NewString()
	defaultResultRequest.IdempotencyKey = "orchestrator-integration-default-result"
	defaultResultJob, _, err := store.Create(ctx, defaultResultRequest)
	if err != nil {
		t.Fatalf("create default-result job: %v", err)
	}
	if _, err := store.Start(ctx, request.OrgID, defaultResultJob.JobID); err != nil {
		t.Fatalf("start default-result job: %v", err)
	}
	defaultResultJob, err = store.Complete(ctx, request.OrgID, defaultResultJob.JobID, nil)
	if err != nil || string(defaultResultJob.Result) != "{}" {
		t.Fatalf("default result = %s err=%v, want {}", defaultResultJob.Result, err)
	}

	failedRequest := request
	failedRequest.JobID = uuid.NewString()
	failedRequest.IdempotencyKey = "orchestrator-integration-failed"
	failedJob, _, err := store.Create(ctx, failedRequest)
	if err != nil {
		t.Fatalf("create failed job: %v", err)
	}
	failedJob, err = store.Fail(ctx, request.OrgID, failedJob.JobID, "isolated failure")
	if err != nil || failedJob.Status != model.StatusFailed || failedJob.CompletedAt == nil {
		t.Fatalf("fail = %+v err=%v", failedJob, err)
	}

	invalid := request
	invalid.JobID, invalid.IdempotencyKey = uuid.NewString(), "short"
	if _, _, err := store.Create(ctx, invalid); err == nil {
		t.Fatal("short idempotency key unexpectedly passed database constraint")
	}
}

func orchestratorIntegrationPool(t *testing.T) *pgxpool.Pool {
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
	schema := "orchestrator_jobs_" + strings.ReplaceAll(uuid.NewString(), "-", "")
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
