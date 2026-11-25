package erasure

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// versionErasureSetting is the setting migration 0043's procedure_versions trigger reads. Only an erasure sets it, and
// the transaction sets it with is_local, so it ends with the purge.
const versionErasureSetting = "mp.version_erasure"

// purgeStep deletes one table's rows for one organization. Every statement takes the organization id as $1.
type purgeStep struct {
	table string
	sql   string
	// uuidOrg marks models.org_id, a uuid column. An organization id that is not a UUID owns no model row, so that
	// step is skipped rather than failing the whole purge.
	uuidOrg bool
}

// orgPurgeSteps lists every capability-core table that carries an org_id, plus the children that the foreign keys
// reach through a parent, in the order that respects those foreign keys: a child goes before the parent it references,
// and the procedures chain goes last, under the erasure setting. The coverage test in purge_coverage_test.go fails when
// a migration adds an org-scoped table that this list does not purge.
//
// space_watches and space_watch_events are created by session-core's migration 0036 on this same database, and the
// capability-core watch store writes them. A step whose table is absent is skipped, because an absent table holds no
// rows.
var orgPurgeSteps = []purgeStep{
	// The day counters of the learning review (0045): ids and counts only.
	{table: "learning_review_counters", sql: `DELETE FROM learning_review_counters WHERE org_id = $1`},
	// Run watches (0012).
	{table: "run_watch_subscriptions", sql: `DELETE FROM run_watch_subscriptions WHERE org_id = $1`},
	// Space watches (session-core 0036): events reference their watch, so the events go first.
	{table: "space_watch_events", sql: `DELETE FROM space_watch_events WHERE org_id = $1`},
	{table: "space_watches", sql: `DELETE FROM space_watches WHERE org_id = $1`},
	// Tool policies (0017), tool classes (0016) and the registry audit log (0003).
	{table: "tool_policies", sql: `DELETE FROM tool_policies WHERE org_id = $1`},
	{table: "capability_tools", sql: `DELETE FROM capability_tools WHERE org_id = $1`},
	{table: "registry_audit_log", sql: `DELETE FROM registry_audit_log WHERE org_id = $1`},
	// Organization policy and rules (0023, 0028, 0039, 0040, 0003).
	{table: "org_agent_policy_history", sql: `DELETE FROM org_agent_policy_history WHERE org_id = $1`},
	{table: "org_agent_policy", sql: `DELETE FROM org_agent_policy WHERE org_id = $1`},
	{table: "org_hook_rules", sql: `DELETE FROM org_hook_rules WHERE org_id = $1`},
	{table: "org_permission_rules", sql: `DELETE FROM org_permission_rules WHERE org_id = $1`},
	{table: "plugin_kill_list", sql: `DELETE FROM plugin_kill_list WHERE org_id = $1`},
	{table: "routing_policies", sql: `DELETE FROM routing_policies WHERE org_id = $1`},
	{table: "safety_policies", sql: `DELETE FROM safety_policies WHERE org_id = $1`},
	// Skill bundles (0021): files reference their bundle, and the blobs are keyed by organization.
	{table: "skill_bundle_files", sql: `DELETE FROM skill_bundle_files WHERE bundle_id IN (SELECT id FROM skill_bundles WHERE org_id = $1)`},
	{table: "skill_bundles", sql: `DELETE FROM skill_bundles WHERE org_id = $1`},
	{table: "skill_bundle_blobs", sql: `DELETE FROM skill_bundle_blobs WHERE org_id = $1`},
	// Skill packages (0003): resources reference their package.
	{table: "skill_resources", sql: `DELETE FROM skill_resources WHERE skill_id IN (SELECT id FROM skill_packages WHERE org_id = $1)`},
	{table: "skill_packages", sql: `DELETE FROM skill_packages WHERE org_id = $1`},
	// MCP servers (0003) and their OAuth tokens: tokens reference their server, and the credentials of an organization's
	// servers go with them.
	{table: "mcp_oauth_tokens", sql: `DELETE FROM mcp_oauth_tokens WHERE org_id = $1 OR server_id IN (SELECT id FROM mcp_servers WHERE org_id = $1)`},
	{table: "mcp_servers", sql: `DELETE FROM mcp_servers WHERE org_id = $1`},
	// Plugins (0003, 0039): components reference their package.
	{table: "plugin_components", sql: `DELETE FROM plugin_components WHERE plugin_id IN (SELECT id FROM plugin_packages WHERE org_id = $1)`},
	{table: "plugin_packages", sql: `DELETE FROM plugin_packages WHERE org_id = $1`},
	// Capabilities (0003, 0007): versions, health and scopes reference their capability. A scope can also belong to the
	// organization through its own org_id while it names a platform capability, so it is matched both ways.
	{table: "capability_versions", sql: `DELETE FROM capability_versions WHERE capability_id IN (SELECT id FROM capabilities WHERE org_id = $1)`},
	{table: "capability_health", sql: `DELETE FROM capability_health WHERE capability_id IN (SELECT id FROM capabilities WHERE org_id = $1)`},
	{table: "capability_scopes", sql: `DELETE FROM capability_scopes WHERE org_id = $1 OR capability_id IN (SELECT id FROM capabilities WHERE org_id = $1)`},
	{table: "capabilities", sql: `DELETE FROM capabilities WHERE org_id = $1`},
	// Model rows (0001): the organization's own models, when its id is a UUID.
	{table: "models", sql: `DELETE FROM models WHERE org_id = $1::text::uuid`, uuidOrg: true},
	// Saved browser procedures (0043): tags reference their version and procedure, and versions are immutable except
	// under the erasure setting that the transaction sets first.
	{table: "procedure_tags", sql: `DELETE FROM procedure_tags WHERE procedure_id IN (SELECT id FROM procedures WHERE org_id = $1)`},
	{table: "procedure_versions", sql: `DELETE FROM procedure_versions WHERE org_id = $1`},
	{table: "procedures", sql: `DELETE FROM procedures WHERE org_id = $1`},
}

