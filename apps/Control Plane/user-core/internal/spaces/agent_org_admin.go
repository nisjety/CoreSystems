package spaces

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// W3-DEC-ORGAGENT (owner decision D19): org owners and admins manage the org-visible agents.
// Since W3-DEC-CTL1 these are ownerless, so no owner can suspend or retire them. An org admin
// lists them, suspends one, reactivates one and retires one. Retire is a soft delete: the row
// stays for audit. Each change is one transaction: the definition, its admissions in every
// room and the seats those admissions hold move together, the admission ledger records each
// room change, and one user_audit_outbox row records the change. Ids, states and revisions only.
//
// The HTTP layer verifies the gateway delegation, the agents:manage scope and the canonical
// owner or admin role before any of this runs. Nothing here trusts a body field for the
// organization or the actor. This is the organization-level path. It does not duplicate the
// per-room admission rules of AdmitDenial.

// Org-admin lifecycle actions on an org-visible agent definition.
const (
	OrgAgentActionSuspend    = "suspend"
	OrgAgentActionReactivate = "reactivate"
	OrgAgentActionRetire     = "retire"
)

// CodeAgentLifecycleInvalid refuses a lifecycle change the agent's current state does not
// allow. A retired agent is terminal, so it cannot be suspended or reactivated.
const CodeAgentLifecycleInvalid = "agent_lifecycle_invalid"

// Reasons recorded on the definition (suspended_reason) and on the room rows a change touches.
const (
	orgAgentSuspendedReason = "org_admin_suspended"
	orgAgentRetiredReason   = "agent_retired"
)

// Audit subjects of the three org-admin actions, inside user-core's authority prefix
// (nats.conf publishes verevon.audit.v2.control.user-core.>).
const (
	AuditSubjectOrgAgentSuspended   = "verevon.audit.v2.control.user-core.org_agent_suspended"
	AuditSubjectOrgAgentReactivated = "verevon.audit.v2.control.user-core.org_agent_reactivated"
	AuditSubjectOrgAgentRetired     = "verevon.audit.v2.control.user-core.org_agent_retired"
)

// orgAgentAudit names the audit event and the subject of each org-admin action.
var orgAgentAudit = map[string]struct{ event, subject string }{
	OrgAgentActionSuspend:    {"org_agent_suspended", AuditSubjectOrgAgentSuspended},
	OrgAgentActionReactivate: {"org_agent_reactivated", AuditSubjectOrgAgentReactivated},
	OrgAgentActionRetire:     {"org_agent_retired", AuditSubjectOrgAgentRetired},
}

// OrgAgentView is one org-visible agent definition in the org-admin list. OwnerUserID is
// empty for an ownerless definition, and is set only on a legacy row that still records one.
type OrgAgentView struct {
	AgentSubjectID  string `json:"agent_subject_id"`
	OwnerUserID     string `json:"owner_user_id,omitempty"`
	Visibility      string `json:"visibility"`
	AddPolicy       string `json:"add_policy"`
	Lifecycle       string `json:"lifecycle"`
	SuspendedReason string `json:"suspended_reason,omitempty"`
	Revision        int64  `json:"revision"`
}

// ListOrgAgents returns the org-visible agent definitions of one organization, ordered by id,
// with their current lifecycle. Personal (owner-visible) agents are not listed here.
func (r *Repository) ListOrgAgents(ctx context.Context, orgID string) ([]OrgAgentView, error) {
	if r == nil || r.db == nil {
		return nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return []OrgAgentView{}, nil
	}
	rows, err := r.db.Pool.Query(ctx, `
		SELECT agent_subject_id, COALESCE(owner_user_id, ''), visibility, add_policy, lifecycle,
		       suspended_reason, revision
		FROM agent_subjects
		WHERE org_id=$1 AND visibility=$2
		ORDER BY agent_subject_id`, orgID, AgentVisibilityOrg)
	if err != nil {
		return nil, fmt.Errorf("list org agent subjects: %w", err)
	}
	defer rows.Close()
	views := []OrgAgentView{}
	for rows.Next() {
		var view OrgAgentView
		if err := rows.Scan(&view.AgentSubjectID, &view.OwnerUserID, &view.Visibility, &view.AddPolicy,
			&view.Lifecycle, &view.SuspendedReason, &view.Revision); err != nil {
			return nil, fmt.Errorf("scan org agent subject: %w", err)
		}
		views = append(views, view)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read org agent subjects: %w", err)
	}
	return views, nil
}

// OrgAgentLifecycleRequest is one org-admin lifecycle change. OrgID, ActorID and ActorOrgRole
// come from the verified delegation and the canonical role check, never from a body field.
type OrgAgentLifecycleRequest struct {
	OrgID          string
	ActorID        string
	ActorOrgRole   string
	AgentSubjectID string
	Action         string
	// ExpectedRevision is the revision the admin read. A stale one answers agent_state_changed.
	ExpectedRevision int64
}

