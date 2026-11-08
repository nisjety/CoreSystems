package spaces

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

// WS10-S5a storage of the Space tool policy layers. A write replaces one whole
// layer in a single transaction under a compare-and-set on the layer revision.
// Rows hold tool keys and levels only.

// ErrToolPolicyConflict: the expected_revision no longer matches the layer.
var ErrToolPolicyConflict = errors.New("tool policy revision conflict")

func toolPolicyAgentKey(layer, agentSubjectID string) (string, error) {
	agentSubjectID = strings.TrimSpace(agentSubjectID)
	switch layer {
	case ToolPolicyLayerSpaceCeiling:
		if agentSubjectID != "" {
			return "", fmt.Errorf("the Space ceiling layer names no agent")
		}
		return "", nil
	case ToolPolicyLayerAgent:
		if agentSubjectID == "" || len(agentSubjectID) > maxApprovalIDLen {
			return "", fmt.Errorf("agent_subject_id is required for the agent layer")
		}
		return agentSubjectID, nil
	}
	return "", fmt.Errorf("unknown tool policy layer")
}

type toolPolicyQuerier interface {
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

func readToolPolicySet(ctx context.Context, q toolPolicyQuerier, spaceRef, orgID, layer, agentKey string) (ToolPolicySet, error) {
	set := ToolPolicySet{Layer: layer, SpaceRef: spaceRef, AgentSubjectID: agentKey, Entries: []ToolPolicyEntry{}}
	err := q.QueryRow(ctx, `
		SELECT revision, updated_by FROM space_tool_policy_sets
		WHERE space_ref=$1 AND org_id=$2 AND layer=$3 AND agent_key=$4`,
		spaceRef, orgID, layer, agentKey).Scan(&set.Revision, &set.UpdatedBy)
	if errors.Is(err, pgx.ErrNoRows) {
		return set, nil
	}
	if err != nil {
		return ToolPolicySet{}, fmt.Errorf("read tool policy revision: %w", err)
	}
	rows, err := q.Query(ctx, `
		SELECT tool_key, level FROM space_tool_policies
		WHERE space_ref=$1 AND org_id=$2 AND layer=$3 AND COALESCE(agent_subject_id,'')=$4
		ORDER BY tool_key`, spaceRef, orgID, layer, agentKey)
	if err != nil {
		return ToolPolicySet{}, fmt.Errorf("read tool policy entries: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var entry ToolPolicyEntry
		if err := rows.Scan(&entry.ToolKey, &entry.Level); err != nil {
			return ToolPolicySet{}, fmt.Errorf("scan tool policy entry: %w", err)
		}
		set.Entries = append(set.Entries, entry)
	}
	if err := rows.Err(); err != nil {
		return ToolPolicySet{}, fmt.Errorf("read tool policy entries: %w", err)
	}
	return set, nil
}

// GetToolPolicySet returns one stored layer, or an empty revision-0 layer when
// none is stored (which means allow, never a permissive widening).
func (r *Repository) GetToolPolicySet(ctx context.Context, spaceRef, orgID, layer, agentSubjectID string) (ToolPolicySet, error) {
	if r == nil || r.db == nil {
		return ToolPolicySet{}, fmt.Errorf("space authority repository unavailable")
	}
	agentKey, err := toolPolicyAgentKey(layer, agentSubjectID)
	if err != nil {
		return ToolPolicySet{}, err
	}
	return readToolPolicySet(ctx, r.db.Pool, strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), layer, agentKey)
}

// ListAgentToolPolicySets returns every agent layer stored for a Space (the
// editor shows them next to the ceiling), ordered by agent.
func (r *Repository) ListAgentToolPolicySets(ctx context.Context, spaceRef, orgID string) ([]ToolPolicySet, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("space authority repository unavailable")
	}
	spaceRef, orgID = strings.TrimSpace(spaceRef), strings.TrimSpace(orgID)
	rows, err := r.db.Pool.Query(ctx, `
		SELECT agent_key FROM space_tool_policy_sets
		WHERE space_ref=$1 AND org_id=$2 AND layer=$3 ORDER BY agent_key`,
		spaceRef, orgID, ToolPolicyLayerAgent)
	if err != nil {
		return nil, fmt.Errorf("list agent tool policies: %w", err)
	}
	var agents []string
	for rows.Next() {
		var agent string
		if err := rows.Scan(&agent); err != nil {
			rows.Close()
			return nil, fmt.Errorf("scan agent tool policy: %w", err)
		}
		agents = append(agents, agent)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list agent tool policies: %w", err)
	}
	sort.Strings(agents)
	sets := make([]ToolPolicySet, 0, len(agents))
	for _, agent := range agents {
		set, err := readToolPolicySet(ctx, r.db.Pool, spaceRef, orgID, ToolPolicyLayerAgent, agent)
		if err != nil {
			return nil, err
		}
		sets = append(sets, set)
	}
	return sets, nil
}

