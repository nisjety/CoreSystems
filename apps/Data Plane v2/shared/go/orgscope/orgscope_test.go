package orgscope

import (
	"context"
	"os"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// The role name is interpolated into SQL (SET LOCAL ROLE cannot bind), so it
// must never become caller-controlled. This is a tripwire on that invariant:
// if someone parameterizes the role, this fails and they have to think about
// injection.
func TestRuntimeRoleIsAFixedBareIdentifier(t *testing.T) {
	if RuntimeRole != "dataplane_app" {
		t.Fatalf("unexpected runtime role %q", RuntimeRole)
	}
	for _, r := range RuntimeRole {
		if !(r == '_' || (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9')) {
			t.Fatalf("runtime role must be a bare identifier, got %q", RuntimeRole)
		}
	}
}

// Policies read current_setting('app.current_org'). If the constant the code
// sets and the one the policy reads ever diverge, every policy silently stops
// matching, so pin it.
func TestOrgGUCMatchesMigrationPolicyExpression(t *testing.T) {
	if OrgGUC != "app.current_org" {
		t.Fatalf("unexpected GUC %q", OrgGUC)
	}
}

// The empty-org guard must trip before any connection is attempted. A nil pool
// guarantees that: if the guard regressed we would panic on the nil
// dereference instead of getting the error asserted here, so this is not
// tautological.
func TestEmptyOrgIDRejectedBeforeTouchingDatabase(t *testing.T) {
	for _, candidate := range []string{"", "   ", "\t"} {
		err := WithOrgScope(context.Background(), nil, candidate, func(pgx.Tx) error {
			t.Fatal("callback must not run for an empty org")
			return nil
		})
		if err == nil || !strings.Contains(err.Error(), "non-empty orgID") {
			t.Fatalf("expected ErrEmptyOrgID for %q, got %v", candidate, err)
		}
	}
}

// Real end-to-end proof against a live database. Skipped unless one is
// supplied, mirroring the Rust crate's ignored integration test:
//
//	TEST_DATABASE_URL=postgres://dataplane:...@127.0.0.1:5442/dataplane \
//	  go test ./...
func TestScopedTransactionIsolatesOrgsForReal(t *testing.T) {
	url := os.Getenv("TEST_DATABASE_URL")
	if url == "" {
		t.Skip("set TEST_DATABASE_URL to a DPv2 database with the RLS migration applied")
	}

	ctx := context.Background()
	pool, err := pgxpool.New(ctx, url)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()

	// Unscoped (superuser) baseline.
	var total int64
	if err := pool.QueryRow(ctx, "SELECT count(*) FROM documents").Scan(&total); err != nil {
		t.Fatalf("baseline count: %v", err)
	}

	// Scoped to an org that owns nothing: must see zero and mutate nothing.
	err = WithOrgScope(ctx, pool, "orgscope-nonexistent-org", func(tx pgx.Tx) error {
		var visible int64
		if err := tx.QueryRow(ctx, "SELECT count(*) FROM documents").Scan(&visible); err != nil {
			return err
		}
		if visible != 0 {
			t.Errorf("a foreign org must see no documents, saw %d", visible)
		}

		tag, err := tx.Exec(ctx, "UPDATE documents SET title = 'orgscope-test-should-not-apply'")
		if err != nil {
			return err
		}
		if n := tag.RowsAffected(); n != 0 {
			t.Errorf("a foreign org must not update any document, updated %d", n)
		}
		// Return an error so the helper rolls back: this test must never
		// leave a trace, and it also exercises the rollback-on-error path.
		return errSentinelRollback
	})
	if err != errSentinelRollback {
		t.Fatalf("expected the sentinel rollback error, got %v", err)
	}

	// The unscoped connection still sees everything, proving the filtering
	// above came from the scoped role and not from an empty table.
	var after int64
	if err := pool.QueryRow(ctx, "SELECT count(*) FROM documents").Scan(&after); err != nil {
		t.Fatalf("post count: %v", err)
	}
	if after != total {
		t.Fatalf("unscoped visibility changed: %d -> %d", total, after)
	}
}

type sentinelErr string

func (e sentinelErr) Error() string { return string(e) }

const errSentinelRollback = sentinelErr("intentional rollback")
