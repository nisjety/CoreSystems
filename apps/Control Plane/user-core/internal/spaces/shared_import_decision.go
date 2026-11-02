package spaces

import (
	"errors"
	"fmt"
	"strings"
	"time"
)

// WS14-S1: shared-Space import decisions. They use the SAME action id
// (ingestion.import.write), schema (sha256:ingestion-import-v1) and payload
// digest function as the personal import decision, so Imports Core,
// Documents API and the Go/Python/Rust verifiers need no change. What differs
// is only who may obtain one: a current editor-or-above member of a
// non-personal Space inside the current recipient audience, in an org with the
// import entitlement on and Zero Data Retention off, for the one source type a
// shared room accepts.

// ErrSharedImportSourceType is returned when a shared Space import names a
// source type other than the room-upload type. Connector imports into a room
// stay refused (a later item, not this slice).
var ErrSharedImportSourceType = errors.New("shared Space import accepts only room uploads")

// sharedImportSourceTypes is the allow-list of source types a shared Space
// import decision may carry.
var sharedImportSourceTypes = map[string]struct{}{
	SpaceUploadSourceType: {},
}

// SharedImportSourceTypeAllowed reports whether a shared Space import decision
// may carry this source type.
func SharedImportSourceTypeAllowed(value string) bool {
	_, ok := sharedImportSourceTypes[strings.TrimSpace(value)]
	return ok
}

// IssueSharedImportDecision emits short-lived authority for one Ingestion
// import initiation in a shared Space. Like the personal issuer it accepts
// only retry identity and a source type; the actor, audience, privacy floor
// and resource reference all come from the resolved evidence.
func IssueSharedImportDecision(
	evidence PersonalThreadDecisionEvidence,
	request PersonalImportDecisionRequest,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForSharedImport(); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if !validImportSourceType(request.SourceType) {
		return Decision{}, fmt.Errorf("shared import decision source_type is invalid")
	}
	if !SharedImportSourceTypeAllowed(request.SourceType) {
		return Decision{}, ErrSharedImportSourceType
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("shared import decision issuance time is required")
	}
	decision := newEvidenceDecision(
		evidence, request.DecisionRef, personalImportAudience, personalImportAction, personalImportSchema,
		personalImportPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"ingestion:import"}, evidence.Privacy.ZeroDataRetention, now,
	)
	decision.ImportSourceType = strings.TrimSpace(request.SourceType)
	return decision, nil
}

// IssueSharedImportExecutionDecision is the Data-targeted re-authorization the
// import worker asks for immediately before a durable write. It refreshes
// current Control evidence but keeps the original intent's payload binding.
func IssueSharedImportExecutionDecision(
	evidence PersonalThreadDecisionEvidence,
	intent PersonalImportExecutionIntent,
	decisionRef string,
	nonce string,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForSharedImport(); err != nil {
		return Decision{}, err
	}
	if err := intent.Validate(); err != nil {
		return Decision{}, err
	}
	if !SharedImportSourceTypeAllowed(intent.SourceType) {
		return Decision{}, ErrSharedImportSourceType
	}
	if intent.OrgID != evidence.Membership.OrgID || intent.SpaceRef != evidence.Membership.SpaceRef || intent.SubjectID != evidence.Membership.SubjectID {
		return Decision{}, fmt.Errorf("shared import execution intent does not match current authority")
	}
	if strings.TrimSpace(decisionRef) == "" || strings.TrimSpace(nonce) == "" || now.IsZero() {
		return Decision{}, fmt.Errorf("shared import execution decision fields are required")
	}
	decision := newEvidenceDecision(
		evidence, decisionRef, personalImportExecutionAudience, personalImportAction, personalImportSchema,
		intent.PayloadDigest, intent.IdempotencyKey, nonce,
		[]string{"documents:write"}, evidence.Privacy.ZeroDataRetention, now,
	)
	decision.ImportSourceType = strings.TrimSpace(intent.SourceType)
	return decision, nil
}
