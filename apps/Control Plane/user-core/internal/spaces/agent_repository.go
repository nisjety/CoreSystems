package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// WS17-S4: Control-side repository of the agent registry. Every write runs in one
// transaction, locks the rows it decides on, and reads its facts from Control's
// own tables. The acting user and the organization come from the verified
// delegation at the HTTP layer; nothing here trusts a body field for either.

const (
	// maxAgentAccessEntries bounds one access-list replacement.
	maxAgentAccessEntries = 200
	// maxAgentCandidates bounds one candidates answer.
	maxAgentCandidates = 200
)

// SetAgentAdmissionMode wires SPACE_AGENT_ADMISSION once at startup.
func (r *Repository) SetAgentAdmissionMode(mode AgentAdmissionMode) {
	r.agentAdmission = mode
}

// agentAdmissionChecks reports whether the roster consults admission (observe or
// enforce). The zero value is off, so an unwired repository changes nothing.
func (r *Repository) agentAdmissionChecks() bool {
	return r != nil && (r.agentAdmission == AgentAdmissionObserve || r.agentAdmission == AgentAdmissionEnforce)
}

// AgentView is one agent definition with the caller's role on it: owner, manager,
// collaborator, or member (an org-visible agent the caller does not manage).
type AgentView struct {
	AgentSubjectID string `json:"agent_subject_id"`
	OwnerUserID    string `json:"owner_user_id,omitempty"`
	Visibility     string `json:"visibility"`
	AddPolicy      string `json:"add_policy"`
	Lifecycle      string `json:"lifecycle"`
	Revision       int64  `json:"revision"`
	Role           string `json:"role"`
}

// AgentRegisterRequest is POST /api/v1/internal/agents/subjects. The owner is the
// verified actor, never a body field.
type AgentRegisterRequest struct {
	OrgID          string
	ActorID        string
	AgentSubjectID string
	Visibility     string
	AddPolicy      string
}

