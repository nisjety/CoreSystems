//go:build integration

package registry

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// setupToolClassDB starts the throwaway Postgres the registry tests share and
// applies every migration in order, the way docker-entrypoint.sh does at boot,
// so the seed runs against the capability rows the earlier migrations create.
func setupToolClassDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	pool := setupRegistryDB(t)
	applyMigrations(t, pool)
	return pool
}

func applyMigrations(t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	paths, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil || len(paths) == 0 {
		t.Fatalf("list migrations: %v", err)
	}
	for _, path := range paths {
		sqlBytes, readErr := os.ReadFile(path)
		if readErr != nil {
			t.Fatalf("read %s: %v", path, readErr)
		}
		if _, applyErr := pool.Exec(context.Background(), string(sqlBytes)); applyErr != nil {
			t.Fatalf("apply %s: %v", filepath.Base(path), applyErr)
		}
	}
}

func TestSeedClassifiesEveryBuiltInToolAndTheCeilingFollows(t *testing.T) {
	pool := setupToolClassDB(t)
	ctx := context.Background()

	want := readSeedRows(t)
	var count int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM capability_tools WHERE org_id = 'global' AND source = 'seed'`).Scan(&count); err != nil {
		t.Fatalf("count seed rows: %v", err)
	}
	if count != len(want) {
		t.Fatalf("seeded %d tools, the migration declares %d", count, len(want))
	}
	var unclassified int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM capability_tools WHERE side_effect_class = 'unclassified'`).Scan(&unclassified); err != nil {
		t.Fatalf("count unclassified: %v", err)
	}
	if unclassified != 0 {
		t.Fatalf("%d seeded tools are unclassified", unclassified)
	}

	ceilingClass := map[string]string{}
	ceilingFloor := map[string]string{}
	for _, row := range want {
		ceilingClass[row.capabilityID] = models.StricterSideEffectClass(orRead(ceilingClass[row.capabilityID]), row.class)
		ceilingFloor[row.capabilityID] = models.StricterApprovalFloor(orNone(ceilingFloor[row.capabilityID]), row.floor)
	}
	checked := 0
	for id, class := range ceilingClass {
		var gotClass, gotFloor string
		err := pool.QueryRow(ctx, `
			SELECT side_effect_class, approval_floor FROM capabilities
			WHERE id = $1 AND org_id = 'global' AND deleted_at IS NULL
		`, id).Scan(&gotClass, &gotFloor)
		if err != nil {
			// cap.skill.summarize lives only in the in-memory static seed.
			continue
		}
		checked++
		if gotClass != class || gotFloor != ceilingFloor[id] {
			t.Errorf("%s: ceiling %s/%s, want %s/%s", id, gotClass, gotFloor, class, ceilingFloor[id])
		}
	}
	if checked < 15 {
		t.Fatalf("only %d seeded capabilities exist as rows; the earlier migrations did not run", checked)
	}
}

