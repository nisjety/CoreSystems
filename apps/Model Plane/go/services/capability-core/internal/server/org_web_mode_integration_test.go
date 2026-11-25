//go:build integration

package server

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// The web gate reads an org's two keys in one SQL statement over org_agent_policy. A fake cannot see how Postgres reads
// the jsonb operators, and three states differ: an absent key, a present key with a JSON null, and a stored value. So
// the reader is pinned here on a real Postgres, over capability-core's own migration 0023 (F-f09-13).
//
// Run with: go test -tags integration -run TestOrgWebModeStore ./internal/server/

// orgWebTestDigest is a well-formed digest for the row's check constraint. It is a fixed low-entropy fake.
const orgWebTestDigest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

// setupOrgWebModeStore spins a throwaway Postgres, applies capability-core's migration 0023 and returns the store over
// it with the pool that holds the rows.
func setupOrgWebModeStore(t *testing.T) (*OrgWebModeStore, *pgxpool.Pool) {
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

	migration := filepath.Join("..", "..", "migrations", "0023_org_agent_policy.up.sql")
	sql, err := os.ReadFile(migration)
	if err != nil {
		t.Fatalf("read %s: %v — if the migration moved, re-point this test rather than deleting it", migration, err)
	}
	if _, err := pool.Exec(ctx, string(sql)); err != nil {
		t.Fatalf("apply 0023: %v", err)
	}
	return NewOrgWebModeStore(pool), pool
}

// TestOrgWebModeStoreReadsTheStoredDocument pins what the statement returns for each stored shape: the opt-out, the
// defaults, a JSON null, and an org with no row.
func TestOrgWebModeStoreReadsTheStoredDocument(t *testing.T) {
	store, pool := setupOrgWebModeStore(t)
	ctx := context.Background()

	for org, doc := range map[string]string{
		"org-off":        `{"web.mode": "off", "web.covers_browser": false}`,
		"org-defaults":   `{}`,
		"org-cached":     `{"web.mode": "cached_only", "web.covers_browser": true}`,
		"org-string-off": `{"web.mode": "off", "web.covers_browser": "false"}`,
		"org-null-mode":  `{"web.mode": null}`,
		"org-null-cover": `{"web.covers_browser": null}`,
	} {
		if _, err := pool.Exec(ctx,
			`INSERT INTO org_agent_policy (org_id, revision, doc, digest, updated_by) VALUES ($1, 1, $2::jsonb, $3, 'test')`,
			org, doc, orgWebTestDigest); err != nil {
			t.Fatalf("insert %s: %v", org, err)
		}
	}

	for _, tc := range []struct {
		org        string
		wantMode   string
		wantCovers bool
	}{
		{"org-off", "off", false},
		{"org-defaults", "live", true},
		{"org-cached", "cached_only", true},
		{"org-null-mode", "", true},
		{"org-null-cover", "live", true},
		{"org-string-off", "off", true},
		{"org-without-a-row", "live", true},
	} {
		t.Run(tc.org, func(t *testing.T) {
			mode, covers, err := store.WebMode(ctx, tc.org)
			if err != nil {
				t.Fatalf("WebMode: %v", err)
			}
			if mode != tc.wantMode || covers != tc.wantCovers {
				t.Fatalf("WebMode(%s) = (%q, %v), want (%q, %v)", tc.org, mode, covers, tc.wantMode, tc.wantCovers)
			}
		})
	}
}
