package api

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"net/http"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
)

// P2-go-15-rel1: a Space skill's bundle is part of the skill's content, so an upload or a delete of that bundle is a
// Space skill write too. A Space skill is changed by a Space decision, never by an org role (EP-11 D7), and the skill's
// own create, update and delete verify that decision (F-f10-19). The bundle routes verify the same decision with the
// same verifier, for the operation, the Space the stored row names, the skill, the person and the body they receive.
// Both SKILL_SPACE_SCOPE and SKILL_SPACE_WRITE_VERIFY must be the literal "true"; either off, the bundle routes are as
// they were.

// storedSkillScopeSQL reads the scope and the Space of the org's skill. The Space is empty for a row that names none.
const storedSkillScopeSQL = `SELECT scope, COALESCE(scope_id, '') FROM agent_skills WHERE id = $1 AND org_id = $2`

// spaceSkillBundleOperation is the operation a bundle write binds. A bundle changes one Space skill, named by its id, and
// the skill itself stays, so it is an update. Its body digest tells a bundle write from an edit of the skill's text.
const spaceSkillBundleOperation = spaceSkillOperationUpdate

// WithSpaceSkillDecisionVerifier wires the Control decision verifier for bundle writes to a Space skill (P2-go-15-rel1).
// It takes effect only while SKILL_SPACE_SCOPE and SKILL_SPACE_WRITE_VERIFY are both the literal "true"; nil keeps every
// such write refused, as the skill routes refuse a Space skill they cannot verify.
func (h *SkillBundlesHandler) WithSpaceSkillDecisionVerifier(verifier *SpaceSkillDecisionVerifier) *SkillBundlesHandler {
	h.spaceSkillVerifier = verifier
	return h
}

// spaceSkillBundleDigest is the request_digest a bundle write's decision binds: "sha256:" and the lowercase hex of the
// body as received. A bundle is a zip, not JSON, so the body is not canonicalised. A delete's body is empty, so its digest
// is that of zero bytes, as a skill delete's is.
func spaceSkillBundleDigest(body []byte) string {
	sum := sha256.Sum256(body)
	return spaceSkillDigestPrefix + hex.EncodeToString(sum[:])
}

// spaceSkillBundleDecided reports whether bundle writes to a Space skill are decided by a verified decision: both
// switches on, read when the handler is built. Otherwise no Space check runs and every bundle write is as it was.
func (h *SkillBundlesHandler) spaceSkillBundleDecided() bool {
	return h.spaceScope && h.spaceWriteVerify
}

// verifySpaceSkillBundleWrite refuses a bundle write to a Space skill unless a verified decision binds it: this operation,
// the Space the stored row names, this skill, the org and person of the verified principal, and the body digest. A skill
// of another scope, or no skill row, is not a Space skill and passes as before. It runs after the body's own checks and
// just before the write, so a refused body does not use the decision up.
func (h *SkillBundlesHandler) verifySpaceSkillBundleWrite(r *http.Request, principal authctx.Principal, skillID, operation string, body []byte) error {
	if !h.spaceSkillBundleDecided() {
		return nil
	}
	var scope, spaceRef string
	err := h.db.QueryRow(r.Context(), storedSkillScopeSQL, skillID, principal.OrganizationID).Scan(&scope, &spaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if scope != spaceSkillScope {
		return nil
	}
	if h.spaceSkillVerifier == nil {
		return errSpaceSkillDecisionRequired
	}
	token := strings.TrimSpace(r.Header.Get(spaceSkillDecisionHeader))
	if token == "" {
		return errSpaceSkillDecisionMissing
	}
	if principal.PrincipalType != "user" || principal.OrganizationID == "" || principal.ActorID == "" {
		return errSpaceSkillDecisionInvalid
	}
	return h.spaceSkillVerifier.Verify(token, SkillWriteIntent{
		OrgID: principal.OrganizationID, SpaceRef: spaceRef, SubjectID: principal.ActorID,
		Operation: operation, SkillID: skillID, RequestDigest: spaceSkillBundleDigest(body),
	})
}
