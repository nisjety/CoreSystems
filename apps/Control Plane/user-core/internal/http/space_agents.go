package http

import (
	"context"
	"errors"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// WS17-S4: the personal agent registry (/api/v1/internal/agents/subjects) and the room
// agent routes (/api/v1/internal/spaces/{space_ref}/agent-*). Every route answers the
// typed error envelope {"error":{"code","message","details"}}.
//
// Both families take the gateway principal with a verified delegation and a scope:
// agents:manage for the registry, spaces:agents:manage for the room routes. The acting
// user and the organization come only from the signed delegation, never from the body
// or the path. No flag gates these routes: AdmitDenial and the access list decide.

const (
	agentsManageScope      = "agents:manage"
	spaceAgentsManageScope = "spaces:agents:manage"
)

// requireGatewayDelegation admits only the gateway workload, carrying a verified
// delegation and the given scope. It is modelled on requireSpaceMembershipActor.
func requireGatewayDelegation(scope string) gin.HandlerFunc {
	return func(c *gin.Context) {
		if c.GetString("auth_method") != "service_principal" ||
			c.GetString("service_id") != verevonGatewayPrincipal ||
			!c.GetBool("delegation_verified") ||
			!hasServiceScope(c, scope) ||
			strings.TrimSpace(c.GetString("user_id")) == "" ||
			strings.TrimSpace(c.GetString("org_id")) == "" {
			abortTypedError(c, http.StatusForbidden, spaces.CodeAgentDelegationRequired,
				"a signed gateway delegation with the agent scope is required", nil)
			return
		}
		c.Next()
	}
}

// writeAgentError answers a registry error. A typed refusal keeps its status and code.
// Anything else is an outage: 503 agent_registry_unavailable, logged without content.
func writeAgentError(c *gin.Context, err error) {
	var roomErr *spaces.RoomError
	if errors.As(err, &roomErr) {
		abortTypedError(c, roomErr.Status, roomErr.Code, roomErr.Message, roomErr.Details)
		return
	}
	log.Error().Err(err).Msg("agent registry operation failed")
	abortTypedError(c, http.StatusServiceUnavailable, spaces.CodeAgentRegistryUnavailable,
		"the agent registry is unavailable", nil)
}

// unadmittedList renders skipped_unadmitted as a JSON array, never null.
func unadmittedList(ids []string) []string {
	if ids == nil {
		return []string{}
	}
	return ids
}

type registerAgentSubjectBody struct {
	AgentSubjectID string `json:"agent_subject_id"`
	Visibility     string `json:"visibility"`
	AddPolicy      string `json:"add_policy"`
}

// registerAgentSubject records an agent definition the verified actor owns (201), or
// replays the same owner's definition (200).
func (s *Server) registerAgentSubject(c *gin.Context) {
	if s.spaceRepo == nil {
		writeAgentError(c, errors.New("agent registry repository unavailable"))
		return
	}
	var body registerAgentSubjectBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	view, created, err := s.spaceRepo.RegisterAgentSubject(c.Request.Context(), spaces.AgentRegisterRequest{
		OrgID:          c.GetString("org_id"),
		ActorID:        c.GetString("user_id"),
		AgentSubjectID: body.AgentSubjectID,
		Visibility:     body.Visibility,
		AddPolicy:      body.AddPolicy,
	})
	if err != nil {
		writeAgentError(c, err)
		return
	}
	status := http.StatusOK
	if created {
		status = http.StatusCreated
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(status, gin.H{"data": view})
}

// listAgentSubjects lists the agent definitions the verified actor may see.
func (s *Server) listAgentSubjects(c *gin.Context) {
	if s.spaceRepo == nil {
		writeAgentError(c, errors.New("agent registry repository unavailable"))
		return
	}
	views, err := s.spaceRepo.VisibleAgents(c.Request.Context(), c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		writeAgentError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"agents": views}})
}

