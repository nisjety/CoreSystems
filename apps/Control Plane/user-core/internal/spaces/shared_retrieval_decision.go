package spaces

import (
	"fmt"
	"time"
)

// IssueSharedRetrievalDecision uses the same retrieval effect digest and
// audience as personal retrieval, but only after Control has resolved a
// current shared recipient audience and confirmed retrieval entitlement for
// that Space's org. It deliberately accepts no caller-selected recipient
// fields; those come from the registered audience snapshot in Repository.
func IssueSharedRetrievalDecision(
	evidence PersonalThreadDecisionEvidence,
	request PersonalRetrievalDecisionRequest,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForSharedRetrieval(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("shared retrieval decision issuance time is required")
	}
	// D18: agent retrieval reads room content on the member's behalf, so the member's floor is carried.
	return carryHistoryFloor(newRetrievalReadDecision(evidence, request, now), evidence), nil
}
