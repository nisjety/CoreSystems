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

// D21h: POST /api/v1/internal/spaces/browser-run-stop-decision, registered
// through the P-UC1 seam. It is a gateway-delegated issuer route (scope
// spaces:issue, scopes_browser_run_stop.go). The body is {space_ref, run_id,
// idempotency_key} and nothing else: an actor, organisation, role, audience or
// any authority field is refused as an unknown field, never ignored.
//
// Any current member of the room may get the decision; it binds the caller,
// the Space and the run, and the model-gateway verifier refuses it unless the
// run is this Space's and was started by the caller. Managers keep their own
// path (run-control-decision) for other members' runs.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/browser-run-stop-decision", s.requireVerifiedSpaceResolver, s.issueBrowserRunStopDecision)
	})
}

// spaceBrowserRunStopDeniedMessage is the fixed refusal string the gateway maps
// to its typed starter-stop refusal. spaceBrowserRunStopInvalidMessage is the
// 400 for a malformed request.
const (
	spaceBrowserRunStopDeniedMessage  = "Space browser run stop is not authorized"
	spaceBrowserRunStopInvalidMessage = "valid browser run stop request required"
)

type spaceBrowserRunStopDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	RunID          string `json:"run_id"`
	IdempotencyKey string `json:"idempotency_key"`
}

// issueBrowserRunStopDecision answers 200 {data:{decision, token, claims}}. A
// malformed body is a 400 before any authority is read. Every authority refusal
// (not a member, personal or DM Space, ZDR) is a 403 with the fixed message and
// the typed denial code where one exists. An archived room still issues it.
// With the Space repository unwired the route answers 503.
func (s *Server) issueBrowserRunStopDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request spaceBrowserRunStopDecisionRequest
	if !decodeBrowserRunStopDecisionBody(c, &request) {
		return
	}
	shape := spaces.BrowserRunStopDecisionRequest{
		DecisionRef: "x", RunID: request.RunID, IdempotencyKey: request.IdempotencyKey, Nonce: "x",
	}
	if strings.TrimSpace(request.SpaceRef) == "" || shape.Validate() != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": spaceBrowserRunStopInvalidMessage})
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedBrowserRunStopDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, spaceBrowserRunStopDeniedMessage,
		http.StatusForbidden, spaceBrowserRunStopDeniedMessage) {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, claims, err := spaces.IssueBrowserRunStopDecision(evidence, spaces.BrowserRunStopDecisionRequest{
		DecisionRef: decisionRef, RunID: request.RunID, IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, spaceBrowserRunStopDeniedMessage)
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token, "claims": claims}})
}

// decodeBrowserRunStopDecisionBody reads the body strictly, as the browser run
// decision does: an unknown field is a 400 and is never silently dropped.
func decodeBrowserRunStopDecisionBody(c *gin.Context, dst *spaceBrowserRunStopDecisionRequest) bool {
	decoder := json.NewDecoder(io.LimitReader(c.Request.Body, 1<<16))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(dst); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": spaceBrowserRunStopInvalidMessage})
		return false
	}
	return true
}