// getAgentSubject returns one agent definition the verified actor may see. An agent the
// actor may not see answers 404, the same as an unknown id.
func (s *Server) getAgentSubject(c *gin.Context) {
	if s.spaceRepo == nil {
		writeAgentError(c, errors.New("agent registry repository unavailable"))
		return
	}
	view, err := s.spaceRepo.VisibleAgent(c.Request.Context(), c.GetString("org_id"), c.GetString("user_id"),
		c.Param("agent_subject_id"))
	if err != nil {
		writeAgentError(c, err)
		return
	}
	if view == nil {
		abortTypedError(c, http.StatusNotFound, spaces.CodeAgentNotFound, "agent not found", nil)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": view})
}

type updateAgentAccessBody struct {
	Visibility       string                    `json:"visibility"`
	AddPolicy        string                    `json:"add_policy"`
	Entries          []spaces.AgentAccessEntry `json:"entries"`
	ExpectedRevision *int64                    `json:"expected_revision"`
}

// updateAgentAccess replaces an agent's visibility, add policy and access list at the
// revision the editor read. A stale revision answers 409 agent_state_changed.
func (s *Server) updateAgentAccess(c *gin.Context) {
	if s.spaceRepo == nil {
		writeAgentError(c, errors.New("agent registry repository unavailable"))
		return
	}
	var body updateAgentAccessBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	if body.ExpectedRevision == nil {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest,
			"expected_revision is required", map[string]any{"field": "expected_revision"})
		return
	}
	view, err := s.spaceRepo.UpdateAgentAccess(c.Request.Context(), spaces.AgentAccessRequest{
		OrgID:            c.GetString("org_id"),
		ActorID:          c.GetString("user_id"),
		AgentSubjectID:   c.Param("agent_subject_id"),
		Visibility:       body.Visibility,
		AddPolicy:        body.AddPolicy,
		Entries:          body.Entries,
		ExpectedRevision: *body.ExpectedRevision,
		// D19/D22as: a canonical org owner or admin may edit an ownerless org-visible
		// agent. The role is read from the membership authority only when the repository
		// needs it, never from the body.
		ResolveOrgRole: func(ctx context.Context) (string, error) {
			role, err := s.orgAdminRole(ctx, strings.TrimSpace(c.GetString("user_id")), strings.TrimSpace(c.GetString("org_id")))
			if errors.Is(err, ErrMembershipNotFound) {
				// Not a canonical member: no org role, so a plain 403, not a 503.
				return "", nil
			}
			return role, err
		},
	})
	if err != nil {
		writeAgentError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": view})
}

type agentCandidatesBody struct {
	AgentSubjectIDs []string `json:"agent_subject_ids"`
}

// spaceAgentCandidates answers, for each agent id, whether the verified actor may add it
// to the room now. An agent the actor cannot see answers like an unknown id.
func (s *Server) spaceAgentCandidates(c *gin.Context) {
	if s.spaceRepo == nil {
		writeAgentError(c, errors.New("agent registry repository unavailable"))
		return
	}
	var body agentCandidatesBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	candidates, err := s.spaceRepo.AgentCandidates(c.Request.Context(), c.GetString("org_id"),
		c.GetString("user_id"), c.Param("space_ref"), body.AgentSubjectIDs)
	if err != nil {
		writeAgentError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"candidates": candidates}})
}

type admitSpaceAgentBody struct {
	AgentSubjectID string `json:"agent_subject_id"`
	IdempotencyKey string `json:"idempotency_key"`
}

// admitSpaceAgent admits an agent to a room under AdmitDenial: 201 when the admission
// changed, 200 when it was already in place or replayed.
func (s *Server) admitSpaceAgent(c *gin.Context) {
	if s.spaceRepo == nil {
		writeAgentError(c, errors.New("agent registry repository unavailable"))
		return
	}
	var body admitSpaceAgentBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	result, err := s.spaceRepo.AdmitAgent(c.Request.Context(), spaces.AgentAdmitRequest{
		OrgID:          c.GetString("org_id"),
		ActorID:        c.GetString("user_id"),
		SpaceRef:       c.Param("space_ref"),
		AgentSubjectID: body.AgentSubjectID,
		IdempotencyKey: body.IdempotencyKey,
	})
	if err != nil {
		writeAgentError(c, err)
		return
	}
	status := http.StatusOK
	if result.Changed {
		status = http.StatusCreated
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(status, gin.H{"data": gin.H{
		"admission_ref":       "agent-admission:" + result.SpaceRef + ":" + result.AgentSubjectID,
		"space_ref":           result.SpaceRef,
		"agent_subject_id":    result.AgentSubjectID,
		"state":               result.State,
		"changed":             result.Changed,
		"replayed":            result.Replayed,
		"membership_revision": result.MembershipRevision,
	}})
}

// withdrawSpaceAgent withdraws an admitted agent from a room and removes its seat.
func (s *Server) withdrawSpaceAgent(c *gin.Context) {
	if s.spaceRepo == nil {
		writeAgentError(c, errors.New("agent registry repository unavailable"))
		return
	}
	result, err := s.spaceRepo.WithdrawAgent(c.Request.Context(), spaces.AgentWithdrawRequest{
		OrgID:          c.GetString("org_id"),
		ActorID:        c.GetString("user_id"),
		SpaceRef:       c.Param("space_ref"),
		AgentSubjectID: c.Param("agent_subject_id"),
	})
	if err != nil {
		writeAgentError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"state":               result.State,
		"changed":             result.Changed,
		"membership_revision": result.MembershipRevision,
	}})
}