// PgPurger deletes an organization's capability-core rows from the database the service runs on. It deletes by
// organization id and never reads row content to decide what to delete.
type PgPurger struct {
	pool *pgxpool.Pool
}

// NewPgPurger returns the Postgres purger over pool.
func NewPgPurger(pool *pgxpool.Pool) *PgPurger {
	return &PgPurger{pool: pool}
}

// PurgeOrg deletes every capability-core row that orgID owns in one transaction and returns how many rows it deleted. An
// empty organization id, or the platform scope, is refused before the database is touched.
func (p *PgPurger) PurgeOrg(ctx context.Context, orgID string) (int64, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" || orgID == platformScope {
		return 0, errors.New("erasure: refusing to purge an empty or platform organization id")
	}
	if p == nil || p.pool == nil {
		return 0, errors.New("erasure: org purge has no database")
	}
	tx, err := p.pool.Begin(ctx)
	if err != nil {
		return 0, fmt.Errorf("erasure: begin org purge: %w", err)
	}
	defer func() { _ = tx.Rollback(context.Background()) }()

	if _, err := tx.Exec(ctx, `SELECT set_config($1, 'on', true)`, versionErasureSetting); err != nil {
		return 0, fmt.Errorf("erasure: set the erasure setting: %w", err)
	}
	present, err := existingTables(ctx, tx)
	if err != nil {
		return 0, err
	}
	var deleted int64
	for _, step := range orgPurgeSteps {
		if !present[step.table] {
			continue
		}
		arg := orgID
		if step.uuidOrg {
			parsed, parseErr := uuid.Parse(orgID)
			if parseErr != nil {
				continue
			}
			arg = parsed.String()
		}
		tag, execErr := tx.Exec(ctx, step.sql, arg)
		if execErr != nil {
			return 0, fmt.Errorf("erasure: purge %s: %w", step.table, execErr)
		}
		deleted += tag.RowsAffected()
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, fmt.Errorf("erasure: commit org purge: %w", err)
	}
	return deleted, nil
}

// existingTables returns which of the purge's tables exist on this database. An absent table holds no rows, so its step
// is skipped. The rows are closed before the purge's first delete runs on the same transaction.
func existingTables(ctx context.Context, tx pgx.Tx) (map[string]bool, error) {
	names := make([]string, 0, len(orgPurgeSteps))
	for _, step := range orgPurgeSteps {
		names = append(names, step.table)
	}
	rows, err := tx.Query(ctx, `SELECT t FROM unnest($1::text[]) AS t WHERE to_regclass(t) IS NOT NULL`, names)
	if err != nil {
		return nil, fmt.Errorf("erasure: list purge tables: %w", err)
	}
	defer rows.Close()
	present := make(map[string]bool, len(names))
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			return nil, fmt.Errorf("erasure: read purge tables: %w", err)
		}
		present[name] = true
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("erasure: read purge tables: %w", err)
	}
	return present, nil
}
