//go:build integration

package grant

// WS8.C integration tests: the human_browse / attach contract on the production
// store shape, the shape of the rows, the table constraints of migration 0004
// and the migration's idempotency and reversibility.
//
//	GOWORK=off go test -tags integration -run 'TestPg_HumanBrowse|TestPg_Migration0004' ./internal/grant/...

import (
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
)

// The WS8.C contract, on Routed{PgStore, MemoryRepository}.
func TestPg_HumanBrowseContract(t *testing.T) {
	for _, c := range humanBrowseContract {
		t.Run(c.name, func(t *testing.T) {
			pool, _ := newTestDB(t)
			c.run(t, func(*testing.T) (Repository, *fakeClock) {
				_, _, routed, clock := stores(pool)
				return routed, clock
			})
		})
	}
}

func TestPg_HumanBrowse_RowShapeAndZdrWritesNoRow(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	g := human(t, store, CreateHumanBrowseParams{DeniedDomains: []string{".internal", "metadata.goog"}})
	a, err := store.Derive(ctx, attachParams(g, "attach", "run-7", "example.com"))
	if err != nil {
		t.Fatal(err)
	}
	var mode string
	var denied, allowed []string
	var abs, exp time.Time
	if err := pool.QueryRow(ctx, `SELECT policy_mode, denied_domains, allowed_domains, absolute_expires_at, expires_at
		FROM browser_grants WHERE id = $1`, g.ID).Scan(&mode, &denied, &allowed, &abs, &exp); err != nil {
		t.Fatal(err)
	}
	if mode != "open_web" || !equalStrings(denied, []string{".internal", "metadata.goog"}) || len(allowed) != 0 ||
		!abs.Equal(g.AbsoluteExpiresAt) || !exp.Equal(g.ExpiresAt) || !abs.After(exp) {
		t.Fatalf("human_browse row: mode=%q denied=%v allowed=%v absolute=%v expires=%v", mode, denied, allowed, abs, exp)
	}
	var resource *string
	var childMode string
	if err := pool.QueryRow(ctx, `SELECT resource_id, policy_mode FROM browser_grants WHERE id = $1`, a.ID).Scan(&resource, &childMode); err != nil {
		t.Fatal(err)
	}
	if resource == nil || *resource != "run-7" || childMode != "allowlist" {
		t.Fatalf("attach row: resource=%v mode=%q", resource, childMode)
	}
	// Every other kind has no resource.
	if n := rowCount(t, pool, "WHERE resource_id IS NOT NULL AND kind <> 'attach'"); n != 0 {
		t.Fatalf("%d non-attach rows carry a resource", n)
	}
	// A ZDR human_browse grant and its attach refusal write no row.
	before := rowCount(t, pool, "")
	z := human(t, store, CreateHumanBrowseParams{SessionKey: "zdr", Zdr: true})
	if _, err := store.Derive(ctx, attachParams(z, "attach", "run-1", "example.com")); !errors.Is(err, ErrAttachZdrRefused) {
		t.Fatalf("attach under a ZDR parent: %v", err)
	}
	if _, err := store.Renew(ctx, RenewParams{OrgID: z.OrgID, OwnerID: z.OwnerID, GrantID: z.ID, TTL: time.Minute}); err != nil {
		t.Fatalf("renew a ZDR grant: %v", err)
	}
	if after := rowCount(t, pool, ""); after != before {
		t.Fatalf("a ZDR human_browse grant wrote %d rows", after-before)
	}
}

