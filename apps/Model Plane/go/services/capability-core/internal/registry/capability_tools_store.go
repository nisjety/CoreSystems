// Package registry — the per-tool half of the capability registry.
//
// A capability row carries one ceiling for everything behind it; capability_tools
// carries the declared class and approval floor of each tool (WS05-S1). Only
// the read side exists here: the rows are seeded by migration 0016 and, for MCP
// tools, will be written by observation (WS05-S2), never by a generic upsert.
package registry

import (
	"context"
	"fmt"
	"time"
)

// CapabilityToolRow is one declared tool: the class and floor a call to it is
// governed by, and where that declaration came from.
type CapabilityToolRow struct {
	OrgID        string
	CapabilityID string
	ToolName     string
	// SideEffectClass and ApprovalFloor are the stored declaration, never an
	// inference: models.NormalizeSideEffectClass applies only on the wire.
	SideEffectClass string
	ApprovalFloor   string
	// CallRefinement is empty for every tool except execute_provider_action,
	// whose row is the write class and whose catalogued read operations may
	// lower to read (models.CallRefinementProviderOperation).
	CallRefinement string
	// Source is `seed` (a migration), `derived` (observation) or `override` (an
	// authorised operator edit). Seed re-runs never rewrite an override.
	Source    string
	CreatedAt time.Time
	UpdatedAt time.Time
}

// ListToolsForOrg returns the declared tools the verified tenant may see: its
// own rows and the process-wide `global` seed. A non-empty capabilityID narrows
// to one capability. Rows come back ordered by capability then tool name, so a
// listing is stable across calls.
func (s *CapabilitiesStore) ListToolsForOrg(ctx context.Context, orgID, capabilityID string) ([]*CapabilityToolRow, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT org_id, capability_id, tool_name, side_effect_class, approval_floor,
		       COALESCE(call_refinement, ''), source, created_at, updated_at
		FROM capability_tools
		WHERE (org_id = $1 OR org_id = 'global')
		  AND ($2 = '' OR capability_id = $2)
		ORDER BY capability_id, tool_name, org_id
	`, orgID, capabilityID)
	if err != nil {
		return nil, fmt.Errorf("list capability tools: %w", err)
	}
	defer rows.Close()

	var out []*CapabilityToolRow
	for rows.Next() {
		r := &CapabilityToolRow{}
		if err := rows.Scan(
			&r.OrgID, &r.CapabilityID, &r.ToolName, &r.SideEffectClass, &r.ApprovalFloor,
			&r.CallRefinement, &r.Source, &r.CreatedAt, &r.UpdatedAt,
		); err != nil {
			return nil, fmt.Errorf("scan capability tool: %w", err)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}
