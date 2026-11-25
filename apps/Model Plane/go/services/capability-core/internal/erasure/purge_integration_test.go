//go:build integration

package erasure

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// setupMigratedDatabase starts a throwaway Postgres and applies every capability-core migration in name order, as
// docker-entrypoint.sh does. Session-core's migrations are not applied, so space_watches is absent and the purge must
// skip it without failing.
func setupMigratedDatabase(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 180*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("capability_core"),
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

	files, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil {
		t.Fatalf("list migrations: %v", err)
	}
	sort.Strings(files)
	for _, file := range files {
		sqlBytes, err := os.ReadFile(file)
		if err != nil {
			t.Fatalf("read %s: %v", filepath.Base(file), err)
		}
		if _, err := pool.Exec(ctx, string(sqlBytes)); err != nil {
			t.Fatalf("apply %s: %v", filepath.Base(file), err)
		}
	}
	return pool
}

// seedRow is one seed statement. Each is a single statement, so it can carry its own arguments.
type seedRow struct {
	sql  string
	args []any
}

// seedRowsFor returns one row in every org-scoped table for org, and the child rows the foreign keys reach through them.
// n makes the procedure id, which must match migration 0043's shape. The same rows are written for org-1 (the erased
// organization), org-2 and the platform scope, so the test can prove that only org-1's rows go.
func seedRowsFor(org string, n int) []seedRow {
	hash := strings.Repeat("a", 64)
	procedureID := fmt.Sprintf("proc_%08x-0000-4000-8000-%012x", n, n)
	ruleID := fmt.Sprintf("%08x-0000-4000-8000-%012x", 0x10+n, 0x10+n)
	return []seedRow{
		{`INSERT INTO learning_review_counters (key, org_id, count, expires_at) VALUES ($1, $2, 3, now() + interval '1 day')`,
			[]any{"learning-review:org:" + org + ":2026-11-20", org}},
		{`INSERT INTO run_watch_subscriptions (id, org_id, run_id, user_id) VALUES ($1, $2, 'run-1', 'user-1')`,
			[]any{"rw-" + org, org}},
		{`INSERT INTO tool_policies (id, org_id, scope_kind, scope_value, capability_id, decision, set_by) VALUES ($1, $2, 'org', $2, $3, 'ask', 'admin')`,
			[]any{"tp-" + org, org, "cap-" + org}},
		{`INSERT INTO capability_tools (org_id, capability_id, tool_name) VALUES ($1, $2, 'tool_a')`,
			[]any{org, "cap-" + org}},
		{`INSERT INTO capabilities (id, org_id, kind, name) VALUES ($1, $2, 'mcp_tool', $3)`,
			[]any{"cap-" + org, org, "tool-" + org}},
		{`INSERT INTO capability_versions (id, capability_id, version) VALUES ($1, $2, '1.0.0')`,
			[]any{"cv-" + org, "cap-" + org}},
		{`INSERT INTO capability_health (id, capability_id) VALUES ($1, $2)`,
			[]any{"ch-" + org, "cap-" + org}},
		{`INSERT INTO capability_scopes (id, capability_id, scope_kind, scope_value, org_id) VALUES ($1, $2, 'workspace', '*', $3)`,
			[]any{"cs-" + org, "cap-" + org, org}},
		{`INSERT INTO skill_packages (id, org_id, name, version) VALUES ($1, $2, $3, '1.0.0')`,
			[]any{"skill-" + org, org, "skill-" + org}},
		{`INSERT INTO skill_resources (id, skill_id, name) VALUES ($1, $2, 'readme')`,
			[]any{"res-" + org, "skill-" + org}},
		{`INSERT INTO mcp_servers (id, org_id, name, endpoint_url) VALUES ($1, $2, $3, 'https://mcp.example.test/srv')`,
			[]any{"mcp-" + org, org, "srv-" + org}},
		{`INSERT INTO mcp_oauth_tokens (id, server_id, org_id) VALUES ($1, $2, $3)`,
			[]any{"tok-" + org, "mcp-" + org, org}},
		{`INSERT INTO plugin_packages (id, org_id, name, version) VALUES ($1, $2, $3, '1.0.0')`,
			[]any{"plug-" + org, org, "plug-" + org}},
		{`INSERT INTO plugin_components (plugin_id, kind, name, status) VALUES ($1, 'skill', 'c1', 'installed')`,
			[]any{"plug-" + org}},
		{`INSERT INTO plugin_kill_list (digest, org_id) VALUES ($1, $2)`,
			[]any{"sha256:" + hash, org}},
		{`INSERT INTO skill_bundles (id, org_id, skill_id, manifest_sha256, file_count, total_bytes, created_by) VALUES ($1, $2, $3, $4, 1, 10, 'admin')`,
			[]any{"bundle-" + org, org, "skill-b-" + org, hash}},
		{`INSERT INTO skill_bundle_files (bundle_id, path, sha256, size_bytes) VALUES ($1, 'SKILL.md', $2, 10)`,
			[]any{"bundle-" + org, hash}},
		{`INSERT INTO skill_bundle_blobs (org_id, sha256, content, size_bytes) VALUES ($1, $2, decode('6162', 'hex'), 2)`,
			[]any{org, hash}},
		{`INSERT INTO org_hook_rules (id, org_id, event, tool_matcher, decision, reason, created_by, updated_by) VALUES ($1, $2, 'pre_tool', 'shell.*', 'deny', 'no shell', 'admin', 'admin')`,
			[]any{"hook-" + org, org}},
		{`INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, org_id) VALUES ($1, 'capability', $2, 'created', $3)`,
			[]any{"audit-" + org, "cap-" + org, org}},
		{`INSERT INTO procedures (id, org_id, owner_user_id, name) VALUES ($1, $2, 'user-1', 'recipe')`,
			[]any{procedureID, org}},
		{`INSERT INTO procedure_versions (procedure_id, org_id, version, digest, body, step_count) VALUES ($1, $2, 1, $3, '{}'::jsonb, 1)`,
			[]any{procedureID, org, hash}},
		{`INSERT INTO procedure_tags (procedure_id, tag, version) VALUES ($1, 'stable', 1)`,
			[]any{procedureID}},
		{`INSERT INTO org_agent_policy (org_id, revision, doc, digest, updated_by) VALUES ($1, 1, '{}'::jsonb, $2, 'admin')`,
			[]any{org, hash}},
		{`INSERT INTO org_agent_policy_history (id, org_id, revision, doc, digest, actor) VALUES ($1, $2, 1, '{}'::jsonb, $3, 'admin')`,
			[]any{"aph-" + org, org, hash}},
		{`INSERT INTO org_permission_rules (rule_id, org_id, tool_pattern, target_kind, target_pattern, decision, created_by) VALUES ($1::text::uuid, $2, 'shell.*', 'any', '*', 'deny', 'admin')`,
			[]any{ruleID, org}},
		{`INSERT INTO routing_policies (id, org_id, name) VALUES ($1, $2, $3)`,
			[]any{"route-" + org, org, "route-" + org}},
		{`INSERT INTO safety_policies (id, org_id, name) VALUES ($1, $2, $3)`,
			[]any{"safety-" + org, org, "safety-" + org}},
	}
}

