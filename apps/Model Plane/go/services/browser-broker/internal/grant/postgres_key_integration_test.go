//go:build integration

package grant

// INNH-10 MP-2: API-key grants on the production store shape.
//
//	TEST_DATABASE_URL=... GOWORK=off go test -tags integration -run 'TestPg_Key' ./internal/grant/...

import (
	"testing"
	"time"
)

func TestPg_KeyContract(t *testing.T) {
	for _, c := range keyContract {
		t.Run(c.name, func(t *testing.T) {
			pool, _ := newTestDB(t)
			c.run(t, func(*testing.T) (Repository, *fakeClock) {
				_, _, routed, clock := stores(pool)
				return routed, clock
			})
		})
	}
}

// A key grant is durable with its key id; a ZDR one never reaches the table.
func TestPg_KeyGrantIsDurableAndAZdrOneIsNot(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	g := keySession(t, store, "key_durable", CreateSessionParams{})
	if n := rowCount(t, pool, "WHERE id = $1 AND key_id = 'key_durable'", g.ID); n != 1 {
		t.Fatalf("%d durable rows for the key grant, want 1", n)
	}
	z := keySession(t, store, "key_durable", CreateSessionParams{Zdr: true, SessionKey: "z"})
	if z.ID[:2] != "z_" {
		t.Fatalf("zdr key grant id %q has no z_ prefix", z.ID)
	}
	if n := rowCount(t, pool, "WHERE key_id IS NOT NULL"); n != 1 {
		t.Fatalf("%d key rows, want 1 (the ZDR grant must stay in memory)", n)
	}
	if n := rowCount(t, pool, "WHERE key_id IS NULL"); n != 0 {
		t.Fatalf("%d rows without a key", n)
	}
	// RevokeByKey reaches the durable row with the key_revoked reason.
	_, _, routed, _ := stores(pool)
	if n, err := routed.RevokeByKey(ctx, "key_durable", KeyRevokedReason); err != nil || n != 1 {
		t.Fatalf("a fresh Routed (empty memory) revoked %d, %v; want the 1 durable grant", n, err)
	}
	var reason string
	if err := pool.QueryRow(ctx, `SELECT revoked_reason FROM browser_grants WHERE id = $1 AND revoked_at IS NOT NULL`, g.ID).Scan(&reason); err != nil || reason != KeyRevokedReason {
		t.Fatalf("revoked_reason = %q, %v", reason, err)
	}
}

// The table's own fence: a key_id row must be an allow-list session grant with
// no parent, at least one host and a full budget claim.
func TestPg_KeyCheckConstraintFencesTheShape(t *testing.T) {
	pool, _ := newTestDB(t)
	const base = `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains,
		expires_at, absolute_expires_at, budget_micro_usd, budget_max_steps, budget_max_runtime_s, policy_mode, key_id, budget_source)
		VALUES ($1, $2, 'o', 'u', 's', '', $3, now(), now(), $4, $5, $6, $7, $8, $9)`
	type row struct {
		name       string
		kind       string
		domains    []string
		micro      any
		steps, run any
		mode       string
		key        any
		ok         bool
	}
	full := func(r row) row {
		if r.kind == "" {
			r.kind = "session"
		}
		if r.domains == nil {
			r.domains = []string{"example.com"}
		}
		if r.mode == "" {
			r.mode = "allowlist"
		}
		return r
	}
	rows := []row{
		{name: "valid", micro: 1, steps: 1, run: 1, key: "k", ok: true},
		{name: "no key (ordinary)", micro: nil, steps: nil, run: nil, key: nil, ok: true},
		{name: "empty key", micro: 1, steps: 1, run: 1, key: ""},
		{name: "long key", micro: 1, steps: 1, run: 1, key: repeat('a', 129)},
		{name: "no budget", micro: nil, steps: nil, run: nil, key: "k"},
		{name: "no steps", micro: 1, steps: nil, run: 1, key: "k"},
		{name: "no runtime", micro: 1, steps: 1, run: nil, key: "k"},
		{name: "no hosts", domains: []string{}, micro: 1, steps: 1, run: 1, key: "k"},
		{name: "human_browse", kind: "human_browse", mode: "open_web", micro: 1, steps: 1, run: 1, key: "k"},
	}
	for i, r := range rows {
		r = full(r)
		// 0003's claim check: a source exactly when any bound is set.
		var source any
		if r.micro != nil || r.steps != nil || r.run != nil {
			source = "request"
		}
		_, err := pool.Exec(ctx, base, "chk"+string(rune('a'+i)), r.kind, r.domains, r.micro, r.steps, r.run, r.mode, r.key, source)
		if (err == nil) != r.ok {
			t.Fatalf("%s: insert error = %v, want ok=%v", r.name, err, r.ok)
		}
	}
}

func repeat(c byte, n int) string {
	b := make([]byte, n)
	for i := range b {
		b[i] = c
	}
	return string(b)
}

// 0005 is re-applied on every broker start; its down migration deletes key
// grants first so none survives as an ordinary grant of the key's owner.
func TestPg_Migration0005_IsIdempotentAndReversible(t *testing.T) {
	pool, _ := newTestDB(t)
	applyMigration(t, pool, "0005_browser_grants_key.up.sql")
	applyMigration(t, pool, "0005_browser_grants_key.up.sql")
	_, _, store, _ := stores(pool)
	keySession(t, store, "key_mig", CreateSessionParams{})
	plain := session(t, store, CreateSessionParams{SessionKey: "keep", TTL: 10 * time.Minute})

	applyMigration(t, pool, "0005_browser_grants_key.down.sql")
	applyMigration(t, pool, "0005_browser_grants_key.down.sql")
	if n := rowCount(t, pool, ""); n != 1 {
		t.Fatalf("down left %d rows, want only the ordinary grant", n)
	}
	var cols int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM information_schema.columns
		WHERE table_name = 'browser_grants' AND column_name = 'key_id'`).Scan(&cols); err != nil || cols != 0 {
		t.Fatalf("down left the key_id column (%d), %v", cols, err)
	}
	applyMigration(t, pool, "0005_browser_grants_key.up.sql")
	pool.Reset()
	if _, err := store.GetScoped(ctx, plain.ID, plain.OrgID, plain.OwnerID); err != nil {
		t.Fatalf("0005 down then up lost the ordinary grant: %v", err)
	}
	keySession(t, store, "key_mig", CreateSessionParams{SessionKey: "again"})
}