// RegisterAgentSubject records an agent definition owned by the actor and returns
// created=true. The same owner registering the same id again replays the stored
// definition with created=false. Another owner, or another organization, gets
// agent_subject_conflict, which does not say who owns it.
func (r *Repository) RegisterAgentSubject(ctx context.Context, req AgentRegisterRequest) (*AgentView, bool, error) {
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ActorID = strings.TrimSpace(req.ActorID)
	req.AgentSubjectID = strings.TrimSpace(req.AgentSubjectID)
	req.Visibility = strings.TrimSpace(req.Visibility)
	req.AddPolicy = strings.TrimSpace(req.AddPolicy)
	if req.OrgID == "" || req.ActorID == "" {
		return nil, false, AgentError(403, CodeAgentDelegationRequired, "a signed gateway delegation is required", nil)
	}
	if !ValidAgentSubjectID(req.AgentSubjectID) {
		return nil, false, AgentError(422, CodeAgentPolicyInvalid, "agent_subject_id must be an agent id",
			map[string]any{"field": "agent_subject_id"})
	}
	if req.AddPolicy == "" {
		req.AddPolicy = AgentAddPolicyAnyone
		if req.Visibility == AgentVisibilityOwner {
			req.AddPolicy = AgentAddPolicyOwnerOnly
		}
	}
	if err := ValidateAgentPolicy(req.Visibility, req.AddPolicy); err != nil {
		return nil, false, err
	}
	if r == nil || r.db == nil {
		return nil, false, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, false, fmt.Errorf("begin agent registration: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	// An org-visible definition is owned by the organization, not by the actor (D10), so it is
	// registered without an owner. Only an active organization member may register or repeat one.
	var orgAccess string
	if req.Visibility == AgentVisibilityOrg {
		orgAccess, err = orgMemberAccess(ctx, tx, req.OrgID, req.AgentSubjectID, req.ActorID)
		if err != nil {
			return nil, false, err
		}
	}

	// The id is unique across organizations, so the lookup is by id alone.
	var existingOrg string
	var existingOwner *string
	var existing AgentView
	err = tx.QueryRow(ctx, `
		SELECT org_id, owner_user_id, visibility, add_policy, lifecycle, revision
		FROM agent_subjects WHERE agent_subject_id=$1 FOR UPDATE`, req.AgentSubjectID).Scan(
		&existingOrg, &existingOwner, &existing.Visibility, &existing.AddPolicy, &existing.Lifecycle, &existing.Revision)
	if err == nil {
		if existingOrg != req.OrgID {
			return nil, false, AgentError(409, CodeAgentSubjectConflict, "this agent id is already registered", nil)
		}
		if existingOwner == nil {
			// An ownerless (org-owned) definition repeats for any active member of its organization. An
			// owner-visible definition always has an owner, so only an org-visible repeat reaches this branch.
			if existing.Visibility != AgentVisibilityOrg || req.Visibility != AgentVisibilityOrg {
				return nil, false, AgentError(409, CodeAgentSubjectConflict, "this agent id is already registered", nil)
			}
			existing.AgentSubjectID = req.AgentSubjectID
			existing.Role = agentRoleOf("", req.ActorID, orgAccess)
			return &existing, false, nil
		}
		if *existingOwner != req.ActorID {
			return nil, false, AgentError(409, CodeAgentSubjectConflict, "this agent id is already registered", nil)
		}
		existing.AgentSubjectID = req.AgentSubjectID
		existing.OwnerUserID = req.ActorID
		existing.Role = "owner"
		return &existing, false, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return nil, false, fmt.Errorf("read agent subject for registration: %w", err)
	}
	// An org-visible definition is ownerless (D10): owner_user_id stays NULL and registered_by keeps
	// the registrant for the record. An owner-visible definition is owned by the actor, as before.
	var owner *string
	view := AgentView{
		AgentSubjectID: req.AgentSubjectID, Visibility: req.Visibility,
		AddPolicy: req.AddPolicy, Lifecycle: AgentLifecycleActive, Revision: 1,
	}
	if req.Visibility == AgentVisibilityOrg {
		view.Role = agentRoleOf("", req.ActorID, orgAccess)
	} else {
		owner = &req.ActorID
		view.OwnerUserID = req.ActorID
		view.Role = "owner"
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO agent_subjects
			(org_id, agent_subject_id, owner_user_id, visibility, add_policy, lifecycle, revision, registered_by)
		VALUES ($1, $2, $3, $4, $5, 'active', 1, $6)`,
		req.OrgID, req.AgentSubjectID, owner, req.Visibility, req.AddPolicy, req.ActorID); err != nil {
		return nil, false, fmt.Errorf("insert agent subject: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, false, fmt.Errorf("commit agent registration: %w", err)
	}
	return &view, true, nil
}

// VisibleAgents lists the agent definitions the actor may see in the organization:
// owned, listed on the access list, or org-visible, each with the actor's role. An
// actor who is not an active organization member sees none.
func (r *Repository) VisibleAgents(ctx context.Context, orgID, actorID string) ([]AgentView, error) {
	return r.visibleAgents(ctx, orgID, actorID, "")
}

// VisibleAgent returns one visible agent definition, or (nil, nil) when the actor may
// not see it. The caller answers that the same way as an unknown id.
func (r *Repository) VisibleAgent(ctx context.Context, orgID, actorID, agentSubjectID string) (*AgentView, error) {
	views, err := r.visibleAgents(ctx, orgID, actorID, strings.TrimSpace(agentSubjectID))
	if err != nil || len(views) == 0 {
		return nil, err
	}
	return &views[0], nil
}

// VisibleAgentAccess returns one visible agent definition together with its access list,
// read in one transaction so the list matches the revision. It answers (nil, nil, nil)
// when the actor may not see the definition, like VisibleAgent. Control is the authority
// for the list; the Convex copy is only a display projection (DEPTH-WS17-S5-ACCESS-READ).
func (r *Repository) VisibleAgentAccess(ctx context.Context, orgID, actorID, agentSubjectID string) (*AgentView, []AgentAccessEntry, error) {
	if r == nil || r.db == nil {
		return nil, nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	tx, err := r.db.Pool.BeginTx(ctx, pgx.TxOptions{IsoLevel: pgx.RepeatableRead, AccessMode: pgx.ReadOnly})
	if err != nil {
		return nil, nil, fmt.Errorf("begin agent access read: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	views, err := visibleAgentsIn(ctx, tx, orgID, actorID, strings.TrimSpace(agentSubjectID))
	if err != nil || len(views) == 0 {
		return nil, nil, err
	}
	entries, err := agentEntries(ctx, tx, strings.TrimSpace(orgID), views[0].AgentSubjectID)
	if err != nil {
		return nil, nil, err
	}
	return &views[0], entries, nil
}

func (r *Repository) visibleAgents(ctx context.Context, orgID, actorID, onlyAgentID string) ([]AgentView, error) {
	if r == nil || r.db == nil {
		return nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	return visibleAgentsIn(ctx, r.db.Pool, orgID, actorID, onlyAgentID)
}

// agentQuerier is what both the pool and a transaction offer for a read.
type agentQuerier interface {
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
}

func visibleAgentsIn(ctx context.Context, q agentQuerier, orgID, actorID, onlyAgentID string) ([]AgentView, error) {
	orgID = strings.TrimSpace(orgID)
	actorID = strings.TrimSpace(actorID)
	if orgID == "" || actorID == "" {
		return []AgentView{}, nil
	}
	rows, err := q.Query(ctx, `
		SELECT a.agent_subject_id, COALESCE(a.owner_user_id, ''), a.visibility, a.add_policy, a.lifecycle, a.revision,
		       COALESCE((SELECT g.role FROM agent_subject_access g
		                  WHERE g.org_id=a.org_id AND g.agent_subject_id=a.agent_subject_id
		                    AND g.principal_type='user' AND g.principal_id=$2), '') AS access
		FROM agent_subjects a
		WHERE a.org_id=$1
		  AND ($3 = '' OR a.agent_subject_id = $3)
		  AND EXISTS (SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
		               WHERE u.user_id=$2 AND u.org_id=$1 AND u.status='active' AND x.status='active')
		  AND (a.owner_user_id=$2 OR a.visibility='org'
		       OR EXISTS (SELECT 1 FROM agent_subject_access g
		                   WHERE g.org_id=a.org_id AND g.agent_subject_id=a.agent_subject_id
		                     AND g.principal_type='user' AND g.principal_id=$2))
		ORDER BY a.agent_subject_id`, orgID, actorID, onlyAgentID)
	if err != nil {
		return nil, fmt.Errorf("list visible agent subjects: %w", err)
	}
	defer rows.Close()
	views := []AgentView{}
	for rows.Next() {
		var view AgentView
		var access string
		if err := rows.Scan(&view.AgentSubjectID, &view.OwnerUserID, &view.Visibility, &view.AddPolicy,
			&view.Lifecycle, &view.Revision, &access); err != nil {
			return nil, fmt.Errorf("scan visible agent subject: %w", err)
		}
		view.Role = agentRoleOf(view.OwnerUserID, actorID, access)
		views = append(views, view)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read visible agent subjects: %w", err)
	}
	return views, nil
}

// AgentAccessEntry is one access-list entry: a user and a manager or collaborator role.
type AgentAccessEntry struct {
	PrincipalID string `json:"principal_id"`
	Role        string `json:"role"`
}

// AgentAccessRequest is PUT /api/v1/internal/agents/subjects/{agent_subject_id}/access.
type AgentAccessRequest struct {
	OrgID            string
	ActorID          string
	AgentSubjectID   string
	Visibility       string
	AddPolicy        string
	Entries          []AgentAccessEntry
	ExpectedRevision int64
	// ResolveOrgRole (D19/D22as) returns the actor's canonical organization role from
	// the membership authority. It is called only when the agent is ownerless and
	// org-visible and the actor is neither its owner nor a manager on its list, so the
	// owner and manager paths never depend on it. nil means no org-admin path.
	ResolveOrgRole func(ctx context.Context) (string, error)
}

// Who an access edit was authorized as (D19/D22as). agentAccessAuthorityOrgAdmin is a
// canonical org owner or admin editing an ownerless org-visible agent.
const (
	agentAccessAuthorityOwner    = "owner"
	agentAccessAuthorityManager  = "manager"
	agentAccessAuthorityOrgAdmin = "org_admin"
)

// AuditSubjectOrgAgentAccessChanged is the audit subject of an access edit made through
// org-admin authority, or of an owner giving up ownership by flipping to org visibility.
const AuditSubjectOrgAgentAccessChanged = "verevon.audit.v2.control.user-core.org_agent_access_changed"

// AgentAccessEditAuthority decides who may edit an agent's access. The owner and a
// manager on the access list always may. A canonical org owner or admin may edit an
// ownerless org-visible agent (D19/D22as): nobody else can, so without this path such an
// agent could not be edited at all. It returns "" when the actor may not edit.
func AgentAccessEditAuthority(isOwner bool, access string, ownerless bool, actorOrgRole string) string {
	switch {
	case isOwner:
		return agentAccessAuthorityOwner
	case access == AgentAccessManager:
		return agentAccessAuthorityManager
	case ownerless && (actorOrgRole == "owner" || actorOrgRole == "admin"):
		return agentAccessAuthorityOrgAdmin
	}
	return ""
}

// AgentAccessClearsOwner reports whether an access edit gives up the agent's owner
// (D22as): an owner-visible agent with an owner that becomes org-visible becomes an
// organization agent, and an organization agent has no owner (D10).
func AgentAccessClearsOwner(hasOwner bool, fromVisibility, toVisibility string) bool {
	return hasOwner && fromVisibility == AgentVisibilityOwner && toVisibility == AgentVisibilityOrg
}

// UpdateAgentAccess replaces an agent's visibility, add policy and access list. Its
// owner, or a manager on its access list, may edit, and only at the revision the
// editor read. A canonical org owner or admin may also edit an ownerless org-visible
// agent (D19/D22as). Every listed principal must be an active organization member. The
// revision moves only when something changed. The owner never changes here, except that
// an owner flipping their agent from owner to org visibility clears the owner (D22as).
// An org-admin edit and an owner clear each write one org_agent_access_changed audit row
// in the same transaction.
func (r *Repository) UpdateAgentAccess(ctx context.Context, req AgentAccessRequest) (*AgentView, error) {
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ActorID = strings.TrimSpace(req.ActorID)
	req.AgentSubjectID = strings.TrimSpace(req.AgentSubjectID)
	req.Visibility = strings.TrimSpace(req.Visibility)
	req.AddPolicy = strings.TrimSpace(req.AddPolicy)
	if req.OrgID == "" || req.ActorID == "" {
		return nil, AgentError(403, CodeAgentDelegationRequired, "a signed gateway delegation is required", nil)
	}
	if !ValidAgentSubjectID(req.AgentSubjectID) {
		return nil, agentNotFound()
	}
	if err := ValidateAgentPolicy(req.Visibility, req.AddPolicy); err != nil {
		return nil, err
	}
	if len(req.Entries) > maxAgentAccessEntries {
		return nil, AgentError(422, CodeAgentPolicyInvalid, "too many access entries", map[string]any{"field": "entries"})
	}
	entries := make([]AgentAccessEntry, 0, len(req.Entries))
	seen := make(map[string]struct{}, len(req.Entries))
	for _, entry := range req.Entries {
		principal := strings.TrimSpace(entry.PrincipalID)
		if principal == "" || !ValidAgentAccessRole(entry.Role) {
			return nil, AgentError(422, CodeAgentPolicyInvalid,
				"each entry needs a principal and a manager or collaborator role", map[string]any{"field": "entries"})
		}
		if _, dup := seen[principal]; dup {
			return nil, AgentError(422, CodeAgentPolicyInvalid, "a principal may appear once", map[string]any{"field": "entries"})
		}
		seen[principal] = struct{}{}
		entries = append(entries, AgentAccessEntry{PrincipalID: principal, Role: entry.Role})
	}
	if r == nil || r.db == nil {
		return nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin agent access update: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var owner *string
	var visibility, addPolicy, lifecycle string
	var revision int64
	err = tx.QueryRow(ctx, `
		SELECT owner_user_id, visibility, add_policy, lifecycle, revision
		FROM agent_subjects WHERE org_id=$1 AND agent_subject_id=$2 FOR UPDATE`,
		req.OrgID, req.AgentSubjectID).Scan(&owner, &visibility, &addPolicy, &lifecycle, &revision)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, agentNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("lock agent subject for access update: %w", err)
	}
	ownerID := ""
	if owner != nil {
		ownerID = *owner
	}
	access, active, err := agentActorAccess(ctx, tx, req.OrgID, req.AgentSubjectID, req.ActorID)
	if err != nil {
		return nil, err
	}
	isOwner := ownerID != "" && ownerID == req.ActorID
	if !active || (!isOwner && access == "" && visibility != AgentVisibilityOrg) {
		return nil, agentNotFound()
	}
	ownerless := owner == nil && visibility == AgentVisibilityOrg
	actorOrgRole := ""
	if !CanEditAgentAccess(isOwner, access) && ownerless && req.ResolveOrgRole != nil {
		role, roleErr := req.ResolveOrgRole(ctx)
		if roleErr != nil {
			return nil, AgentError(503, CodeAgentRegistryUnavailable, "the organization role could not be verified", nil)
		}
		actorOrgRole = strings.ToLower(strings.TrimSpace(role))
	}
	authority := AgentAccessEditAuthority(isOwner, access, ownerless, actorOrgRole)
	if authority == "" {
		return nil, AgentError(403, CodeAgentAccessForbidden, "only the owner, a manager or an organization admin may change this agent", nil)
	}
	if owner == nil && req.Visibility == AgentVisibilityOwner {
		// D10: an organization agent has no owner, so it cannot become owner-visible. The owner CHECK
		// on agent_subjects would refuse the write, so the refusal is typed here instead.
		return nil, AgentError(422, CodeAgentPolicyInvalid, "an organization agent has no owner, so it cannot become owner-visible",
			map[string]any{"field": "visibility"})
	}
	if req.ExpectedRevision != revision {
		return nil, AgentError(409, CodeAgentStateChanged, "the agent changed since it was read",
			map[string]any{"current_revision": revision})
	}
	// D24 (a gap a D22as builder found): an owner flipping their agent to org visibility
	// (AgentAccessClearsOwner below) must not lock themselves out just because the access
	// list they submitted does not name them. The access list, not ownership, now governs
	// their edit rights (D10), so keep them as a manager unless they are already listed.
	if AgentAccessClearsOwner(owner != nil, visibility, req.Visibility) && len(entries) < maxAgentAccessEntries {
		if _, listed := seen[req.ActorID]; !listed {
			entries = append(entries, AgentAccessEntry{PrincipalID: req.ActorID, Role: AgentAccessManager})
			seen[req.ActorID] = struct{}{}
		}
	}
	if len(entries) > 0 {
		principals := make([]string, 0, len(entries))
		for _, entry := range entries {
			principals = append(principals, entry.PrincipalID)
		}
		var found int
		if err := tx.QueryRow(ctx, `
			SELECT count(*) FROM user_org_memberships u JOIN users x ON x.id=u.user_id
			WHERE u.org_id=$1 AND u.status='active' AND x.status='active' AND u.user_id = ANY($2::text[])`,
			req.OrgID, principals).Scan(&found); err != nil {
			return nil, fmt.Errorf("check agent access principals: %w", err)
		}
		if found != len(principals) {
			return nil, AgentError(422, CodePrincipalNotInOrg,
				"every listed principal must be an active organization member", nil)
		}
	}
	current, err := agentEntries(ctx, tx, req.OrgID, req.AgentSubjectID)
	if err != nil {
		return nil, err
	}
	entriesChanged := !sameAgentEntries(current, entries)
	clearOwner := AgentAccessClearsOwner(owner != nil, visibility, req.Visibility)
	changed := entriesChanged || visibility != req.Visibility || addPolicy != req.AddPolicy
	audited := changed && (authority == agentAccessAuthorityOrgAdmin || clearOwner)
	if audited && r.policyAudit == nil {
		// The audit row is part of the change, as for the org-admin lifecycle actions.
		return nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	revisionBefore := revision
	if changed {
		if _, err := tx.Exec(ctx, `
			UPDATE agent_subjects SET visibility=$3, add_policy=$4,
			       owner_user_id=CASE WHEN $5 THEN NULL ELSE owner_user_id END,
			       revision=revision+1, updated_at=NOW()
			WHERE org_id=$1 AND agent_subject_id=$2`,
			req.OrgID, req.AgentSubjectID, req.Visibility, req.AddPolicy, clearOwner); err != nil {
			return nil, fmt.Errorf("update agent subject: %w", err)
		}
		revision++
	}
	if entriesChanged {
		if _, err := tx.Exec(ctx, `
			DELETE FROM agent_subject_access WHERE org_id=$1 AND agent_subject_id=$2`,
			req.OrgID, req.AgentSubjectID); err != nil {
			return nil, fmt.Errorf("clear agent access list: %w", err)
		}
		for _, entry := range entries {
			if _, err := tx.Exec(ctx, `
				INSERT INTO agent_subject_access (org_id, agent_subject_id, principal_type, principal_id, role, granted_by)
				VALUES ($1, $2, 'user', $3, $4, $5)`,
				req.OrgID, req.AgentSubjectID, entry.PrincipalID, entry.Role, req.ActorID); err != nil {
				return nil, fmt.Errorf("write agent access entry: %w", err)
			}
		}
	}
	if audited {
		payload, err := buildOrgAgentAccessAuditPayload(orgAgentAccessAuditFacts{
			OrgID: req.OrgID, ActorID: req.ActorID, ActorOrgRole: actorOrgRole, Authority: authority,
			AgentID: req.AgentSubjectID, FromVisibility: visibility, ToVisibility: req.Visibility,
			FromAddPolicy: addPolicy, ToAddPolicy: req.AddPolicy, EntriesChanged: entriesChanged,
			EntryCount: len(entries), OwnerCleared: clearOwner,
			RevisionBefore: revisionBefore, RevisionAfter: revision,
		}, time.Now())
		if err != nil {
			return nil, err
		}
		if err := r.policyAudit.EnqueueAuditTx(ctx, tx,
			fmt.Sprintf("org-agent-access:%s:%d", req.AgentSubjectID, revision), AuditSubjectOrgAgentAccessChanged, payload); err != nil {
			return nil, fmt.Errorf("enqueue org agent access audit: %w", err)
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit agent access update: %w", err)
	}
	if clearOwner {
		ownerID = ""
	}
	return &AgentView{
		AgentSubjectID: req.AgentSubjectID, OwnerUserID: ownerID, Visibility: req.Visibility,
		AddPolicy: req.AddPolicy, Lifecycle: lifecycle, Revision: revision,
		Role: agentRoleOf(ownerID, req.ActorID, access),
	}, nil
}

// AgentCandidate is the admission answer for one agent id: whether the caller can see
// it, whether the caller may add it to the room now, and the first reason it may not.
// An agent the caller cannot see answers exactly like an unknown id.
type AgentCandidate struct {
	AgentSubjectID string `json:"agent_subject_id"`
	Visible        bool   `json:"visible"`
	CanAdd         bool   `json:"can_add"`
	Reason         string `json:"reason"`
}

// AgentCandidates answers, for each id, whether the caller may add that agent to the
// room. The room must be in the caller's organization and the caller an active member
// of it. Duplicate ids are answered once.
func (r *Repository) AgentCandidates(ctx context.Context, orgID, actorID, spaceRef string, agentIDs []string) ([]AgentCandidate, error) {
	orgID = strings.TrimSpace(orgID)
	actorID = strings.TrimSpace(actorID)
	spaceRef = strings.TrimSpace(spaceRef)
	if orgID == "" || actorID == "" {
		return nil, AgentError(403, CodeAgentDelegationRequired, "a signed gateway delegation is required", nil)
	}
	if len(agentIDs) > maxAgentCandidates {
		return nil, AgentError(422, CodeAgentPolicyInvalid, "too many agent ids", map[string]any{"field": "agent_subject_ids"})
	}
	if r == nil || r.db == nil {
		return nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin agent candidates read: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	space, err := roomForAgents(ctx, tx, orgID, spaceRef)
	if err != nil {
		return nil, err
	}
	actorRole, member, err := roomActorRole(ctx, tx, space.SpaceRef, orgID, actorID)
	if err != nil {
		return nil, err
	}
	if !member {
		return nil, roomNotFound()
	}
	orgPersonalAdmit, err := orgPersonalAgentAdmit(ctx, tx, orgID)
	if err != nil {
		return nil, err
	}
	out := make([]AgentCandidate, 0, len(agentIDs))
	seen := make(map[string]struct{}, len(agentIDs))
	for _, raw := range agentIDs {
		id := strings.TrimSpace(raw)
		if _, dup := seen[id]; dup {
			continue
		}
		seen[id] = struct{}{}
		candidate := AgentCandidate{AgentSubjectID: id, Reason: AdmitReasonAgentNotVisible}
		if ValidAgentSubjectID(id) {
			facts, visible, err := agentAdmitFacts(ctx, tx, space, actorRole, actorID, id, orgPersonalAdmit)
			if err != nil {
				return nil, err
			}
			if visible {
				candidate.Visible = true
				candidate.Reason = AdmitDenial(facts)
				candidate.CanAdd = candidate.Reason == ""
			}
		}
		out = append(out, candidate)
	}
	return out, nil
}

// AgentAdmitRequest is POST /api/v1/internal/spaces/{space_ref}/agent-admissions.
type AgentAdmitRequest struct {
	OrgID          string
	ActorID        string
	SpaceRef       string
	AgentSubjectID string
	IdempotencyKey string
}

// AgentAdmitResult is one admission. Changed is false when the agent was already
// admitted. Replayed is true when the idempotency key matched an earlier admission of
// the same agent.
type AgentAdmitResult struct {
	SpaceRef           string
	AgentSubjectID     string
	State              string
	Changed            bool
	Replayed           bool
	MembershipRevision int64
}

// AdmitAgent admits an agent to a room. AdmitDenial decides on facts read in the same
// transaction as the write. A refusal answers 403 agent_not_addable with details.reason,
// or 404 agent_not_found when the caller cannot see the agent. The admission row and its
// ledger event commit together.
func (r *Repository) AdmitAgent(ctx context.Context, req AgentAdmitRequest) (*AgentAdmitResult, error) {
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ActorID = strings.TrimSpace(req.ActorID)
	req.SpaceRef = strings.TrimSpace(req.SpaceRef)
	req.AgentSubjectID = strings.TrimSpace(req.AgentSubjectID)
	req.IdempotencyKey = strings.TrimSpace(req.IdempotencyKey)
	if req.OrgID == "" || req.ActorID == "" {
		return nil, AgentError(403, CodeAgentDelegationRequired, "a signed gateway delegation is required", nil)
	}
	if len(req.IdempotencyKey) > 128 {
		return nil, invalidRequest("idempotency_key", "idempotency_key is too long")
	}
	if r == nil || r.db == nil {
		return nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	if !ValidAgentSubjectID(req.AgentSubjectID) {
		return nil, agentNotFound()
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin agent admission: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	space, err := roomForAgents(ctx, tx, req.OrgID, req.SpaceRef)
	if err != nil {
		return nil, err
	}
	actorRole, member, err := roomActorRole(ctx, tx, space.SpaceRef, req.OrgID, req.ActorID)
	if err != nil {
		return nil, err
	}
	if !member {
		return nil, roomNotFound()
	}
	orgPersonalAdmit, err := orgPersonalAgentAdmit(ctx, tx, req.OrgID)
	if err != nil {
		return nil, err
	}
	facts, visible, err := agentAdmitFacts(ctx, tx, space, actorRole, req.ActorID, req.AgentSubjectID, orgPersonalAdmit)
	if err != nil {
		return nil, err
	}
	if !visible {
		return nil, agentNotFound()
	}
	if reason := AdmitDenial(facts); reason != "" {
		return nil, AgentError(403, CodeAgentNotAddable, "this agent cannot be added to this room",
			map[string]any{"reason": reason})
	}
	if req.IdempotencyKey != "" {
		replayed, err := priorAdmissionKey(ctx, tx, space.SpaceRef, req.AgentSubjectID, req.IdempotencyKey)
		if err != nil {
			return nil, err
		}
		if replayed {
			state, revision, err := admissionAndRevision(ctx, tx, space.SpaceRef, req.AgentSubjectID)
			if err != nil {
				return nil, err
			}
			return &AgentAdmitResult{
				SpaceRef: space.SpaceRef, AgentSubjectID: req.AgentSubjectID, State: state,
				Replayed: true, MembershipRevision: revision,
			}, nil
		}
	}

	var state string
	err = tx.QueryRow(ctx, `
		SELECT state FROM space_agent_admissions WHERE space_ref=$1 AND agent_subject_id=$2 FOR UPDATE`,
		space.SpaceRef, req.AgentSubjectID).Scan(&state)
	if err != nil && !errors.Is(err, pgx.ErrNoRows) {
		return nil, fmt.Errorf("lock agent admission: %w", err)
	}
	changed := errors.Is(err, pgx.ErrNoRows) || state != AdmissionStateAdmitted
	if changed {
		if _, err := tx.Exec(ctx, `
			INSERT INTO space_agent_admissions
				(space_ref, org_id, agent_subject_id, state, state_reason, admitted_by, revision)
			VALUES ($1, $2, $3, 'admitted', '', $4, 1)
			ON CONFLICT (space_ref, agent_subject_id) DO UPDATE
			SET state='admitted', state_reason='', admitted_by=EXCLUDED.admitted_by,
			    revision=space_agent_admissions.revision+1, updated_at=NOW()`,
			space.SpaceRef, req.OrgID, req.AgentSubjectID, req.ActorID); err != nil {
			return nil, fmt.Errorf("record agent admission: %w", err)
		}
		if err := insertAdmissionEvent(ctx, tx, space.SpaceRef, req.AgentSubjectID,
			AdmissionEventAdmitted, req.ActorID, req.IdempotencyKey); err != nil {
			return nil, err
		}
	}
	revision, err := membershipRevisionOf(ctx, tx, space.SpaceRef)
	if err != nil {
		return nil, err
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit agent admission: %w", err)
	}
	return &AgentAdmitResult{
		SpaceRef: space.SpaceRef, AgentSubjectID: req.AgentSubjectID, State: AdmissionStateAdmitted,
		Changed: changed, MembershipRevision: revision,
	}, nil
}

// AgentWithdrawRequest is DELETE /api/v1/internal/spaces/{space_ref}/agent-admissions/{agent_subject_id}.
type AgentWithdrawRequest struct {
	OrgID          string
	ActorID        string
	SpaceRef       string
	AgentSubjectID string
}

// AgentWithdrawResult is the admission state after a withdrawal.
type AgentWithdrawResult struct {
	SpaceRef           string
	AgentSubjectID     string
	State              string
	Changed            bool
	MembershipRevision int64
}

// WithdrawAgent withdraws an agent from a room. The room's owner or manager, the
// agent's owner, or a manager on its access list may do it. The admission and the
// agent's service membership in the room change in one transaction, so the roster and
// the admission agree at once.
func (r *Repository) WithdrawAgent(ctx context.Context, req AgentWithdrawRequest) (*AgentWithdrawResult, error) {
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ActorID = strings.TrimSpace(req.ActorID)
	req.SpaceRef = strings.TrimSpace(req.SpaceRef)
	req.AgentSubjectID = strings.TrimSpace(req.AgentSubjectID)
	if req.OrgID == "" || req.ActorID == "" {
		return nil, AgentError(403, CodeAgentDelegationRequired, "a signed gateway delegation is required", nil)
	}
	if r == nil || r.db == nil {
		return nil, AgentError(503, CodeAgentRegistryUnavailable, "agent registry is unavailable", nil)
	}
	if !ValidAgentSubjectID(req.AgentSubjectID) {
		return nil, agentNotFound()
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin agent withdrawal: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	space, err := roomForAgents(ctx, tx, req.OrgID, req.SpaceRef)
	if err != nil {
		return nil, err
	}
	actorRole, member, err := roomActorRole(ctx, tx, space.SpaceRef, req.OrgID, req.ActorID)
	if err != nil {
		return nil, err
	}
	if !member {
		return nil, roomNotFound()
	}
	access, active, err := agentActorAccess(ctx, tx, req.OrgID, req.AgentSubjectID, req.ActorID)
	if err != nil {
		return nil, err
	}
	var owner *string
	err = tx.QueryRow(ctx, `
		SELECT owner_user_id FROM agent_subjects WHERE org_id=$1 AND agent_subject_id=$2`,
		req.OrgID, req.AgentSubjectID).Scan(&owner)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, agentNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("read agent owner for withdrawal: %w", err)
	}
	isOwner := owner != nil && *owner == req.ActorID
	if !active || !CanWithdrawAgent(actorRole, isOwner, access) {
		return nil, AgentError(403, CodeAgentWithdrawForbidden, "you cannot withdraw this agent from this room", nil)
	}
	var state string
	err = tx.QueryRow(ctx, `
		SELECT state FROM space_agent_admissions WHERE space_ref=$1 AND agent_subject_id=$2 FOR UPDATE`,
		space.SpaceRef, req.AgentSubjectID).Scan(&state)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, agentNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("lock agent admission for withdrawal: %w", err)
	}
	changed := state != AdmissionStateWithdrawn
	if changed {
		if _, err := tx.Exec(ctx, `
			UPDATE space_agent_admissions
			SET state=$3, state_reason='withdrawn', revision=revision+1, updated_at=NOW()
			WHERE space_ref=$1 AND agent_subject_id=$2`,
			space.SpaceRef, req.AgentSubjectID, AdmissionStateWithdrawn); err != nil {
			return nil, fmt.Errorf("withdraw agent admission: %w", err)
		}
		if err := insertAdmissionEvent(ctx, tx, space.SpaceRef, req.AgentSubjectID,
			AdmissionEventWithdrawn, req.ActorID, ""); err != nil {
			return nil, err
		}
		deactivated, err := deactivateAgentMemberships(ctx, tx, space.SpaceRef, []string{req.AgentSubjectID}, "agent_withdrawn")
		if err != nil {
			return nil, err
		}
		if len(deactivated) > 0 {
			if err := bumpSpaceMembershipRevision(ctx, tx, space.SpaceRef); err != nil {
				return nil, err
			}
		}
	}
	revision, err := membershipRevisionOf(ctx, tx, space.SpaceRef)
	if err != nil {
		return nil, err
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit agent withdrawal: %w", err)
	}
	return &AgentWithdrawResult{
		SpaceRef: space.SpaceRef, AgentSubjectID: req.AgentSubjectID, State: AdmissionStateWithdrawn,
		Changed: changed, MembershipRevision: revision,
	}, nil
}

// agentRoleOf names the caller's role on an agent definition.
func agentRoleOf(ownerID, actorID, access string) string {
	switch {
	case ownerID != "" && ownerID == actorID:
		return "owner"
	case access == AgentAccessManager, access == AgentAccessCollaborator:
		return access
	}
	return "member"
}

func agentNotFound() *RoomError {
	return AgentError(404, CodeAgentNotFound, "agent not found", nil)
}

// agentActorAccess reads the caller's access-list role on one agent ("" when none) and
// whether the caller is an active organization member.
func agentActorAccess(ctx context.Context, tx pgx.Tx, orgID, agentID, actorID string) (string, bool, error) {
	var access string
	var active bool
	err := tx.QueryRow(ctx, `
		SELECT COALESCE((SELECT g.role FROM agent_subject_access g
		                  WHERE g.org_id=$1 AND g.agent_subject_id=$2
		                    AND g.principal_type='user' AND g.principal_id=$3), ''),
		       EXISTS (SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
		                WHERE u.user_id=$3 AND u.org_id=$1 AND u.status='active' AND x.status='active')`,
		orgID, agentID, actorID).Scan(&access, &active)
	if err != nil {
		return "", false, fmt.Errorf("read agent access for actor: %w", err)
	}
	return access, active, nil
}

// orgMemberAccess reads the caller's access-list role on an agent and refuses a caller who is not an active
// organization member. Only an org-visible (ownerless, D10) definition is registered or repeated through it.
func orgMemberAccess(ctx context.Context, tx pgx.Tx, orgID, agentID, actorID string) (string, error) {
	access, active, err := agentActorAccess(ctx, tx, orgID, agentID, actorID)
	if err != nil {
		return "", err
	}
	if !active {
		return "", AgentError(403, CodeAgentAccessForbidden, "only an active organization member may register an organization agent", nil)
	}
	return access, nil
}

// agentEntries reads an agent's access list, ordered by principal.
func agentEntries(ctx context.Context, tx pgx.Tx, orgID, agentID string) ([]AgentAccessEntry, error) {
	rows, err := tx.Query(ctx, `
		SELECT principal_id, role FROM agent_subject_access
		WHERE org_id=$1 AND agent_subject_id=$2 AND principal_type='user'
		ORDER BY principal_id`, orgID, agentID)
	if err != nil {
		return nil, fmt.Errorf("read agent access list: %w", err)
	}
	defer rows.Close()
	out := []AgentAccessEntry{}
	for rows.Next() {
		var entry AgentAccessEntry
		if err := rows.Scan(&entry.PrincipalID, &entry.Role); err != nil {
			return nil, fmt.Errorf("scan agent access entry: %w", err)
		}
		out = append(out, entry)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read agent access list: %w", err)
	}
	return out, nil
}

// sameAgentEntries reports whether two access lists hold the same principal and role
// pairs, in any order.
func sameAgentEntries(a, b []AgentAccessEntry) bool {
	if len(a) != len(b) {
		return false
	}
	want := make(map[AgentAccessEntry]struct{}, len(a))
	for _, entry := range a {
		want[entry] = struct{}{}
	}
	for _, entry := range b {
		if _, ok := want[entry]; !ok {
			return false
		}
	}
	return true
}

// roomForAgents locks the room inside tx. A room that is unknown, or belongs to another
// organization, answers room_not_found, the same as a room the caller cannot see.
func roomForAgents(ctx context.Context, tx pgx.Tx, orgID, spaceRef string) (*RegisteredSpace, error) {
	space, err := registeredSpaceForUpdate(ctx, tx, spaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, roomNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("lock Space for agent operation: %w", err)
	}
	if space.OrgID != orgID {
		return nil, roomNotFound()
	}
	return space, nil
}

// roomActorRole reads the caller's active role in the room inside tx. ok is false when
// the caller is not an active room member who is also an active organization member.
func roomActorRole(ctx context.Context, tx pgx.Tx, spaceRef, orgID, actorID string) (string, bool, error) {
	var role string
	err := tx.QueryRow(ctx, `
		SELECT m.role FROM space_memberships m
		WHERE m.space_ref=$1 AND m.subject_type='user' AND m.subject_id=$2 AND m.active=TRUE
		  AND EXISTS (SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
		               WHERE u.user_id=$2 AND u.org_id=$3 AND u.status='active' AND x.status='active')`,
		spaceRef, actorID, orgID).Scan(&role)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", false, nil
	}
	if err != nil {
		return "", false, fmt.Errorf("read acting room role: %w", err)
	}
	return role, true, nil
}

// orgPersonalAgentAdmit reads the organization's personal_agent_admit_entitled bit.
// No policy row means deny.
func orgPersonalAgentAdmit(ctx context.Context, tx pgx.Tx, orgID string) (bool, error) {
	var entitled bool
	err := tx.QueryRow(ctx, `
		SELECT COALESCE((SELECT p.personal_agent_admit_entitled FROM space_effect_policies p
		                  WHERE p.org_id=$1), FALSE)`, orgID).Scan(&entitled)
	if err != nil {
		return false, fmt.Errorf("read personal agent entitlement: %w", err)
	}
	return entitled, nil
}

// ownerInRoom reports whether an agent's owner is an active room member and an active
// organization member.
func ownerInRoom(ctx context.Context, tx pgx.Tx, spaceRef, orgID, ownerID string) (bool, error) {
	var in bool
	err := tx.QueryRow(ctx, `
		SELECT EXISTS (SELECT 1 FROM space_memberships m
		                WHERE m.space_ref=$1 AND m.subject_type='user' AND m.subject_id=$2 AND m.active=TRUE)
		   AND EXISTS (SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
		                WHERE u.user_id=$2 AND u.org_id=$3 AND u.status='active' AND x.status='active')`,
		spaceRef, ownerID, orgID).Scan(&in)
	if err != nil {
		return false, fmt.Errorf("read agent owner membership: %w", err)
	}
	return in, nil
}

// agentAdmitFacts reads what AdmitDenial needs for one agent. visible is false when the
// agent does not exist in the organization or the caller may not see it. The caller
// answers both the same way.
func agentAdmitFacts(ctx context.Context, tx pgx.Tx, space *RegisteredSpace, actorRoomRole, actorID, agentID string,
	orgPersonalAdmit bool) (AgentAdmitFacts, bool, error) {
	agent := AgentSubject{OrgID: space.OrgID, AgentSubjectID: agentID}
	var owner *string
	err := tx.QueryRow(ctx, `
		SELECT owner_user_id, visibility, add_policy, lifecycle, revision
		FROM agent_subjects WHERE org_id=$1 AND agent_subject_id=$2`,
		space.OrgID, agentID).Scan(&owner, &agent.Visibility, &agent.AddPolicy, &agent.Lifecycle, &agent.Revision)
	if errors.Is(err, pgx.ErrNoRows) {
		return AgentAdmitFacts{}, false, nil
	}
	if err != nil {
		return AgentAdmitFacts{}, false, fmt.Errorf("read agent subject: %w", err)
	}
	if owner != nil {
		agent.OwnerUserID = *owner
	}
	access, active, err := agentActorAccess(ctx, tx, space.OrgID, agentID, actorID)
	if err != nil {
		return AgentAdmitFacts{}, false, err
	}
	isOwner := agent.OwnerUserID != "" && agent.OwnerUserID == actorID
	if !active || (!isOwner && access == "" && agent.Visibility != AgentVisibilityOrg) {
		return AgentAdmitFacts{}, false, nil
	}
	facts := AgentAdmitFacts{
		SpaceActive:      space.RegistrationState == "active",
		SpaceKind:        space.Kind,
		ActorRoomRole:    actorRoomRole,
		Agent:            agent,
		ActorIsOwner:     isOwner,
		ActorAccess:      access,
		OrgPersonalAdmit: orgPersonalAdmit,
	}
	if agent.OwnerUserID != "" {
		ownerIn, err := ownerInRoom(ctx, tx, space.SpaceRef, space.OrgID, agent.OwnerUserID)
		if err != nil {
			return AgentAdmitFacts{}, false, err
		}
		facts.OwnerActiveInRoom = ownerIn
	}
	return facts, true, nil
}

// priorAdmissionKey reports whether an earlier admission in this room used the key for
// this agent. A key used for another agent, or for another event, is a conflict.
func priorAdmissionKey(ctx context.Context, tx pgx.Tx, spaceRef, agentID, key string) (bool, error) {
	var priorAgent, priorEvent string
	err := tx.QueryRow(ctx, `
		SELECT agent_subject_id, event FROM space_agent_admission_events
		WHERE space_ref=$1 AND idempotency_key=$2`, spaceRef, key).Scan(&priorAgent, &priorEvent)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("read agent admission idempotency key: %w", err)
	}
	if priorAgent != agentID || priorEvent != AdmissionEventAdmitted {
		return false, AgentError(409, CodeIdempotencyKeyReused, "idempotency_key was used for a different request", nil)
	}
	return true, nil
}

// admissionAndRevision reads the admission state of one agent in a room and the room's
// membership revision.
func admissionAndRevision(ctx context.Context, tx pgx.Tx, spaceRef, agentID string) (string, int64, error) {
	var state string
	if err := tx.QueryRow(ctx, `
		SELECT state FROM space_agent_admissions WHERE space_ref=$1 AND agent_subject_id=$2`,
		spaceRef, agentID).Scan(&state); err != nil {
		return "", 0, fmt.Errorf("read agent admission state: %w", err)
	}
	revision, err := membershipRevisionOf(ctx, tx, spaceRef)
	if err != nil {
		return "", 0, err
	}
	return state, revision, nil
}

// membershipRevisionOf reads a room's membership revision.
func membershipRevisionOf(ctx context.Context, tx pgx.Tx, spaceRef string) (int64, error) {
	var revision int64
	if err := tx.QueryRow(ctx, `
		SELECT membership_revision FROM space_authority_revisions WHERE space_ref=$1`,
		spaceRef).Scan(&revision); err != nil {
		return 0, fmt.Errorf("read Space membership revision: %w", err)
	}
	return revision, nil
}

// bumpSpaceMembershipRevision advances a room's authority and membership revisions once,
// after its membership rows changed.
func bumpSpaceMembershipRevision(ctx context.Context, tx pgx.Tx, spaceRef string) error {
	if _, err := tx.Exec(ctx, `
		UPDATE space_authority_revisions
		SET authority_revision=authority_revision+1, membership_revision=membership_revision+1, updated_at=NOW()
		WHERE space_ref=$1`, spaceRef); err != nil {
		return fmt.Errorf("advance Space membership revision: %w", err)
	}
	return nil
}

// insertAdmissionEvent appends one admission ledger row. Ids, states and the key only.
func insertAdmissionEvent(ctx context.Context, tx pgx.Tx, spaceRef, agentID, event, actorID, idempotencyKey string) error {
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_agent_admission_events
			(space_ref, agent_subject_id, event, actor_subject_id, idempotency_key)
		VALUES ($1, $2, $3, $4, $5)`,
		spaceRef, agentID, event, actorID, idempotencyKey); err != nil {
		return fmt.Errorf("append agent admission event: %w", err)
	}
	return nil
}

// deactivateAgentMemberships deactivates the service membership of each agent in a room,
// revokes its standing approval rules (WS10-S2) and returns the agents whose membership
// was active. The caller advances the room's revisions. A direct message's roster is fixed
// at registration (WS12-S1), so the update never touches a dm or group_dm Space.
func deactivateAgentMemberships(ctx context.Context, tx pgx.Tx, spaceRef string, agentIDs []string, reason string) ([]string, error) {
	var deactivated []string
	for _, agentID := range agentIDs {
		tag, err := tx.Exec(ctx, `
			UPDATE space_memberships SET active=FALSE, revision=revision+1, updated_at=NOW()
			WHERE space_ref=$1 AND subject_type='service' AND subject_id=$2 AND active=TRUE
			  AND NOT EXISTS (SELECT 1 FROM registered_spaces s
			                  WHERE s.space_ref=$1 AND s.space_kind IN ('dm', 'group_dm'))`,
			spaceRef, agentID)
		if err != nil {
			return nil, fmt.Errorf("deactivate agent membership: %w", err)
		}
		if tag.RowsAffected() == 0 {
			continue
		}
		if err := revokeApprovalRulesForAgent(ctx, tx, spaceRef, agentID, reason); err != nil {
			return nil, err
		}
		deactivated = append(deactivated, agentID)
	}
	return deactivated, nil
}

// suspendOwnedAgentsInRoom (cascade B, ADR 4) suspends the admission of each owner-visible
// agent of ownerID that is admitted to the room, because the owner has left it. It returns
// the agents whose service membership in the room is active. The caller deactivates them
// in the same transaction. The admission ledger records the suspension. The caller must
// hold the room row lock. Org-visible agents, ownerless or org-owned (D10), are never matched here.
func suspendOwnedAgentsInRoom(ctx context.Context, tx pgx.Tx, spaceRef, orgID, ownerID, actorID, reason string) ([]string, error) {
	rows, err := tx.Query(ctx, `
		SELECT adm.agent_subject_id
		FROM space_agent_admissions adm
		JOIN agent_subjects a ON a.org_id=adm.org_id AND a.agent_subject_id=adm.agent_subject_id
		WHERE adm.space_ref=$1 AND adm.org_id=$2 AND a.owner_user_id=$3
		  AND a.visibility=$4 AND adm.state=$5
		FOR UPDATE OF adm`,
		spaceRef, orgID, ownerID, AgentVisibilityOwner, AdmissionStateAdmitted)
	if err != nil {
		return nil, fmt.Errorf("read owned agent admissions: %w", err)
	}
	agentIDs, err := scanStrings(rows)
	if err != nil {
		return nil, err
	}
	for _, agentID := range agentIDs {
		if _, err := tx.Exec(ctx, `
			UPDATE space_agent_admissions
			SET state=$3, state_reason=$4, revision=revision+1, updated_at=NOW()
			WHERE space_ref=$1 AND agent_subject_id=$2`,
			spaceRef, agentID, AdmissionStateSuspended, reason); err != nil {
			return nil, fmt.Errorf("suspend agent admission: %w", err)
		}
		if err := insertAdmissionEvent(ctx, tx, spaceRef, agentID, AdmissionEventSuspended, actorID, ""); err != nil {
			return nil, err
		}
	}
	return activeServiceMembers(ctx, tx, spaceRef, agentIDs)
}

// activeServiceMembers returns the agents among agentIDs whose service membership in the
// room is active.
func activeServiceMembers(ctx context.Context, tx pgx.Tx, spaceRef string, agentIDs []string) ([]string, error) {
	if len(agentIDs) == 0 {
		return nil, nil
	}
	rows, err := tx.Query(ctx, `
		SELECT subject_id FROM space_memberships
		WHERE space_ref=$1 AND subject_type='service' AND active=TRUE AND subject_id = ANY($2::text[])
		ORDER BY subject_id`, spaceRef, agentIDs)
	if err != nil {
		return nil, fmt.Errorf("read active agent memberships: %w", err)
	}
	return scanStrings(rows)
}

// agentRosterAdmitted reports whether the roster may carry this agent in the room: its
// admission is admitted and the definition is active. An unknown agent is not admitted.
func agentRosterAdmitted(ctx context.Context, tx pgx.Tx, spaceRef, orgID, agentID string) (bool, error) {
	var admitted bool
	err := tx.QueryRow(ctx, `
		SELECT COALESCE(adm.state = $3 AND a.lifecycle = $4, FALSE)
		FROM agent_subjects a
		LEFT JOIN space_agent_admissions adm
		  ON adm.space_ref=$1 AND adm.org_id=a.org_id AND adm.agent_subject_id=a.agent_subject_id
		WHERE a.org_id=$2 AND a.agent_subject_id=$5`,
		spaceRef, orgID, AdmissionStateAdmitted, AgentLifecycleActive, agentID).Scan(&admitted)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("read agent admission for roster: %w", err)
	}
	return admitted, nil
}

// scanStrings reads a one-column text result and closes it.
func scanStrings(rows pgx.Rows) ([]string, error) {
	defer rows.Close()
	var out []string
	for rows.Next() {
		var value string
		if err := rows.Scan(&value); err != nil {
			return nil, fmt.Errorf("scan agent id: %w", err)
		}
		out = append(out, value)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read agent ids: %w", err)
	}
	return out, nil
}
