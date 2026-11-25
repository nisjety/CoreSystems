package api

import (
	"errors"
	"net/http"
	"os"
	"strings"

	"github.com/triodelab/model-plane/pkg/authctx"
)

// Space skills (EP-11 S9). A skill of scope "space" steers only the turns of threads inside one
// Space; session-core lists it only for a thread it resolves to that Space itself. Who may write
// one is a Space decision, not an org role: capability-core must not trust a Space role the
// client or the BFF asserts (EP-11 D7). Control signs a model.space.skill.write decision for it
// (F-f10-18); while SKILL_SPACE_WRITE_VERIFY is off or no verifier is configured, every write that
// would create, change or delete a Space skill is refused here with a typed code, as before. On,
// a write is made only under a decision that verifies for it (F-f10-19, space_skill_decision_verifier.go).

const (
	spaceSkillScope    = "space"
	skillSpaceScopeEnv = "SKILL_SPACE_SCOPE"

	// spaceSkillDecisionRequiredCode is the typed refusal of a Space skill write.
	spaceSkillDecisionRequiredCode = "space_skill_decision_required"
)

// errSpaceSkillDecisionRequired ends a change to a Space skill inside its transaction, before its
// statement, so nothing is written.
var errSpaceSkillDecisionRequired = errors.New("a Space skill changes only under a Control-signed Space decision")

// SkillSpaceScopeFromLookup reads SKILL_SPACE_SCOPE strictly: only the literal "true" turns the
// Space scope on. Off, a body naming scope "space" is an invalid scope, as it always was.
func SkillSpaceScopeFromLookup(lookup func(string) string) bool {
	return lookup(skillSpaceScopeEnv) == "true"
}

// skillSpaceScopeEnabled is read where it is used, as the version switch is in session-core,
// so a create needs no handler field.
func skillSpaceScopeEnabled() bool {
	return SkillSpaceScopeFromLookup(os.Getenv)
}

// writeSpaceSkillDecisionRequired answers a Space skill write with the typed envelope. A 403:
// the caller's org role is not what decides it.
func writeSpaceSkillDecisionRequired(w http.ResponseWriter) {
	writeTypedError(w, http.StatusForbidden, spaceSkillDecisionRequiredCode,
		"a Space skill can be written only with a Control-signed Space decision, which this deployment cannot verify yet")
}

// errSpaceSkillDecisionMissing is a Space skill write with verification on but no decision header.
var errSpaceSkillDecisionMissing = errors.New("a Space skill write needs a Control-signed Space decision")

// WithSpaceSkillDecisionVerifier wires the Control decision verifier (F-f10-19). It takes effect only
// while SKILL_SPACE_SCOPE and SKILL_SPACE_WRITE_VERIFY are both the literal "true"; nil keeps every
// Space skill write refused.
func (h *SkillsHandler) WithSpaceSkillDecisionVerifier(verifier *SpaceSkillDecisionVerifier) *SkillsHandler {
	h.spaceSkillVerifier = verifier
	return h
}

// spaceSkillWritesVerified reports whether a Space skill write is decided by a verified decision:
// SKILL_SPACE_SCOPE and SKILL_SPACE_WRITE_VERIFY on and a verifier configured. Otherwise every such
// write answers space_skill_decision_required exactly as before.
func (h *SkillsHandler) spaceSkillWritesVerified() bool {
	return h.spaceWriteVerify && h.spaceSkillVerifier != nil && skillSpaceScopeEnabled()
}

// verifySpaceSkillWrite checks the decision in X-Space-Skill-Decision against the write about to be
// made. The org and the person come from the verified principal only, and only a person may author;
// the Space, operation, skill id and body come from the caller of this function. The header is never
// logged.
func (h *SkillsHandler) verifySpaceSkillWrite(r *http.Request, operation, spaceRef, skillID string, body []byte) error {
	token := strings.TrimSpace(r.Header.Get(spaceSkillDecisionHeader))
	if token == "" {
		return errSpaceSkillDecisionMissing
	}
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.PrincipalType != "user" || principal.OrganizationID == "" || principal.ActorID == "" {
		return errSpaceSkillDecisionInvalid
	}
	digest, err := spaceSkillRequestDigest(operation, body)
	if err != nil {
		return errSpaceSkillDecisionInvalid
	}
	return h.spaceSkillVerifier.Verify(token, SkillWriteIntent{
		OrgID: principal.OrganizationID, SpaceRef: spaceRef, SubjectID: principal.ActorID,
		Operation: operation, SkillID: skillID, RequestDigest: digest,
	})
}

// spaceSkillInsertSQL and spaceSkillInsertWithKindSQL are the create's insert with scope_id, used only
// for a Space skill whose decision verified; every other create keeps its statement unchanged.
const (
	spaceSkillInsertSQL = `
				INSERT INTO agent_skills (id, org_id, name, description, content,
				    trigger_keywords, trigger_file_patterns, tool_restrictions,
				    enabled, scope, owner_user_id, shared_with, created_at, updated_at, scope_id)
				VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
			`
	spaceSkillInsertWithKindSQL = `
					INSERT INTO agent_skills (id, org_id, name, description, content,
					    trigger_keywords, trigger_file_patterns, tool_restrictions,
					    enabled, scope, owner_user_id, shared_with, created_at, updated_at,
					    kind, settings, scope_id)
					VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)
				`
)

// storedSpaceSkillRefSQL reads the Space a stored Space skill belongs to. It runs only inside a
// change's transaction, after ensureSkillCallerMayChange has locked the row and found scope 'space'.
const storedSpaceSkillRefSQL = `SELECT scope_id FROM agent_skills WHERE id=$1 AND org_id=$2`

// authorizeStoredSpaceSkillChange verifies a change or delete of a stored Space skill against the
// Space the row names, never one the request claims.
func (h *SkillsHandler) authorizeStoredSpaceSkillChange(r *http.Request, tx registryDatabase, id, orgID, operation string, body []byte) error {
	var spaceRef string
	if err := tx.QueryRow(r.Context(), storedSpaceSkillRefSQL, id, orgID).Scan(&spaceRef); err != nil {
		return err
	}
	return h.verifySpaceSkillWrite(r, operation, spaceRef, id, body)
}

// writeSpaceSkillDecisionError answers a refused Space skill write and reports whether err was one.
// Off, only errSpaceSkillDecisionRequired can occur, and it answers exactly as before.
func writeSpaceSkillDecisionError(w http.ResponseWriter, err error) bool {
	switch {
	case errors.Is(err, errSpaceSkillDecisionRequired):
		writeSpaceSkillDecisionRequired(w)
	case errors.Is(err, errSpaceSkillDecisionMissing):
		writeTypedError(w, http.StatusForbidden, spaceSkillDecisionRequiredCode,
			"a Space skill can be written only with a Control-signed Space decision in "+spaceSkillDecisionHeader)
	case errors.Is(err, errSpaceSkillDecisionInvalid):
		writeTypedError(w, http.StatusForbidden, spaceSkillDecisionInvalidCode,
			"the Space skill decision does not authorize this write")
	case errors.Is(err, errSpaceSkillDecisionReplayed):
		writeTypedError(w, http.StatusConflict, spaceSkillDecisionReplayedCode,
			"the Space skill decision was already used; ask for a new one")
	case errors.Is(err, errSpaceSkillDecisionZDR):
		writeSkillAuthoringZDRRefusal(w)
	default:
		return false
	}
	return true
}