// MergedToolPolicyFor reads the Space ceiling and, when agentSubjectID is not
// empty, that agent's layer, and folds them. It is the one read both the editor
// route and the gate view use.
func (r *Repository) MergedToolPolicyFor(ctx context.Context, spaceRef, orgID, agentSubjectID string) (MergedToolPolicy, error) {
	ceiling, err := r.GetToolPolicySet(ctx, spaceRef, orgID, ToolPolicyLayerSpaceCeiling, "")
	if err != nil {
		return MergedToolPolicy{}, err
	}
	var agent *ToolPolicySet
	if strings.TrimSpace(agentSubjectID) != "" {
		set, err := r.GetToolPolicySet(ctx, spaceRef, orgID, ToolPolicyLayerAgent, agentSubjectID)
		if err != nil {
			return MergedToolPolicy{}, err
		}
		agent = &set
	}
	return MergeToolPolicy(&ceiling, agent), nil
}

// ReplaceToolPolicySet replaces one whole layer. The caller has already proven
// that the actor is a current manager or owner of the Space; a tool policy only
// tightens, so it needs no entitlement and is allowed under Zero Data Retention
// (it stores keys and levels, no content). An agent layer can only name an
// active service member of the Space.
func (r *Repository) ReplaceToolPolicySet(ctx context.Context, spaceRef, orgID, actorID, layer, agentSubjectID string, update ToolPolicyUpdate) (ToolPolicySet, error) {
	if r == nil || r.db == nil {
		return ToolPolicySet{}, fmt.Errorf("space authority repository unavailable")
	}
	if err := update.Validate(); err != nil {
		return ToolPolicySet{}, err
	}
	agentKey, err := toolPolicyAgentKey(layer, agentSubjectID)
	if err != nil {
		return ToolPolicySet{}, err
	}
	spaceRef, orgID, actorID = strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(actorID)
	if actorID == "" {
		return ToolPolicySet{}, fmt.Errorf("tool policy actor is required")
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return ToolPolicySet{}, fmt.Errorf("begin tool policy write: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	// Serialise writers of this Space and prove the Space belongs to the org.
	var locked int
	if err := tx.QueryRow(ctx, `
		SELECT 1 FROM space_authority_revisions r
		JOIN registered_spaces s ON s.space_ref = r.space_ref
		WHERE r.space_ref=$1 AND s.org_id=$2 FOR UPDATE OF r`, spaceRef, orgID).Scan(&locked); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return ToolPolicySet{}, ErrNoCurrentMembership
		}
		return ToolPolicySet{}, fmt.Errorf("lock Space for tool policy: %w", err)
	}
	if layer == ToolPolicyLayerAgent {
		var agentActive bool
		if err := tx.QueryRow(ctx, `
			SELECT EXISTS (SELECT 1 FROM space_memberships
			WHERE space_ref=$1 AND subject_type='service' AND subject_id=$2 AND active=TRUE)`,
			spaceRef, agentKey).Scan(&agentActive); err != nil {
			return ToolPolicySet{}, fmt.Errorf("check agent membership: %w", err)
		}
		if !agentActive {
			return ToolPolicySet{}, ErrApprovalAgentNotInSpace
		}
	}

	var current int64
	err = tx.QueryRow(ctx, `
		SELECT revision FROM space_tool_policy_sets
		WHERE space_ref=$1 AND layer=$2 AND agent_key=$3 FOR UPDATE`, spaceRef, layer, agentKey).Scan(&current)
	if errors.Is(err, pgx.ErrNoRows) {
		current = 0
	} else if err != nil {
		return ToolPolicySet{}, fmt.Errorf("read tool policy revision: %w", err)
	}
	if current != update.ExpectedRevision {
		return ToolPolicySet{}, ErrToolPolicyConflict
	}
	next := current + 1
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_tool_policy_sets (space_ref, layer, agent_key, org_id, revision, updated_by)
		VALUES ($1,$2,$3,$4,$5,$6)
		ON CONFLICT (space_ref, layer, agent_key)
		DO UPDATE SET revision=EXCLUDED.revision, updated_by=EXCLUDED.updated_by, updated_at=NOW()`,
		spaceRef, layer, agentKey, orgID, next, actorID); err != nil {
		return ToolPolicySet{}, fmt.Errorf("write tool policy revision: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		DELETE FROM space_tool_policies
		WHERE space_ref=$1 AND org_id=$2 AND layer=$3 AND COALESCE(agent_subject_id,'')=$4`,
		spaceRef, orgID, layer, agentKey); err != nil {
		return ToolPolicySet{}, fmt.Errorf("clear tool policy layer: %w", err)
	}
	for _, entry := range update.Entries {
		var agentRef any
		if layer == ToolPolicyLayerAgent {
			agentRef = agentKey
		}
		if _, err := tx.Exec(ctx, `
			INSERT INTO space_tool_policies
				(policy_id, org_id, layer, space_ref, agent_subject_id, tool_key, level, set_by_subject_id, revision)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)`,
			"tpol_"+uuid.NewString(), orgID, layer, spaceRef, agentRef, entry.ToolKey, string(entry.Level), actorID, next); err != nil {
			return ToolPolicySet{}, fmt.Errorf("write tool policy entry: %w", err)
		}
	}
	set, err := readToolPolicySet(ctx, tx, spaceRef, orgID, layer, agentKey)
	if err != nil {
		return ToolPolicySet{}, err
	}
	if err := tx.Commit(ctx); err != nil {
		return ToolPolicySet{}, fmt.Errorf("commit tool policy write: %w", err)
	}
	return set, nil
}