// OrgAgentLifecycleResult is the agent after the change. Changed is false when the agent was
// already in the requested state, and then nothing was written. RoomsChanged counts the rooms
// whose admission moved.
type OrgAgentLifecycleResult struct {
	AgentSubjectID string
	Lifecycle      string
	Revision       int64
	Changed        bool
	RoomsChanged   int
}

// OrgAgentNextLifecycle returns the lifecycle an org-admin action moves an agent to, and
// whether the move changes anything. A retired agent is terminal: suspend and reactivate answer
// agent_lifecycle_invalid, and retire of a retired agent changes nothing.
func OrgAgentNextLifecycle(action, current string) (string, bool, error) {
	switch {
	case action == OrgAgentActionSuspend && current == AgentLifecycleActive:
		return AgentLifecycleSuspended, true, nil
	case action == OrgAgentActionSuspend && current == AgentLifecycleSuspended:
		return current, false, nil
	case action == OrgAgentActionReactivate && current == AgentLifecycleSuspended:
		return AgentLifecycleActive, true, nil
	case action == OrgAgentActionReactivate && current == AgentLifecycleActive:
		return current, false, nil
	case action == OrgAgentActionRetire && (current == AgentLifecycleActive || current == AgentLifecycleSuspended):
		return AgentLifecycleRetired, true, nil
	case action == OrgAgentActionRetire && current == AgentLifecycleRetired:
		return current, false, nil
	}
	return "", false, AgentError(409, CodeAgentLifecycleInvalid,
		"the agent cannot change to this state from its current lifecycle",
		map[string]any{"lifecycle": current, "action": action})
}

// ChangeOrgAgentLifecycle suspends, reactivates or retires one org-visible agent at the revision
// the admin read. One transaction locks the definition, applies the lifecycle, settles the
// agent's admissions and seats in every room, writes the admission ledger and the audit outbox
// row. Suspend suspends the admitted rooms. Reactivate restores the definition only: a room
// manager admits the agent again under AdmitDenial, so no seat returns on its own (no
// auto-resume). Retire withdraws every room row. The audit row is part of the change, so a
// change without an audit outbox is refused.
func (r *Repository) ChangeOrgAgentLifecycle(ctx context.Context, req OrgAgentLifecycleRequest) (*OrgAgentLifecycleResult, error) {
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ActorID = strings.TrimSpace(req.ActorID)
	req.AgentSubjectID = strings.TrimSpace(req.AgentSubjectID)
	req.Action = strings.TrimSpace(req.Action)
	if req.OrgID == "" || req.ActorID == "" {
		return nil, AgentError(403, CodeAgentDelegationRequired, "a signed gateway delegation is required", nil)
	}
	if _, known := orgAgentAudit[req.Action]; !known {
		return nil, AgentError(422, CodeAgentPolicyInvalid, "action must be suspend, reactivate or retire",
			map[string]any{"field": "action"})
	}
	if !ValidAgentSubjectID(req.AgentSubjectID) {
		return nil, agentNotFound()
	}
	if r == nil || r.db == nil || r.policyAudit == nil {
		return nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin org agent lifecycle change: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var lifecycle, visibility string
	var revision int64
	err = tx.QueryRow(ctx, `
		SELECT lifecycle, visibility, revision FROM agent_subjects
		WHERE org_id=$1 AND agent_subject_id=$2 FOR UPDATE`,
		req.OrgID, req.AgentSubjectID).Scan(&lifecycle, &visibility, &revision)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, agentNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("lock org agent subject: %w", err)
	}
	// Only organization-visible definitions are managed here. A personal agent belongs to its
	// owner, so for this route it answers like an unknown id.
	if visibility != AgentVisibilityOrg {
		return nil, agentNotFound()
	}
	if req.ExpectedRevision != revision {
		return nil, AgentError(409, CodeAgentStateChanged, "the agent changed since it was read",
			map[string]any{"current_revision": revision})
	}
	next, changed, err := OrgAgentNextLifecycle(req.Action, lifecycle)
	if err != nil {
		return nil, err
	}
	if !changed {
		return &OrgAgentLifecycleResult{AgentSubjectID: req.AgentSubjectID, Lifecycle: lifecycle, Revision: revision}, nil
	}
	if _, err := tx.Exec(ctx, `
		UPDATE agent_subjects SET lifecycle=$3, suspended_reason=$4, revision=revision+1, updated_at=NOW()
		WHERE org_id=$1 AND agent_subject_id=$2`,
		req.OrgID, req.AgentSubjectID, next, orgAgentReason(req.Action)); err != nil {
		return nil, fmt.Errorf("update org agent lifecycle: %w", err)
	}
	roomsChanged, err := settleOrgAgentRooms(ctx, tx, req.OrgID, req.AgentSubjectID, req.Action, req.ActorID)
	if err != nil {
		return nil, err
	}
	newRevision := revision + 1
	audit := orgAgentAudit[req.Action]
	payload, err := buildOrgAgentAuditPayload(audit.event, orgAgentAuditFacts{
		OrgID:          req.OrgID,
		ActorID:        req.ActorID,
		ActorRole:      req.ActorOrgRole,
		AgentID:        req.AgentSubjectID,
		FromLifecycle:  lifecycle,
		ToLifecycle:    next,
		Reason:         orgAgentReason(req.Action),
		RevisionBefore: revision,
		RevisionAfter:  newRevision,
		RoomsChanged:   roomsChanged,
	}, time.Now())
	if err != nil {
		return nil, err
	}
	if err := r.policyAudit.EnqueueAuditTx(ctx, tx,
		fmt.Sprintf("org-agent:%s:%d", req.AgentSubjectID, newRevision), audit.subject, payload); err != nil {
		return nil, fmt.Errorf("enqueue org agent audit: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit org agent lifecycle change: %w", err)
	}
	return &OrgAgentLifecycleResult{
		AgentSubjectID: req.AgentSubjectID, Lifecycle: next, Revision: newRevision,
		Changed: true, RoomsChanged: roomsChanged,
	}, nil
}