func TestSeedRerunIsIdempotentAndNeverLowersOrRewritesAnOverride(t *testing.T) {
	pool := setupToolClassDB(t)
	ctx := context.Background()

	// An operator raised a seeded ceiling and overrode one tool's declaration.
	if _, err := pool.Exec(ctx, `UPDATE capabilities SET side_effect_class = 'destructive' WHERE id = 'cap.agent.spawn' AND org_id = 'global'`); err != nil {
		t.Fatalf("raise ceiling: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		UPDATE capability_tools SET side_effect_class = 'destructive', source = 'override'
		WHERE org_id = 'global' AND capability_id = 'cap.memory.search' AND tool_name = 'recall_memory'
	`); err != nil {
		t.Fatalf("override tool: %v", err)
	}

	applyMigrations(t, pool) // a second boot

	var ceiling string
	if err := pool.QueryRow(ctx, `SELECT side_effect_class FROM capabilities WHERE id = 'cap.agent.spawn' AND org_id = 'global'`).Scan(&ceiling); err != nil {
		t.Fatalf("read ceiling: %v", err)
	}
	if ceiling != models.SideEffectDestructive {
		t.Fatalf("a boot lowered a raised ceiling to %q", ceiling)
	}
	var class, source string
	if err := pool.QueryRow(ctx, `
		SELECT side_effect_class, source FROM capability_tools
		WHERE org_id = 'global' AND capability_id = 'cap.memory.search' AND tool_name = 'recall_memory'
	`).Scan(&class, &source); err != nil {
		t.Fatalf("read override: %v", err)
	}
	if class != models.SideEffectDestructive || source != "override" {
		t.Fatalf("a boot rewrote an override: %s/%s", class, source)
	}
	var count int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM capability_tools WHERE org_id = 'global'`).Scan(&count); err != nil {
		t.Fatalf("count: %v", err)
	}
	if count != len(readSeedRows(t)) {
		t.Fatalf("re-running the seed changed the row count to %d", count)
	}
}

func TestTheTablesRefuseWhatTheVocabularyForbids(t *testing.T) {
	pool := setupToolClassDB(t)
	ctx := context.Background()

	for name, statement := range map[string]string{
		"owner_effect below durable_owner on a tool": `INSERT INTO capability_tools (org_id, capability_id, tool_name, side_effect_class, approval_floor) VALUES ('org-a', 'cap.x', 't', 'owner_effect', 'ask')`,
		"unknown class on a tool":                    `INSERT INTO capability_tools (org_id, capability_id, tool_name, side_effect_class, approval_floor) VALUES ('org-a', 'cap.x', 't', 'readonly', 'none')`,
		"unknown floor on a tool":                    `INSERT INTO capability_tools (org_id, capability_id, tool_name, side_effect_class, approval_floor) VALUES ('org-a', 'cap.x', 't', 'read', 'sometimes')`,
		"unknown refinement":                         `INSERT INTO capability_tools (org_id, capability_id, tool_name, call_refinement) VALUES ('org-a', 'cap.x', 't', 'by_name')`,
		"unknown source":                             `INSERT INTO capability_tools (org_id, capability_id, tool_name, source) VALUES ('org-a', 'cap.x', 't', 'guess')`,
		"unknown review state":                       `INSERT INTO capability_tools (org_id, capability_id, tool_name, review_state) VALUES ('org-a', 'cap.x', 't', 'maybe')`,
		"owner_effect below durable_owner on a row":  `UPDATE capabilities SET side_effect_class = 'owner_effect', approval_floor = 'none' WHERE id = 'cap.command.shell'`,
		"unknown class on a row":                     `UPDATE capabilities SET side_effect_class = 'readonly' WHERE id = 'cap.command.shell'`,
	} {
		if _, err := pool.Exec(ctx, statement); err == nil {
			t.Errorf("%s: the table accepted it", name)
		}
	}

	// A qualified MCP name resolves to one row per org.
	insert := `INSERT INTO capability_tools (org_id, capability_id, tool_name, qualified_name) VALUES ($1, $2, $3, 'mcp__s__t')`
	if _, err := pool.Exec(ctx, insert, "org-a", "cap.mcp.a", "t"); err != nil {
		t.Fatalf("first qualified name: %v", err)
	}
	if _, err := pool.Exec(ctx, insert, "org-a", "cap.mcp.b", "t"); err == nil {
		t.Error("two rows of one org share a qualified name")
	}
	if _, err := pool.Exec(ctx, insert, "org-b", "cap.mcp.a", "t"); err != nil {
		t.Errorf("another org may use the same qualified name: %v", err)
	}
}

func TestListToolsForOrgShowsTheSeedAndOnlyTheCallersOwnRows(t *testing.T) {
	pool := setupToolClassDB(t)
	ctx := context.Background()
	store, err := NewCapabilitiesStore(pool)
	if err != nil {
		t.Fatalf("store: %v", err)
	}
	for org, tool := range map[string]string{"org-a": "a_tool", "org-b": "b_tool"} {
		if _, err := pool.Exec(ctx, `
			INSERT INTO capability_tools (org_id, capability_id, tool_name, side_effect_class, approval_floor, source)
			VALUES ($1, 'cap.custom', $2, 'external_write', 'none', 'override')
		`, org, tool); err != nil {
			t.Fatalf("insert %s: %v", tool, err)
		}
	}

	rows, err := store.ListToolsForOrg(ctx, "org-a", "")
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	seen := map[string]bool{}
	for _, row := range rows {
		seen[row.ToolName] = true
		if row.OrgID != "global" && row.OrgID != "org-a" {
			t.Errorf("org-a was shown a row of %s", row.OrgID)
		}
	}
	if !seen["a_tool"] || seen["b_tool"] || !seen["tickets.create"] || !seen["execute_provider_action"] {
		t.Fatalf("listing = %v, want the seed and org-a's own tool only", seen)
	}

	one, err := store.ListToolsForOrg(ctx, "org-a", "cap.tool.provider.execute")
	if err != nil {
		t.Fatalf("list one capability: %v", err)
	}
	if len(one) != 1 || one[0].ToolName != "execute_provider_action" ||
		one[0].CallRefinement != models.CallRefinementProviderOperation ||
		one[0].SideEffectClass != models.SideEffectExternalWrite {
		t.Fatalf("cap.tool.provider.execute tools = %+v", one)
	}
	ticket, err := store.ListToolsForOrg(ctx, "org-a", "cap.tool.ticket.create")
	if err != nil || len(ticket) != 1 || ticket[0].SideEffectClass != models.SideEffectOwnerEffect || ticket[0].ApprovalFloor != models.ApprovalFloorDurableOwner {
		t.Fatalf("ticket tool = %+v, err = %v", ticket, err)
	}
}