// seedAllOrganizations writes the same rows for org-1, org-2 and the platform scope.
func seedAllOrganizations(ctx context.Context, t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	for n, org := range []string{"org-1", "org-2", "global"} {
		for _, row := range seedRowsFor(org, n+1) {
			if _, err := pool.Exec(ctx, row.sql, row.args...); err != nil {
				t.Fatalf("seed for %s: %v\n%s", org, err, row.sql)
			}
		}
	}
}

// orgScopedTables lists every table of the migrated schema that has an org_id column.
func orgScopedTables(ctx context.Context, t *testing.T, pool *pgxpool.Pool) []string {
	t.Helper()
	rows, err := pool.Query(ctx, `SELECT table_name FROM information_schema.columns
		WHERE table_schema = current_schema() AND column_name = 'org_id' ORDER BY table_name`)
	if err != nil {
		t.Fatalf("list org-scoped tables: %v", err)
	}
	defer rows.Close()
	var tables []string
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			t.Fatalf("scan table name: %v", err)
		}
		tables = append(tables, name)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("read table names: %v", err)
	}
	return tables
}

// countOrgRows returns how many rows each table holds for org. The comparison is on text, so the uuid column of models
// compares too.
func countOrgRows(ctx context.Context, t *testing.T, pool *pgxpool.Pool, tables []string, org string) map[string]int64 {
	t.Helper()
	counts := make(map[string]int64, len(tables))
	for _, table := range tables {
		var n int64
		query := fmt.Sprintf(`SELECT count(*) FROM %s WHERE org_id::text = $1`, pgx.Identifier{table}.Sanitize())
		if err := pool.QueryRow(ctx, query, org).Scan(&n); err != nil {
			t.Fatalf("count %s rows of %s: %v", table, org, err)
		}
		counts[table] = n
	}
	return counts
}

