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

// setupBindingDB starts a throwaway Postgres with every migration applied, the
// way docker-entrypoint.sh does at boot, so the seed rows exist.
func setupBindingDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	container, err := tcpostgres.Run(ctx, "postgres:16-alpine",
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
		t.Fatalf("conn string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)
	paths, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil || len(paths) == 0 {
		t.Fatalf("list migrations: %v", err)
	}
	for _, path := range paths {
		sqlBytes, readErr := os.ReadFile(path)
		if readErr != nil {
			t.Fatalf("read %s: %v", path, readErr)
		}
		if _, applyErr := pool.Exec(ctx, string(sqlBytes)); applyErr != nil {
			t.Fatalf("apply %s: %v", filepath.Base(path), applyErr)
		}
	}
	return pool
}

func mustExec(t *testing.T, pool *pgxpool.Pool, sql string, args ...any) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), sql, args...); err != nil {
		t.Fatalf("exec %q: %v", sql, err)
	}
}

// toolBindingSQL must resolve a built-in name only to a built-in row, an mcp
// name only to the tenant's own row with that qualified name, and must ignore a
// tool whose capability is deleted or belongs to another tenant.
func TestToolBindingSQLResolvesOnlyWhatTheTenantMaySee(t *testing.T) {
	pool := setupBindingDB(t)
	ctx := context.Background()
	const digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

	insertCapability := `
		INSERT INTO capabilities (id, org_id, kind, name, version, risk_level, side_effect_class, approval_floor)
		VALUES ($1, $2, 'mcp_tool', $3, $4, 'low', 'read', 'none')`
	insertTool := `
		INSERT INTO capability_tools (org_id, capability_id, tool_name, side_effect_class, approval_floor, source,
		                              server_id, qualified_name, pinned_digest, observed_digest, review_state)
		VALUES ($1, $2, $3, $4, 'none', 'derived', 'srv', $5, $6, $6, $7)`

	// Tenant A's MCP tool is named like a built-in on purpose.
	mustExec(t, pool, insertCapability, "cap.mcp_tool.a", "org-a", "mcp__srv__shell", "1.0.0-0123456789ab")
	mustExec(t, pool, insertTool, "org-a", "cap.mcp_tool.a", "shell", "read", "mcp__srv__shell", digest, "approved")
	// Tenant B holds the same qualified name; tenant A must never see it.
	mustExec(t, pool, insertCapability, "cap.mcp_tool.b", "org-b", "mcp__srv__only_b", "1.0.0-aaaaaaaaaaaa")
	mustExec(t, pool, insertTool, "org-b", "cap.mcp_tool.b", "only_b", "read", "mcp__srv__only_b", digest, "approved")
	// A tool whose capability row was deleted.
	mustExec(t, pool, insertCapability, "cap.mcp_tool.dead", "org-a", "mcp__srv__dead", "1.0.0-bbbbbbbbbbbb")
	mustExec(t, pool, insertTool, "org-a", "cap.mcp_tool.dead", "dead", "read", "mcp__srv__dead", digest, "approved")
	mustExec(t, pool, `UPDATE capabilities SET deleted_at = now() WHERE id = 'cap.mcp_tool.dead'`)
	// A tenant row that tries to loosen the seeded class of a built-in.
	mustExec(t, pool, `
		INSERT INTO capability_tools (org_id, capability_id, tool_name, side_effect_class, approval_floor, source)
		VALUES ('org-a', 'cap.command.shell', 'shell', 'read', 'none', 'override')`)

	server := &Server{toolDB: pool}
	found, err := server.lookupToolBindings(ctx, "org-a",
		[]string{"shell", "mcp__srv__shell", "mcp__srv__only_b", "mcp__srv__dead", "web_fetch", "echo", "dead"})
	if err != nil {
		t.Fatalf("lookupToolBindings: %v", err)
	}

	shell := found.forCapability("shell", "cap.command.shell")
	if shell == nil || shell.SideEffectClass != "destructive" || shell.ReviewState != "" || shell.ToolDigest != "" {
		t.Fatalf("built-in shell binding = %+v (a tenant override must not loosen the seed)", shell)
	}
	if len(found["shell"]) != 1 {
		t.Fatalf("the built-in name matched %d capabilities; the MCP row named shell must not match it", len(found["shell"]))
	}
	mcp := found.forCapability("mcp__srv__shell", "cap.mcp_tool.a")
	if mcp == nil || mcp.ToolDigest != digest || mcp.ReviewState != "approved" || mcp.CapabilityVersion != "1.0.0-0123456789ab" || mcp.SideEffectClass != "read" {
		t.Fatalf("mcp binding = %+v", mcp)
	}
	if found["mcp__srv__only_b"] != nil || found["mcp__srv__dead"] != nil {
		t.Fatalf("another tenant's or a deleted tool resolved: %v", found)
	}
	if found["web_fetch"] == nil {
		t.Fatalf("a seeded built-in did not resolve: %v", found)
	}
	// An unqualified MCP tool name is not a name anyone may call.
	if found["echo"] != nil || found["dead"] != nil {
		t.Fatalf("a raw MCP tool name resolved: %v", found)
	}

	// Tenant B resolves its own row, not tenant A's.
	other, err := server.lookupToolBindings(ctx, "org-b", []string{"mcp__srv__shell", "mcp__srv__only_b"})
	if err != nil {
		t.Fatalf("lookupToolBindings(org-b): %v", err)
	}
	if other["mcp__srv__shell"] != nil || other.unique("mcp__srv__only_b") == nil {
		t.Fatalf("tenant isolation failed: %v", other)
	}
}
