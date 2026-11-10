// Package registry: the agent action admission table (WS24-S6).
//
// agent_action_admissions says which owner actions the Model may be offered and how each one is
// classified. A row alone admits nothing: Execution Core also pins the action's name, schema
// hash, parameters and adapter in code, and the owner verifies every call. This store only reads
// the table; the seeded rows are written by migration 0022.
package registry

import (
	"context"
	"fmt"
)

// AgentActionAdmissionEnabled is the only status under which an action may be offered.
const AgentActionAdmissionEnabled = "enabled"

// AgentActionAdmission is one row of agent_action_admissions.
type AgentActionAdmission struct {
	ActionID         string
	ToolName         string
	CapabilityID     string
	ModelAccess      string
	SideEffectClass  string
	ApprovalFloor    string
	Authority        string
	HashBasis        string
	ActionSchemaHash string
	ParametersJSON   string
	Description      string
	RequiresApproval bool
	Status           string
}

// AgentActionAdmissionsStore reads the admission table.
type AgentActionAdmissionsStore struct {
	pool capabilitiesDatabase
}

// NewAgentActionAdmissionsStore constructs a store over the capability-core pool.
func NewAgentActionAdmissionsStore(pool capabilitiesDatabase) (*AgentActionAdmissionsStore, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &AgentActionAdmissionsStore{pool: pool}, nil
}

// ListEnabled returns the enabled rows for the given action ids, ordered by action id. Declared
// and retired rows are never returned, so a row that is not enabled cannot be offered.
func (s *AgentActionAdmissionsStore) ListEnabled(ctx context.Context, actionIDs []string) ([]*AgentActionAdmission, error) {
	if len(actionIDs) == 0 {
		return nil, nil
	}
	rows, err := s.pool.Query(ctx, `
		SELECT action_id, tool_name, capability_id, model_access, side_effect_class, approval_floor,
		       authority, hash_basis, action_schema_hash, parameters_json, description,
		       requires_approval, status
		FROM agent_action_admissions
		WHERE status = $1 AND action_id = ANY($2)
		ORDER BY action_id
	`, AgentActionAdmissionEnabled, actionIDs)
	if err != nil {
		return nil, fmt.Errorf("list agent action admissions: %w", err)
	}
	defer rows.Close()

	var out []*AgentActionAdmission
	for rows.Next() {
		row := &AgentActionAdmission{}
		if err := rows.Scan(
			&row.ActionID, &row.ToolName, &row.CapabilityID, &row.ModelAccess, &row.SideEffectClass,
			&row.ApprovalFloor, &row.Authority, &row.HashBasis, &row.ActionSchemaHash,
			&row.ParametersJSON, &row.Description, &row.RequiresApproval, &row.Status,
		); err != nil {
			return nil, fmt.Errorf("scan agent action admission: %w", err)
		}
		out = append(out, row)
	}
	return out, rows.Err()
}

// ListAllEnabled returns every enabled row, ordered by action id. It backs the read-only
// owner-actions listing: declared and retired rows are never returned, so the list can only
// name actions the run view could offer.
func (s *AgentActionAdmissionsStore) ListAllEnabled(ctx context.Context) ([]*AgentActionAdmission, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT action_id, tool_name, capability_id, model_access, side_effect_class, approval_floor,
		       authority, hash_basis, action_schema_hash, parameters_json, description,
		       requires_approval, status
		FROM agent_action_admissions
		WHERE status = $1
		ORDER BY action_id
	`, AgentActionAdmissionEnabled)
	if err != nil {
		return nil, fmt.Errorf("list all enabled agent action admissions: %w", err)
	}
	defer rows.Close()

	var out []*AgentActionAdmission
	for rows.Next() {
		row := &AgentActionAdmission{}
		if err := rows.Scan(
			&row.ActionID, &row.ToolName, &row.CapabilityID, &row.ModelAccess, &row.SideEffectClass,
			&row.ApprovalFloor, &row.Authority, &row.HashBasis, &row.ActionSchemaHash,
			&row.ParametersJSON, &row.Description, &row.RequiresApproval, &row.Status,
		); err != nil {
			return nil, fmt.Errorf("scan agent action admission: %w", err)
		}
		out = append(out, row)
	}
	return out, rows.Err()
}
