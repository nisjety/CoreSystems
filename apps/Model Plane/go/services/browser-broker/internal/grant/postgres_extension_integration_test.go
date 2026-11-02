//go:build integration

package grant

// E3.2-e1 (OD13): origin-extension grants on the production store shape.
//
//	TEST_DATABASE_URL=... GOWORK=off go test -tags integration -run 'TestPg_Extension' ./internal/grant/...

import (
	"testing"
	"time"
)

func TestPg_ExtensionContract(t *testing.T) {
	for _, c := range extensionContract {
		t.Run(c.name, func(t *testing.T) {
			pool, _ := newTestDB(t)
			c.run(t, func(*testing.T) (Repository, *fakeClock) {
				_, _, routed, clock := stores(pool)
				return routed, clock
			})
		})
	}
}

// An extension is durable with its scope; a ZDR one never reaches the table.
func TestPg_ExtensionIsDurableAndAZdrOneIsNot(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	_, run := extRun(t, store)
	g, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeAlways, "a.other.org"))
	if err != nil {
		t.Fatal(err)
	}
	if n := rowCount(t, pool, "WHERE id = $1 AND kind = 'origin_extension' AND consent_scope = 'always'", g.ID); n != 1 {
		t.Fatalf("%d durable rows for the extension, want 1", n)
	}
	zroot := session(t, store, CreateSessionParams{Zdr: true, SessionKey: "zs", TTL: 30 * time.Minute})
	if _, err := store.CreateOriginExtension(ctx, extParams(zroot, ConsentScopeTurn, "z.other.org")); err != nil {
		t.Fatal(err)
	}
	if n := rowCount(t, pool, "WHERE kind = 'origin_extension'"); n != 1 {
		t.Fatalf("%d extension rows, want 1 (the ZDR extension must stay in memory)", n)
	}
}

// The table's own fence: an extension row is an allow-list grant with a
// parent, one to eight hosts, a valid scope, no key and no sensitive action.
func TestPg_ExtensionCheckConstraintFencesTheShape(t *testing.T) {
	pool, _ := newTestDB(t)
	_, _, store, _ := stores(pool)
	root := session(t, store, CreateSessionParams{TTL: 30 * time.Minute})
	const base = `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url, allowed_domains,
		allowed_actions, parent_grant_id, expires_at, absolute_expires_at, consent_scope, key_id)
		VALUES ($1, $2, 'o', 'u', 's', '', $3, $4, $5, now(), now(), $6, $7)`
	nine := []string{"a.x.org", "b.x.org", "c.x.org", "d.x.org", "e.x.org", "f.x.org", "g.x.org", "h.x.org", "i.x.org"}
	rows := []struct {
		name    string
		kind    string
		domains []string
		actions []string
		parent  any
		scope   any
		key     any
		ok      bool
	}{
		{"valid", "origin_extension", []string{"a.x.org"}, []string{}, root.ID, "turn", nil, true},
		{"valid always", "origin_extension", []string{"a.x.org"}, []string{}, root.ID, "always", nil, true},
		{"no scope", "origin_extension", []string{"a.x.org"}, []string{}, root.ID, nil, nil, false},
		{"bad scope", "origin_extension", []string{"a.x.org"}, []string{}, root.ID, "forever", nil, false},
		{"scope on a session", "session", []string{"a.x.org"}, []string{}, nil, "turn", nil, false},
		{"no parent", "origin_extension", []string{"a.x.org"}, []string{}, nil, "turn", nil, false},
		{"no hosts", "origin_extension", []string{}, []string{}, root.ID, "turn", nil, false},
		{"nine hosts", "origin_extension", nine, []string{}, root.ID, "turn", nil, false},
		{"with an action", "origin_extension", []string{"a.x.org"}, []string{"download_ref"}, root.ID, "turn", nil, false},
		{"with a key", "origin_extension", []string{"a.x.org"}, []string{}, root.ID, "turn", "k", false},
		{"ordinary session", "session", []string{"a.x.org"}, []string{}, nil, nil, nil, true},
	}
	for i, r := range rows {
		_, err := pool.Exec(ctx, base, "ext"+string(rune('a'+i)), r.kind, r.domains, r.actions, r.parent, r.scope, r.key)
		if (err == nil) != r.ok {
			t.Fatalf("%s: insert error = %v, want ok=%v", r.name, err, r.ok)
		}
	}
}

// 0006 is re-applied on every broker start; its down migration deletes
// extension rows first so none survives as another kind of grant.
func TestPg_Migration0006_IsIdempotentAndReversible(t *testing.T) {
	pool, _ := newTestDB(t)
	applyMigration(t, pool, "0006_browser_grants_extension.up.sql")
	applyMigration(t, pool, "0006_browser_grants_extension.up.sql")
	_, _, store, _ := stores(pool)
	_, run := extRun(t, store)
	if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "a.other.org")); err != nil {
		t.Fatal(err)
	}
	applyMigration(t, pool, "0006_browser_grants_extension.down.sql")
	applyMigration(t, pool, "0006_browser_grants_extension.down.sql")
	if n := rowCount(t, pool, "WHERE kind = 'origin_extension'"); n != 0 {
		t.Fatalf("down left %d extension rows", n)
	}
	var cols int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM information_schema.columns
		WHERE table_name = 'browser_grants' AND column_name = 'consent_scope'`).Scan(&cols); err != nil || cols != 0 {
		t.Fatalf("down left the consent_scope column (%d), %v", cols, err)
	}
	applyMigration(t, pool, "0006_browser_grants_extension.up.sql")
	pool.Reset()
	if _, err := store.GetScoped(ctx, run.ID, run.OrgID, run.OwnerID); err != nil {
		t.Fatalf("0006 down then up lost the run grant: %v", err)
	}
	if _, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "b.other.org")); err != nil {
		t.Fatalf("extension after down/up: %v", err)
	}
}
