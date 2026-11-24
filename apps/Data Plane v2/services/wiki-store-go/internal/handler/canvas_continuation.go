package handler

import (
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/spacedecision"
)

// The approved-continuation lane of the canvas save (D22z, WS19-OQ2). An agent canvas write is an `ask` row, and the owner ruled
// that it resumes as a registered resumable kind: once a person grants the approval, the approval delivery worker in execution-core
// runs the save cold, as the requester, from the frozen continuation descriptor. That worker holds no user credential (a
// continuation descriptor is credential-free by design), so it presents its own service bearer instead of the run's delegated
// user bearer.
//
// The lane is narrow:
//   - It is served only on the save route (POST /spaces/{space_ref}/versions). Every read, the version list and restore keep
//     refusing any service bearer with space_decision_mismatch.
//   - It is off unless WIKISTORE_SPACE_CANVAS_CONTINUATION is the literal "true". The switch grants nothing; it only lets the
//     checks below run.
//   - The bearer must be the execution-core service principal and carry wiki.canvas.continue next to wiki.write.
//   - The decision must be an agent run-action decision (run-action-v1). A human decision is never accepted from a service.
//   - The decision is bound to the bearer's org, to this Space and to the canvas audience exactly as for a user bearer. The human
//     the run acts for comes from the Control-signed decision, never from the request: Control resolves the requester from the run.
//   - Everything after the envelope is the agent save's own path: the action, schema and write permission, the payload digest over
//     the arguments and the idempotency key, the refusal under any history floor, and the ZDR refusal of a decision signed under
//     zero data retention. A replay of the same key is the store's idempotent replay, so a worker retry never saves twice.

// canvasContinuationServiceID is the only service principal the continuation lane admits.
const canvasContinuationServiceID = "service:execution-core"

// canvasContinuationScope is the scope the continuation bearer must carry beside wiki.write.
const canvasContinuationScope = "wiki.canvas.continue"

// WithContinuation turns the approved-continuation lane of the save route on or off. Off is the default.
func (h *CanvasHandler) WithContinuation(enabled bool) *CanvasHandler {
	h.continuation = enabled
	return h
}

// verifyWriteCaller verifies the decision a save carries. A user bearer takes the ordinary path. A service bearer is admitted only
// through the continuation lane, and only while it is on; otherwise it is refused as the ordinary path refuses it.
func (h *CanvasHandler) verifyWriteCaller(r *http.Request, bearer *authctx.Claims, spaceRef string) (canvasCaller, *canvasRefusal) {
	if !bearer.IsService() {
		return h.verifyCaller(r, bearer, spaceRef)
	}
	return h.verifyContinuationCaller(r, bearer, spaceRef)
}

// verifyContinuationCaller verifies a service bearer's save under the continuation lane. It decides the envelope only, as
// verifyCaller does; authority, the digest, the floor and ZDR are decided by the save route afterwards.
func (h *CanvasHandler) verifyContinuationCaller(r *http.Request, bearer *authctx.Claims, spaceRef string) (canvasCaller, *canvasRefusal) {
	token := strings.TrimSpace(r.Header.Get(canvasDecisionHeader))
	if token == "" || len(token) > spacedecision.MaxEnvelopeBytes {
		return canvasCaller{}, refusalOf(http.StatusBadRequest, "space_decision_required",
			"a Control Space decision of at most 16 KiB is required")
	}
	notIssued := refusalOf(http.StatusForbidden, spacedecision.CodeDecisionMismatch,
		"the Space decision is not issued to this principal")
	if !h.continuation || bearer.ServiceID != canvasContinuationServiceID || bearer.UserID != "" ||
		!claimsHaveScope(bearer, canvasContinuationScope) {
		return canvasCaller{}, notIssued
	}
	// A service never presents a human decision: only the run-action decision names the run and the human it acts for.
	if !strings.HasPrefix(token, spacedecision.RunEnvelopeVersion+".") {
		return canvasCaller{}, notIssued
	}
	run, err := spacedecision.VerifyRunAction(token, h.keys, time.Now().UTC())
	if err != nil {
		return canvasCaller{}, refusalFromErr(err)
	}
	if strings.TrimSpace(run.SubjectID) == "" {
		return canvasCaller{}, notIssued
	}
	// The subject is the decision's own: Control resolved the requester from the run. The org and the Space are still bound to
	// this request.
	if err := run.CheckEnvelope(bearer.OrgID, run.SubjectID, spaceRef); err != nil {
		return canvasCaller{}, refusalFromErr(err)
	}
	return canvasCaller{agent: true, run: run}, nil
}

// claimsHaveScope reports whether the verified bearer carries scope.
func claimsHaveScope(claims *authctx.Claims, scope string) bool {
	for _, held := range claims.Scopes {
		if held == scope {
			return true
		}
	}
	return false
}
