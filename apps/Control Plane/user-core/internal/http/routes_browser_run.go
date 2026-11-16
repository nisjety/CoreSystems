package http

import (
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS20-S1: POST /api/v1/internal/spaces/browser-run-decision, registered through the
// P-UC1 seam. It is a gateway-delegated issuer route (scope spaces:issue, see
// scopes_browser_run.go). The body names no actor, organisation, role, audience,
// policy or revision, and an unknown field is refused rather than ignored. Membership
// and authority come from Control state; browser_entitled and the editor floor decide.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/browser-run-decision", s.requireVerifiedSpaceResolver, s.issueBrowserRunDecision)
	})
}

// spaceBrowserRunDeniedMessage is the fixed refusal string the gateway maps to its
// typed browser run refusal. spaceBrowserRunInvalidMessage is the 400 for a malformed
// intent.
const (
	spaceBrowserRunDeniedMessage  = "Space browser run is not authorized"
	spaceBrowserRunInvalidMessage = "valid browser run intent required"
)

type spaceBrowserRunDecisionRequest struct {
	SpaceRef       string   `json:"space_ref"`
	ThreadID       string   `json:"thread_id"`
	GoalDigest     string   `json:"goal_digest"`
	AllowedDomains []string `json:"allowed_domains"`
	ViewScope      string   `json:"view_scope"`
	MaxSteps       int      `json:"max_steps"`
	MaxRuntimeS    int      `json:"max_runtime_s"`
	IdempotencyKey string   `json:"idempotency_key"`
}

// issueBrowserRunDecision answers 200 {data:{decision, token, claims}} for an
// authorized run. A malformed body or intent is a 400 before any authority is read.
// Every authority refusal is a 403 with the fixed message and the typed denial code
// where one exists. A write to an archived room answers the typed 409 room_archived.
// With the Space repository unwired the route answers 503.
func (s *Server) issueBrowserRunDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request spaceBrowserRunDecisionRequest
	if !decodeBrowserRunDecisionBody(c, &request) {
		return
	}
	shape := spaces.BrowserRunDecisionRequest{
		DecisionRef: "x", ThreadID: request.ThreadID, GoalDigest: request.GoalDigest,
		AllowedDomains: request.AllowedDomains, ViewScope: request.ViewScope,
		MaxSteps: request.MaxSteps, MaxRuntimeS: request.MaxRuntimeS,
		IdempotencyKey: request.IdempotencyKey, Nonce: "x",
	}
	if strings.TrimSpace(request.SpaceRef) == "" || shape.Validate() != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": spaceBrowserRunInvalidMessage})
		return
	}
	evidence, _, ok := s.resolveThreadWriteEvidence(c, request.SpaceRef,
		s.spaceRepo.ResolvePersonalBrowserRunDecisionEvidence, s.spaceRepo.ResolveSharedBrowserRunDecisionEvidence,
		spaceBrowserRunDeniedMessage)
	if !ok {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, claims, err := spaces.IssueBrowserRunDecision(evidence, spaces.BrowserRunDecisionRequest{
		DecisionRef: decisionRef, ThreadID: request.ThreadID, GoalDigest: request.GoalDigest,
		AllowedDomains: request.AllowedDomains, ViewScope: request.ViewScope,
		MaxSteps: request.MaxSteps, MaxRuntimeS: request.MaxRuntimeS,
		IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, spaceBrowserRunDeniedMessage)
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token, "claims": claims}})
}

// decodeBrowserRunDecisionBody reads the body strictly. An unknown field, such as an
// actor, organisation, role, audience or any authority field a caller tries to send,
// is a 400 and is never silently dropped.
func decodeBrowserRunDecisionBody(c *gin.Context, dst any) bool {
	decoder := json.NewDecoder(io.LimitReader(c.Request.Body, 1<<16))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(dst); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": spaceBrowserRunInvalidMessage})
		return false
	}
	return true
}
