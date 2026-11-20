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

// verifyCaller verifies the decision envelope a canvas request carries (D21n, the step before not-found): the header is present,
// the bearer is a user, the signature and life verify, and the decision is addressed to this org, subject, Space and the canvas
// audience. It decides nothing about the operation: authorize, the digest, the floor and refuseZDR do that after the route has
// answered its not-found refusals.
func (h *CanvasHandler) verifyCaller(r *http.Request, bearer *authctx.Claims, spaceRef string) (canvasCaller, *canvasRefusal) {
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
		run, err := spacedecision.VerifyRunAction(token, h.keys, now)
		if err != nil {
			return canvasCaller{}, refusalFromErr(err)
		}
		if err := run.CheckEnvelope(bearer.OrgID, bearer.UserID, spaceRef); err != nil {
			return canvasCaller{}, refusalFromErr(err)
		}
		return canvasCaller{agent: true, run: run}, nil
	}
	claims, err := spacedecision.Verify(token, h.keys, now)
	if err != nil {
		return canvasCaller{}, refusalFromErr(err)
	}
	if err := claims.CheckEnvelope(bearer.OrgID, bearer.UserID, spaceRef); err != nil {
		return canvasCaller{}, refusalFromErr(err)
	}
	return canvasCaller{user: claims}, nil
}

// authorize binds a verified decision to the route's operation. agentOK says whether the route accepts an agent decision at all;
// one that does not refuses a run-action decision with space_decision_mismatch. Then the action, schema, permission and Space kind
// must be the route's.
func (c canvasCaller) authorize(write, agentOK bool) *canvasRefusal {
	if c.agent {
		if !agentOK {
			return refusalOf(http.StatusForbidden, spacedecision.CodeDecisionMismatch,
				"this canvas route does not accept an agent Space decision")
		}
		if err := c.run.CheckAuthority(write); err != nil {
			return refusalFromErr(err)
		}
		return nil
	}
	if err := c.user.CheckAuthority(write); err != nil {
		return refusalFromErr(err)
	}
	return nil
}

// refuseZDR answers a decision issued under zero data retention with the catalogue refusal. It is the last refusal before the
// store is used (D21n): a decision Control signed under ZDR is refused once the route knows the canvas exists and the decision is
// the route's own.
func (c canvasCaller) refuseZDR() *canvasRefusal {
	if (c.agent && c.run.ZeroDataRetention) || (!c.agent && c.user.ZeroDataRetention) {
		return canvasZDRRefusal()
	}
	return nil
}

// firstRefusal runs the checks in order and returns the first refusal.
func firstRefusal(checks ...func() *canvasRefusal) *canvasRefusal {
	for _, check := range checks {
		if refused := check(); refused != nil {
			return refused
		}
	}
	return nil
}

// probeCanvas answers a route's not-found refusals (D21n), after the decision envelope and before authority. versionID names the
// version the route reads or restores ("" for none). createOK is set for the save route, where a Space with no canvas is a create,
// not a refusal; the save route decides its own not-found answers from the probe. It reports false when it wrote an answer.
func (h *CanvasHandler) probeCanvas(w http.ResponseWriter, r *http.Request, orgID, spaceRef, versionID string, createOK bool) (canvas.Probe, bool) {
	probe, err := h.repo.Probe(r.Context(), orgID, spaceRef, versionID)
	if err != nil {
		writeCanvasStoreError(w, err)
		return canvas.Probe{}, false
	}
	if probe.CanvasID == "" {
		if createOK {
			return probe, true
		}
		writeCanvasError(w, http.StatusNotFound, canvas.CodeCanvasNotFound, "the Space has no canvas", nil)
		return canvas.Probe{}, false
	}
	if versionID != "" && !probe.VersionFound {
		writeCanvasError(w, http.StatusNotFound, canvas.CodeVersionNotFound, "the version is not in this canvas", nil)
		return canvas.Probe{}, false
	}
	return probe, true
}

// author is the verified principal that stamps a version: the user for a human decision, and for an agent decision the run, with
// the human it acts for as on_behalf_of.
func (c canvasCaller) author() canvas.Author {
	if c.agent {
		return canvas.Author{Kind: canvas.AuthorAgent, ID: "run:" + c.run.RunID, OnBehalfOf: c.run.SubjectID, RunID: c.run.RunID}
	}
	return canvasUserAuthor(c.user)
}

// floor is the history floor the verified decision carries, as the canvas repository applies it. A human decision carries a
// WS04-S11 revision floor, a D18 timestamp floor, or both. An agent decision carries a timestamp floor on a canvas read only:
// Control refuses a revision floor on a run action, and the agent reader refuses any claim it does not name.
func (c canvasCaller) floor() canvas.Floor {
	if c.agent {
		return canvas.Floor{At: c.run.HistoryFloorAt}
	}
	return canvas.Floor{At: c.user.HistoryFloorAt, Revision: c.user.HistoryFloorRevision}
}

// floored reports whether the decision's member has a history window that windows the canvas.
func (c canvasCaller) floored() bool { return c.floor().Set() }

// refuseWriteUnderFloor refuses a write that a history floor forbids. Control issues a floored decision for a create only: a
// create writes new text, and a canvas that does not exist holds no pre-floor text. A save or a restore under a floor is refused,
// because a save answers a head-moved or unchanged result that names the head, and a restore makes an older version the head.
// An agent write under any floor is refused, because Control never issues one. create is false for a restore and for a save
// over an existing canvas.
func (c canvasCaller) refuseWriteUnderFloor(create bool) *canvasRefusal {
	if !c.floored() {
		return nil
	}
	if c.agent || !create {
		return refusalOf(http.StatusForbidden, spacedecision.CodeHistoryFloor,
			"a canvas save or restore is not available under this member's history window")
	}
	return nil
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
