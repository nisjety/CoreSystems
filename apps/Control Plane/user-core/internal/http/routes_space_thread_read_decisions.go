package http

import (
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS11-S1: bulk shared-Space read decisions, registered through the P-UC1 seam.
//
// POST /api/v1/internal/spaces/thread-read-decisions issues up to
// maxThreadReadDecisionItems ordinary `model.thread.read` decisions in one
// round trip, so the cross-room attention aggregate does not need one Control
// call per room. Each item goes through exactly the evidence resolution and
// issuer the single route (issueThreadReadDecision) uses, so every token is an
// ordinary decision that Session Core's existing verifier checks on its own:
// no new action, schema, digest or table. The scope is `spaces:issue`
// (scopes_space_thread_read_decisions.go), so the route needs the verified
// gateway delegation exactly like the single route.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/thread-read-decisions", s.requireVerifiedSpaceResolver, s.issueThreadReadDecisions)
	})
}

const maxThreadReadDecisionItems = 60

type threadReadDecisionsRequestItem struct {
	SpaceRef       string `json:"space_ref"`
	IdempotencyKey string `json:"idempotency_key"`
}

type threadReadDecisionsRequest struct {
	Items []threadReadDecisionsRequestItem `json:"items"`
	// Optional, as on the single route; one audience for the whole batch.
	ServiceAudience string `json:"service_audience"`
}

// Closed `reason` words of a denied row. A denied row carries no Space detail
// beyond the reason: a non-member never learns whether the Space exists.
const (
	threadReadDeniedNotMember    = "not_member"
	threadReadDeniedNotEntitled  = "not_entitled"
	threadReadDeniedNotActive    = "not_active"
	threadReadDeniedUnavailable  = "unavailable"
	threadReadDecisionStatusOK   = "issued"
	threadReadDecisionStatusDeny = "denied"
)

func (s *Server) issueThreadReadDecisions(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request threadReadDecisionsRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "items (1 to 60 of space_ref and idempotency_key) are required"})
		return
	}
	if len(request.Items) == 0 || len(request.Items) > maxThreadReadDecisionItems {
		c.JSON(http.StatusBadRequest, gin.H{"error": "items must hold between 1 and 60 entries"})
		return
	}
	seen := make(map[string]struct{}, len(request.Items))
	for _, item := range request.Items {
		if strings.TrimSpace(item.SpaceRef) == "" || strings.TrimSpace(item.IdempotencyKey) == "" {
			c.JSON(http.StatusBadRequest, gin.H{"error": "every item needs space_ref and idempotency_key"})
			return
		}
		if _, dup := seen[item.SpaceRef]; dup {
			c.JSON(http.StatusBadRequest, gin.H{"error": "space_ref must be unique within items"})
			return
		}
		seen[item.SpaceRef] = struct{}{}
	}
	// Refuse an unknown audience once, up front, rather than once per row.
	if err := (spaces.SharedThreadReadDecisionRequest{
		DecisionRef: "x", IdempotencyKey: "x", Nonce: "x", ServiceAudience: request.ServiceAudience,
	}).Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "service_audience is not a recognized Model Plane recipient"})
		return
	}

	orgID, userID := c.GetString("org_id"), c.GetString("user_id")
	results := make([]gin.H, 0, len(request.Items))
	for _, item := range request.Items {
		evidence, issue, err, ownDenial := s.resolveThreadReadEvidence(c.Request.Context(), item.SpaceRef, orgID, userID)
		if err != nil {
			results = append(results, deniedThreadReadRow(item.SpaceRef, threadReadDenialReason(err, ownDenial)))
			continue
		}
		key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
		if !ok {
			return
		}
		decision, err := issue(evidence, spaces.SharedThreadReadDecisionRequest{
			DecisionRef: decisionRef, IdempotencyKey: item.IdempotencyKey, Nonce: nonce,
			ServiceAudience: request.ServiceAudience,
		}, time.Now().UTC())
		if err != nil {
			results = append(results, deniedThreadReadRow(item.SpaceRef, threadReadDenialReason(err, nil)))
			continue
		}
		token, ok := signSpaceDecision(c, key, decision)
		if !ok {
			return
		}
		results = append(results, gin.H{
			"space_ref": item.SpaceRef, "status": threadReadDecisionStatusOK, "decision": decision, "token": token,
		})
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"results": results}})
}

func deniedThreadReadRow(spaceRef, reason string) gin.H {
	return gin.H{"space_ref": spaceRef, "status": threadReadDecisionStatusDeny, "reason": reason}
}

// threadReadDenialReason maps a refusal onto the closed reason words. A typed
// own-scheduled refusal (only set when the shared path found no membership)
// takes precedence, as on the single route. Anything unclassified fails closed
// as not_entitled; a failure to resolve at all is `unavailable`, so a caller
// can tell a transient miss from a real refusal.
func threadReadDenialReason(err error, ownDenial *spaces.Denial) string {
	denial := ownDenial
	if denial == nil {
		if typed, ok := spaces.AsDenial(err); ok {
			denial = typed
		}
	}
	if denial != nil {
		switch denial.Code {
		case spaces.DenialMembershipRequired, spaces.DenialAudienceNotCurrent:
			return threadReadDeniedNotMember
		case spaces.DenialNotActive:
			return threadReadDeniedNotActive
		case spaces.DenialAuthorityUnavailable:
			return threadReadDeniedUnavailable
		default:
			return threadReadDeniedNotEntitled
		}
	}
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		return threadReadDeniedNotMember
	}
	return threadReadDeniedUnavailable
}