// settleOrgAgentRooms moves the agent's admissions in every room with the lifecycle change,
// writes one admission ledger row per room that changed, and deactivates the seats those rooms
// hold. It returns the number of rooms whose admission moved. Reactivate moves no admission.
// The caller holds the agent's definition lock.
func settleOrgAgentRooms(ctx context.Context, tx pgx.Tx, orgID, agentID, action, actorID string) (int, error) {
	var rows pgx.Rows
	var err error
	var event string
	switch action {
	case OrgAgentActionSuspend:
		event = AdmissionEventSuspended
		rows, err = tx.Query(ctx, `
			UPDATE space_agent_admissions
			SET state=$3, state_reason=$4, revision=revision+1, updated_at=NOW()
			WHERE org_id=$1 AND agent_subject_id=$2 AND state=$5
			RETURNING space_ref`,
			orgID, agentID, AdmissionStateSuspended, orgAgentSuspendedReason, AdmissionStateAdmitted)
	case OrgAgentActionRetire:
		event = AdmissionEventWithdrawn
		rows, err = tx.Query(ctx, `
			UPDATE space_agent_admissions
			SET state=$3, state_reason=$4, revision=revision+1, updated_at=NOW()
			WHERE org_id=$1 AND agent_subject_id=$2 AND state<>$3
			RETURNING space_ref`,
			orgID, agentID, AdmissionStateWithdrawn, orgAgentRetiredReason)
	default:
		return 0, nil
	}
	if err != nil {
		return 0, fmt.Errorf("settle org agent admissions: %w", err)
	}
	spaceRefs, err := scanStrings(rows)
	if err != nil {
		return 0, err
	}
	for _, spaceRef := range spaceRefs {
		if err := insertAdmissionEvent(ctx, tx, spaceRef, agentID, event, actorID, ""); err != nil {
			return 0, err
		}
		deactivated, err := deactivateAgentMemberships(ctx, tx, spaceRef, []string{agentID}, orgAgentReason(action))
		if err != nil {
			return 0, err
		}
		if len(deactivated) > 0 {
			if err := bumpSpaceMembershipRevision(ctx, tx, spaceRef); err != nil {
				return 0, err
			}
		}
	}
	return len(spaceRefs), nil
}

// orgAgentReason is the reason an org-admin action records. Reactivate records none.
func orgAgentReason(action string) string {
	switch action {
	case OrgAgentActionSuspend:
		return orgAgentSuspendedReason
	case OrgAgentActionRetire:
		return orgAgentRetiredReason
	}
	return ""
}

// orgAgentAuditFacts is everything the audit payload of an org-admin change carries. Ids,
// states, reasons and counts only.
type orgAgentAuditFacts struct {
	OrgID          string
	ActorID        string
	ActorRole      string
	AgentID        string
	FromLifecycle  string
	ToLifecycle    string
	Reason         string
	RevisionBefore int64
	RevisionAfter  int64
	RoomsChanged   int
}

// buildOrgAgentAuditPayload renders the audit-core event (events.AuditEvent shape) of an
// org-admin change, in the same shape as the Space effect-policy audit. No names, no content.
func buildOrgAgentAuditPayload(event string, facts orgAgentAuditFacts, now time.Time) ([]byte, error) {
	payload := map[string]any{
		"event_id":    fmt.Sprintf("org-agent:%s:%d", facts.AgentID, facts.RevisionAfter),
		"occurred_at": now.UTC().Format(time.RFC3339Nano),
		"org_id":      facts.OrgID,
		"user_id":     facts.ActorID,
		"actor_role":  facts.ActorRole,
		"plane":       "control",
		"producer":    "user-core",
		"event":       event,
		"subject":     "org:" + facts.OrgID,
		"resource_id": facts.AgentID,
		"outcome":     "ok",
		"details": map[string]any{
			"from_lifecycle":  facts.FromLifecycle,
			"to_lifecycle":    facts.ToLifecycle,
			"reason":          facts.Reason,
			"revision_before": facts.RevisionBefore,
			"revision_after":  facts.RevisionAfter,
			"rooms_changed":   facts.RoomsChanged,
		},
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("encode org agent audit event: %w", err)
	}
	return encoded, nil
}