func countWhere(ctx context.Context, t *testing.T, pool *pgxpool.Pool, query string, args ...any) int64 {
	t.Helper()
	var n int64
	if err := pool.QueryRow(ctx, query, args...).Scan(&n); err != nil {
		t.Fatalf("count (%s): %v", query, err)
	}
	return n
}

// TestPurgeOrgDeletesOnlyTheOrganizationsRowsInEveryTable runs the purge against the real migrated schema. It proves the
// statements execute in foreign-key order and under the procedure trigger, that org-1's rows are gone everywhere
// (including the child rows that carry no org_id), that org-2's and the platform's rows are untouched, and that a
// second purge deletes nothing.
func TestPurgeOrgDeletesOnlyTheOrganizationsRowsInEveryTable(t *testing.T) {
	pool := setupMigratedDatabase(t)
	ctx := context.Background()
	seedAllOrganizations(ctx, t, pool)

	tables := orgScopedTables(ctx, t, pool)
	if len(tables) < 20 {
		t.Fatalf("the migrated schema has %d org-scoped tables (%v); expected at least 20", len(tables), tables)
	}
	if n := countWhere(ctx, t, pool, `SELECT count(*) FROM information_schema.tables WHERE table_name = 'space_watches'`); n != 0 {
		t.Fatal("space_watches exists, but only capability-core's migrations are applied here: the skip path is untested")
	}

	before2 := countOrgRows(ctx, t, pool, tables, "org-2")
	beforeGlobal := countOrgRows(ctx, t, pool, tables, "global")
	before1 := countOrgRows(ctx, t, pool, tables, "org-1")
	for _, table := range tables {
		if before1[table] == 0 && table != "models" {
			t.Fatalf("seed left no org-1 row in %s, so the purge of that table is untested", table)
		}
	}

	purger := NewPgPurger(pool)
	deleted, err := purger.PurgeOrg(ctx, "org-1")
	if err != nil {
		t.Fatalf("purge org-1: %v", err)
	}
	if deleted == 0 {
		t.Fatal("the purge deleted no rows, although org-1 owns rows")
	}

	for _, table := range tables {
		if n := countOrgRows(ctx, t, pool, []string{table}, "org-1")[table]; n != 0 {
			t.Errorf("%s still holds %d row(s) of org-1 after the purge", table, n)
		}
	}
	after2 := countOrgRows(ctx, t, pool, tables, "org-2")
	afterGlobal := countOrgRows(ctx, t, pool, tables, "global")
	for _, table := range tables {
		if after2[table] != before2[table] {
			t.Errorf("%s: org-2 had %d row(s) before the purge and %d after; another organization's rows must survive",
				table, before2[table], after2[table])
		}
		if afterGlobal[table] != beforeGlobal[table] {
			t.Errorf("%s: platform rows went from %d to %d; the purge must never touch the platform scope",
				table, beforeGlobal[table], afterGlobal[table])
		}
	}

	// Child rows that carry no org_id are removed through their parent.
	childChecks := []struct {
		name, query string
		arg         string
	}{
		{"capability_versions", `SELECT count(*) FROM capability_versions WHERE capability_id = $1`, "cap-org-1"},
		{"capability_health", `SELECT count(*) FROM capability_health WHERE capability_id = $1`, "cap-org-1"},
		{"skill_resources", `SELECT count(*) FROM skill_resources WHERE skill_id = $1`, "skill-org-1"},
		{"skill_bundle_files", `SELECT count(*) FROM skill_bundle_files WHERE bundle_id = $1`, "bundle-org-1"},
		{"plugin_components", `SELECT count(*) FROM plugin_components WHERE plugin_id = $1`, "plug-org-1"},
		{"procedure_tags", `SELECT count(*) FROM procedure_tags WHERE procedure_id = $1`, "proc_00000001-0000-4000-8000-000000000001"},
	}
	for _, check := range childChecks {
		if n := countWhere(ctx, t, pool, check.query, check.arg); n != 0 {
			t.Errorf("%s still holds %d row(s) of the erased organization", check.name, n)
		}
		survivor := strings.Replace(check.arg, "-org-1", "-org-2", 1)
		if strings.HasPrefix(check.arg, "proc_") {
			survivor = "proc_00000002-0000-4000-8000-000000000002"
		}
		if n := countWhere(ctx, t, pool, check.query, survivor); n == 0 {
			t.Errorf("%s lost the row of org-2 (%s); the purge must not reach another organization", check.name, survivor)
		}
	}

	// The erasure setting lasts for the purge's transaction only: outside one, a version is still immutable.
	if _, err := pool.Exec(ctx, `DELETE FROM procedure_versions WHERE org_id = 'org-2'`); err == nil {
		t.Fatal("procedure_versions accepted a direct delete outside an erasure; the setting leaked out of the transaction")
	}

	again, err := purger.PurgeOrg(ctx, "org-1")
	if err != nil {
		t.Fatalf("second purge of org-1: %v", err)
	}
	if again != 0 {
		t.Fatalf("a repeated purge deleted %d row(s); it must delete nothing", again)
	}
}

