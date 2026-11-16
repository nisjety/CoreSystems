package spaces

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/jackc/pgx/v5"
)

// WS17-S4 cascade A (ADR 4): an agent whose owner is no longer an active member of
// the organization is suspended without waiting for any event. The sweep runs in the
// background every AgentAdmissionSweepInterval. It is started only when
// SPACE_AGENT_ADMISSION is observe or enforce. Its only effect is to remove access.
// There is no auto-resume: the owner re-admits.

// AgentAdmissionSweepInterval is the cadence of the owner-inactivity sweep.
const AgentAdmissionSweepInterval = 60 * time.Second

// agentAdmissionSweepLock is the transaction advisory lock that elects one sweeping
// replica per tick. A replica that does not get it skips the tick.
const agentAdmissionSweepLock int64 = 735200417

// agentSweepActor is the ledger actor of a sweep. It is a label, never a user id.
const agentSweepActor = "sweeper:owner-inactive"

// SweepInactiveOwnerAgents suspends every agent whose owner is no longer an active
// organization member: the agent definition, its admissions in every room, and its
// service memberships in those rooms. One transaction per tick. It returns the number
// of agent definitions suspended by this pass. Only owner-visible (personal) agents are swept: an
// org-visible agent is org-owned (D10), so its registrant leaving the organization does not suspend it.
func (r *Repository) SweepInactiveOwnerAgents(ctx context.Context) (int, error) {
	if r == nil || r.db == nil {
		return 0, nil
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return 0, fmt.Errorf("begin agent admission sweep: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var leader bool
	if err := tx.QueryRow(ctx, `SELECT pg_try_advisory_xact_lock($1)`, agentAdmissionSweepLock).Scan(&leader); err != nil {
		return 0, fmt.Errorf("elect agent admission sweeper: %w", err)
	}
	if !leader {
		return 0, nil
	}

	rows, err := tx.Query(ctx, `
		UPDATE agent_subjects a
		SET lifecycle=$1, suspended_reason=$2, revision=a.revision+1, updated_at=NOW()
		WHERE a.visibility=$4 AND a.owner_user_id IS NOT NULL AND a.lifecycle=$3
		  AND NOT EXISTS (
			SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
			WHERE u.user_id=a.owner_user_id AND u.org_id=a.org_id
			  AND u.status='active' AND x.status='active'
		  )
		RETURNING a.org_id, a.agent_subject_id`,
		AgentLifecycleSuspended, "owner_inactive", AgentLifecycleActive, AgentVisibilityOwner)
	if err != nil {
		return 0, fmt.Errorf("suspend agents of inactive owners: %w", err)
	}
	type suspendedAgent struct{ orgID, agentID string }
	var suspended []suspendedAgent
	for rows.Next() {
		var agent suspendedAgent
		if err := rows.Scan(&agent.orgID, &agent.agentID); err != nil {
			rows.Close()
			return 0, fmt.Errorf("scan suspended agent: %w", err)
		}
		suspended = append(suspended, agent)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return 0, fmt.Errorf("read suspended agents: %w", err)
	}

	for _, agent := range suspended {
		rooms, err := suspendAgentAdmissions(ctx, tx, agent.orgID, agent.agentID)
		if err != nil {
			return 0, err
		}
		for _, spaceRef := range rooms {
			if err := insertAdmissionEvent(ctx, tx, spaceRef, agent.agentID, AdmissionEventSuspended, agentSweepActor, ""); err != nil {
				return 0, err
			}
			deactivated, err := deactivateAgentMemberships(ctx, tx, spaceRef, []string{agent.agentID}, "owner_inactive")
			if err != nil {
				return 0, err
			}
			if len(deactivated) > 0 {
				if err := bumpSpaceMembershipRevision(ctx, tx, spaceRef); err != nil {
					return 0, err
				}
			}
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, fmt.Errorf("commit agent admission sweep: %w", err)
	}
	return len(suspended), nil
}

// suspendAgentAdmissions suspends every admitted room row of one agent and returns the
// rooms it touched. The caller records the ledger rows and the membership changes.
func suspendAgentAdmissions(ctx context.Context, tx pgx.Tx, orgID, agentID string) ([]string, error) {
	rows, err := tx.Query(ctx, `
		UPDATE space_agent_admissions
		SET state=$3, state_reason=$4, revision=revision+1, updated_at=NOW()
		WHERE org_id=$1 AND agent_subject_id=$2 AND state=$5
		RETURNING space_ref`,
		orgID, agentID, AdmissionStateSuspended, "owner_inactive", AdmissionStateAdmitted)
	if err != nil {
		return nil, fmt.Errorf("suspend agent admissions: %w", err)
	}
	return scanStrings(rows)
}

// RunAgentAdmissionSweeper runs SweepInactiveOwnerAgents every AgentAdmissionSweepInterval
// until ctx ends. A failed pass is logged without ids or content and retried on the
// next tick.
func (r *Repository) RunAgentAdmissionSweeper(ctx context.Context) {
	ticker := time.NewTicker(AgentAdmissionSweepInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			suspended, err := r.SweepInactiveOwnerAgents(ctx)
			if err != nil {
				log.Printf("space agent admission sweep failed: %v", err)
				continue
			}
			if suspended > 0 {
				log.Printf("space agent admission sweep suspended %d agent(s) of inactive owners", suspended)
			}
		}
	}
}
