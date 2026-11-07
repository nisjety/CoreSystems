package api

import (
	"errors"
	"net/http"
	"os"
)

// Space skills (EP-11 S9). A skill of scope "space" steers only the turns of threads inside one
// Space; session-core lists it only for a thread it resolves to that Space itself. Who may write
// one is a Space decision, not an org role: capability-core must not trust a Space role the
// client or the BFF asserts (EP-11 D7). Control has no decision kind for "Space skill write"
// yet, so every write that would create, change or delete a Space skill is refused here with
// a typed code until one exists and a verifier for it is wired.

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
