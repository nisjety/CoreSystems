//go:build integration

package repo_test

// Integration test for DocumentRepo.HardPurgeByOrg using testcontainers-postgres.
// Run with: go test -tags integration -race ./...
//
// Requires Docker available locally. Skipped on CI runs without Docker.
// Reuses setupPostgres(t) and its schemaSQL from integration_test.go (same
// package, same build tag) so this exercises the identical documents /
// documents_outbox / source_objects / org_versions schema the production
// repo queries.

import (
	"context"
	"fmt"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
)

// purgeScopedTables lists every table HardPurgeByOrg deletes from — kept in
// one place so the "org A fully purged" / "org B untouched" assertions below
// can loop over the exact same list HardPurgeByOrg's implementation uses.
var purgeScopedTables = []string{"documents", "documents_outbox", "source_objects", "org_versions"}

func countRowsForOrg(ctx context.Context, t *testing.T, pool *pgxpool.Pool, table, orgID string) int {
	t.Helper()
	var n int
	query := fmt.Sprintf(`SELECT COUNT(*) FROM %s WHERE org_id = $1`, table)
	if err := pool.QueryRow(ctx, query, orgID).Scan(&n); err != nil {
		t.Fatalf("count %s for %s: %v", table, orgID, err)
	}
	return n
}

// TestHardPurgeByOrgIsOrgScopedAndIdempotent is the safety contract for the
// GDPR org-erasure consumer (internal/gdpr/org_purge.go): purging org A must
// never touch org B's rows created in the same run, and redelivering the
// same purge (NATS is at-least-once) must not error the second time.
func TestHardPurgeByOrgIsOrgScopedAndIdempotent(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	docRepo := repo.NewDocumentRepo(pool)
	sourceRepo := repo.NewSourceObjectRepo(pool)
	ctx := context.Background()

	// Seed org A: a document (bumps org_versions), a manual outbox row, and a
	// source object.
	docA, err := docRepo.Create(ctx, model.CreateDocumentInput{
		OrgID: "org-A", Source: "s", Type: "t", Title: "A-doc", Content: "secret-a",
	})
	if err != nil {
		t.Fatalf("create org-A document: %v", err)
	}
	if err := docRepo.EnqueueOutbox(ctx, "org-A", "documents.created", []byte(`{"document_id":"`+docA.Document.DocumentID+`"}`)); err != nil {
		t.Fatalf("enqueue org-A outbox: %v", err)
	}
	if _, err := sourceRepo.Upsert(ctx, model.UpsertSourceObjectInput{
		OrgID: "org-A", Connector: "sharepoint", Source: "sharepoint", ExternalID: "ext-a", Name: "file-a.docx",
	}); err != nil {
		t.Fatalf("upsert org-A source object: %v", err)
	}

	// Seed org B with equivalent rows — the control group that must survive
	// org A's purge untouched.
	docB, err := docRepo.Create(ctx, model.CreateDocumentInput{
		OrgID: "org-B", Source: "s", Type: "t", Title: "B-doc", Content: "secret-b",
	})
	if err != nil {
		t.Fatalf("create org-B document: %v", err)
	}
	if err := docRepo.EnqueueOutbox(ctx, "org-B", "documents.created", []byte(`{"document_id":"`+docB.Document.DocumentID+`"}`)); err != nil {
		t.Fatalf("enqueue org-B outbox: %v", err)
	}
	if _, err := sourceRepo.Upsert(ctx, model.UpsertSourceObjectInput{
		OrgID: "org-B", Connector: "sharepoint", Source: "sharepoint", ExternalID: "ext-b", Name: "file-b.docx",
	}); err != nil {
		t.Fatalf("upsert org-B source object: %v", err)
	}

	// Sanity: both orgs have rows in every scoped table before the purge.
	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-A"); n == 0 {
			t.Fatalf("fixture bug: org-A has no seed rows in %s", table)
		}
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("fixture bug: org-B has no seed rows in %s", table)
		}
	}

	if err := docRepo.HardPurgeByOrg(ctx, "org-A"); err != nil {
		t.Fatalf("hard purge org-A: %v", err)
	}

	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-A"); n != 0 {
			t.Fatalf("org-A %s not purged: %d rows remain", table, n)
		}
	}
	// The other org's data — created in the very same test run — must be
	// completely unaffected by org-A's purge.
	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("org-B %s was wrongly purged by org-A's erasure", table)
		}
	}

	// NATS is at-least-once delivery: the consumer may redeliver and re-run
	// the purge for an org that was already fully purged. That must not
	// error, and must still leave org B untouched.
	if err := docRepo.HardPurgeByOrg(ctx, "org-A"); err != nil {
		t.Fatalf("second hard purge of org-A must be idempotent, got error: %v", err)
	}
	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("org-B %s was wrongly purged by org-A's redelivered erasure", table)
		}
	}
}

func TestHardPurgeByOrgRejectsEmptyOrgID(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	docRepo := repo.NewDocumentRepo(pool)
	if err := docRepo.HardPurgeByOrg(context.Background(), "   "); err == nil {
		t.Fatal("expected an error for a blank org_id — must never resolve to an unscoped DELETE")
	}
}
