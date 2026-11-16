package handler

import (
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/spacedecision"
)

// canvasCaller is the verified Control decision a canvas request carries (WS19-S9). A human decision (v2) names the person who
// acted. An agent decision (run-action-v1) names the run; the human it acts for is the run's subject. The author of a version is
// stamped from the verified decision, never from the request body.
type canvasCaller struct {
	agent bool
	user  spacedecision.Claims
	run   spacedecision.RunClaims
}

// authorizeCaller verifies the decision a canvas request carries and binds it to the route, as authorize does for a human. agentOK
// says whether the route accepts an agent decision at all. A route that does not accept one refuses a run-action envelope before any
// verification, with space_decision_mismatch.
func (h *CanvasHandler) authorizeCaller(r *http.Request, bearer *authctx.Claims, spaceRef string, write, agentOK bool) (canvasCaller, *canvasRefusal) {
	token := strings.TrimSpace(r.Header.Get(canvasDecisionHeader))
	if token == "" || len(token) > spacedecision.MaxEnvelopeBytes {
		return canvasCaller{}, refusalOf(http.StatusBadRequest, "space_decision_required",
			"a Control Space decision of at most 16 KiB is required")
	}
	if bearer.IsService() || bearer.UserID == "" {
		return canvasCaller{}, refusalOf(http.StatusForbidden, spacedecision.CodeDecisionMismatch,
			"the Space decision is not issued to this principal")
	}
	now := time.Now().UTC()
	if strings.HasPrefix(token, spacedecision.RunEnvelopeVersion+".") {
		if !agentOK {
			return canvasCaller{}, refusalOf(http.StatusForbidden, spacedecision.CodeDecisionMismatch,
				"this canvas route does not accept an agent Space decision")
		}
		run, err := spacedecision.VerifyRunAction(token, h.keys, now)
		if err != nil {
			return canvasCaller{}, refusalFromErr(err)
		}
		if err := run.CheckCanvas(bearer.OrgID, bearer.UserID, spaceRef, write); err != nil {
			return canvasCaller{}, refusalFromErr(err)
		}
		return canvasCaller{agent: true, run: run}, nil
	}
	claims, err := spacedecision.Verify(token, h.keys, now)
	if err != nil {
		return canvasCaller{}, refusalFromErr(err)
	}
	if err := claims.CheckCanvas(bearer.OrgID, bearer.UserID, spaceRef, write); err != nil {
		return canvasCaller{}, refusalFromErr(err)
	}
	return canvasCaller{user: claims}, nil
}

// author is the verified principal that stamps a version: the user for a human decision, and for an agent decision the run, with
// the human it acts for as on_behalf_of.
func (c canvasCaller) author() canvas.Author {
	if c.agent {
		return canvas.Author{Kind: canvas.AuthorAgent, ID: "run:" + c.run.RunID, OnBehalfOf: c.run.SubjectID, RunID: c.run.RunID}
	}
	return canvasUserAuthor(c.user)
}

// provenance is the decision a write was made under. It is provenance, not authority.
func (c canvasCaller) provenance() canvas.Provenance {
	if c.agent {
		return canvas.Provenance{
			DecisionRef:       c.run.DecisionRef,
			AuthorityRevision: c.run.AuthorityRevision,
			AudienceRevision:  c.run.RecipientAudienceRevision,
		}
	}
	return canvasProvenance(c.user)
}

// checkRead recomputes the digest of a read. A human read carries the decision's key and no canvas; an agent read has no arguments.
func (c canvasCaller) checkRead() *canvasRefusal {
	if c.agent {
		return checkRunDigest(c.run, spacedecision.ActionRunCanvasRead, c.run.IdempotencyKey, nil)
	}
	return checkCanvasDigest(c.user, readOp(c.user))
}

// checkWrite binds a write to its decision. op is the human operation (create or save over the canvas the Space holds). An agent
// digest covers the arguments and the key instead, as the golden registry defines it; the canvas is the one the Space holds under
// the lock, which the repository checks.
func (c canvasCaller) checkWrite(op spacedecision.CanvasOp, args []spacedecision.RunArgument, idempotencyKey string) *canvasRefusal {
	if c.agent {
		return checkRunDigest(c.run, spacedecision.ActionRunCanvasWrite, idempotencyKey, args)
	}
	return checkCanvasDigest(c.user, op)
}

// checkRunDigest recomputes the agent payload digest from the verified claims and this request's arguments, and compares it with the
// digest the execution lane signed. The request's idempotency key must also be the one the decision names.
func checkRunDigest(run spacedecision.RunClaims, action, idempotencyKey string, args []spacedecision.RunArgument) *canvasRefusal {
	if idempotencyKey != run.IdempotencyKey ||
		spacedecision.RunPayloadDigest(action, run.RunID, run.OrgID, run.IdempotencyKey, args) != run.PayloadDigest {
		return refusalOf(http.StatusForbidden, spacedecision.CodeDigestMismatch, "the Space decision does not match this request")
	}
	return nil
}

// runWriteArgs flattens a save's arguments in the registry's argument_order: expected_version_id, content, summary. A null expected
// version and an absent summary flatten to the empty string. The summary is digested as the caller sent it, untrimmed.
func runWriteArgs(body saveBody) []spacedecision.RunArgument {
	return []spacedecision.RunArgument{
		{Name: "expected_version_id", Value: derefString(body.ExpectedVersionID)},
		{Name: "content", Value: body.Content},
		{Name: "summary", Value: derefString(body.Summary)},
	}
}