// TestPurgeOrgRemovesAUUIDOrganizationsModelRowsOnly pins the models step: a text organization id owns no model row and
// the step is skipped, and a UUID organization's model row goes only when that organization is purged.
func TestPurgeOrgRemovesAUUIDOrganizationsModelRowsOnly(t *testing.T) {
	pool := setupMigratedDatabase(t)
	ctx := context.Background()
	const orgUUID = "11111111-1111-4111-8111-111111111111"
	if _, err := pool.Exec(ctx,
		`INSERT INTO models (id, org_id, provider, name, version) VALUES (gen_random_uuid(), $1::text::uuid, 'azure', 'model-1', '1')`,
		orgUUID); err != nil {
		t.Fatalf("seed a model for the UUID organization: %v", err)
	}
	modelsOf := func() int64 {
		return countWhere(ctx, t, pool, `SELECT count(*) FROM models WHERE org_id = $1::text::uuid`, orgUUID)
	}

	purger := NewPgPurger(pool)
	if _, err := purger.PurgeOrg(ctx, "org-1"); err != nil {
		t.Fatalf("purge of a text organization id must not fail on the uuid column: %v", err)
	}
	if n := modelsOf(); n != 1 {
		t.Fatalf("a text organization id reached the UUID organization's model rows: %d left, want 1", n)
	}

	if _, err := purger.PurgeOrg(ctx, strings.ToUpper(orgUUID)); err != nil {
		t.Fatalf("purge of the UUID organization: %v", err)
	}
	if n := modelsOf(); n != 0 {
		t.Fatalf("the UUID organization's model rows survived its purge: %d left", n)
	}
}