func TestPg_HumanBrowse_TableConstraints(t *testing.T) {
	pool, _ := newTestDB(t)
	cases := map[string]struct {
		sql        string
		constraint string
	}{
		"human_browse with an allow-list": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, policy_mode, denied_domains)
			 VALUES ('c1', 'human_browse', 'o', 'u', 's', '', '{example.com}', now(), now(), 'open_web', '{.internal}')`,
			"browser_grants_policy_mode_check",
		},
		"human_browse with no deny-list": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, policy_mode)
			 VALUES ('c2', 'human_browse', 'o', 'u', 's', '', '{}', now(), now(), 'open_web')`,
			"browser_grants_policy_mode_check",
		},
		"open web on a session grant": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, policy_mode, denied_domains)
			 VALUES ('c3', 'session', 'o', 'u', 's', '', '{example.com}', now(), now(), 'open_web', '{.internal}')`,
			"browser_grants_policy_mode_check",
		},
		"a deny-list on an allowlist grant": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, denied_domains)
			 VALUES ('c4', 'session', 'o', 'u', 's', '', '{example.com}', now(), now(), '{.internal}')`,
			"browser_grants_policy_mode_check",
		},
		"a session grant with no hosts": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at)
			 VALUES ('c5', 'session', 'o', 'u', 's', '', '{}', now(), now())`,
			"browser_grants_domains_check",
		},
		"an attach grant with no resource": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, parent_grant_id)
			 VALUES ('c6', 'attach', 'o', 'u', 's', '', '{example.com}', now(), now(), 'p1')`,
			"browser_grants_resource_check",
		},
		"a resource on a session grant": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, resource_id)
			 VALUES ('c7', 'session', 'o', 'u', 's', '', '{example.com}', now(), now(), 'run-1')`,
			"browser_grants_resource_check",
		},
		"an unknown kind": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at)
			 VALUES ('c8', 'bogus', 'o', 'u', 's', '', '{example.com}', now(), now())`,
			"browser_grants_kind_check",
		},
		"a human_browse grant with a parent": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, policy_mode, denied_domains, parent_grant_id)
			 VALUES ('c9', 'human_browse', 'o', 'u', 's', '', '{}', now(), now(), 'open_web', '{.internal}', 'p1')`,
			"browser_grants_parent_check",
		},
		"an expiry past the absolute cap": {
			`INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, policy_mode, denied_domains)
			 VALUES ('c10', 'human_browse', 'o', 'u', 's', '', '{}', now() + interval '2 hours', now() + interval '1 hour', 'open_web', '{.internal}')`,
			"browser_grants_expiry_check",
		},
	}
	if _, err := pool.Exec(ctx, `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at)
		VALUES ('p1', 'session', 'o', 'u', 's', '', '{example.com}', now(), now())`); err != nil {
		t.Fatal(err)
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			_, err := pool.Exec(ctx, tc.sql)
			var pgErr *pgconn.PgError
			if !errors.As(err, &pgErr) || pgErr.Code != "23514" || pgErr.ConstraintName != tc.constraint {
				t.Fatalf("error = %v, want check violation %s", err, tc.constraint)
			}
		})
	}
	// The rows the contract allows go in.
	if _, err := pool.Exec(ctx, `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, policy_mode, denied_domains)
		VALUES ('ok1', 'human_browse', 'o', 'u', 's', '', '{}', now(), now() + interval '4 hours', 'open_web', '{.internal}')`); err != nil {
		t.Fatalf("a valid human_browse row: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, parent_grant_id, resource_id)
		VALUES ('ok2', 'attach', 'o', 'u', 'k', '', '{example.com}', now(), now(), 'p1', 'run-1')`); err != nil {
		t.Fatalf("a valid attach row: %v", err)
	}
	// Attach is idempotent on (parent, session_key) like a run grant.
	if _, err := pool.Exec(ctx, `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at, parent_grant_id, resource_id)
		VALUES ('ok3', 'attach', 'o', 'u', 'k', '', '{example.com}', now(), now(), 'p1', 'run-2')`); err == nil {
		t.Fatal("a second attach under one (parent, session_key) was accepted")
	}
}

// 0004 is re-applied on every broker start, so it must be idempotent; its down
// migration removes what it added and leaves 0001's table as it was.
func TestPg_Migration0004_IsIdempotentAndReversible(t *testing.T) {
	pool, _ := newTestDB(t)
	applyMigration(t, pool, "0004_browser_grants_policy.up.sql")
	applyMigration(t, pool, "0004_browser_grants_policy.up.sql")
	_, _, store, _ := stores(pool)
	g := human(t, store, CreateHumanBrowseParams{})
	session(t, store, CreateSessionParams{SessionKey: "keep"})

	applyMigration(t, pool, "0004_browser_grants_policy.down.sql")
	applyMigration(t, pool, "0004_browser_grants_policy.down.sql")
	if n := rowCount(t, pool, "WHERE kind IN ('human_browse', 'attach')"); n != 0 {
		t.Fatalf("down left %d human_browse or attach rows", n)
	}
	if n := rowCount(t, pool, "WHERE kind = 'session'"); n != 1 {
		t.Fatalf("down touched sessions: %d rows", n)
	}
	var cols int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM information_schema.columns
		WHERE table_name = 'browser_grants' AND column_name IN ('policy_mode', 'denied_domains', 'resource_id')`).Scan(&cols); err != nil || cols != 0 {
		t.Fatalf("down left %d columns, %v", cols, err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains, expires_at, absolute_expires_at)
		VALUES ('x', 'human_browse', 'o', 'u', 's', '', '{}', now(), now())`); err == nil {
		t.Fatalf("0001's constraints are back, yet a human_browse row was accepted (grant %s)", g.ID)
	}
	// And it applies again on the reverted table.
	applyMigration(t, pool, "0004_browser_grants_policy.up.sql")
	human(t, store, CreateHumanBrowseParams{SessionKey: "again"})
}
