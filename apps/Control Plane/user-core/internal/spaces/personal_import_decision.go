package spaces

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

const (
	personalImportAction            = "ingestion.import.write"
	personalImportAudience          = "ingestion-plane-import"
	personalImportExecutionAudience = "data-plane-import"
	personalImportSchema            = "sha256:ingestion-import-v1"
)

// PersonalImportDecisionRequest is deliberately limited to retry identity.
// Control derives the current actor, recipient audience, privacy floor, and
// owner-resource reference; a caller cannot select a source or document
// target and turn it into durable-write authority.
type PersonalImportDecisionRequest struct {
	DecisionRef    string
	IdempotencyKey string
	SourceType     string
	Nonce          string
}

func (r PersonalImportDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "idempotency_key": r.IdempotencyKey, "source_type": r.SourceType, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("personal import decision %s is required", label)
		}
	}
	return nil
}

func validImportSourceType(value string) bool {
	value = strings.TrimSpace(value)
	if value == "" || len(value) > 64 {
		return false
	}
	for _, character := range value {
		if !(character >= 'a' && character <= 'z') && !(character >= '0' && character <= '9') && character != '-' && character != '_' {
			return false
		}
	}
	return true
}

func personalImportPayloadDigest(evidence PersonalThreadDecisionEvidence, request PersonalImportDecisionRequest) string {
	value := strings.Join([]string{
		personalImportAction, "v1", evidence.Membership.OrgID, evidence.Membership.SubjectID,
		evidence.Membership.SpaceRef, request.DecisionRef, evidence.RecipientAudienceRef,
		evidence.Privacy.PolicyRef, evidence.ResourceAuthorizationRef, personalImportSchema,
		request.IdempotencyKey, request.SourceType,
	}, "\x00")
	sum := sha256.Sum256([]byte(value))
	return "sha256:" + hex.EncodeToString(sum[:])
}

// IssuePersonalImportDecision emits short-lived authority for one Ingestion
// import initiation. It cannot be used as Data retrieval, thread creation, or
// a standing worker credential; downstream job-time reauthorization remains a
// separate owner-bound contract.
func IssuePersonalImportDecision(
	evidence PersonalThreadDecisionEvidence,
	request PersonalImportDecisionRequest,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForImport(); err != nil {
		return Decision{}, err
	}
	if evidence.Privacy.ZeroDataRetention {
		return Decision{}, fmt.Errorf("zero data retention forbids durable import")
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if !validImportSourceType(request.SourceType) {
		return Decision{}, fmt.Errorf("personal import decision source_type is invalid")
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("personal import decision issuance time is required")
	}
	decision := newEvidenceDecision(
		evidence, request.DecisionRef, personalImportAudience, personalImportAction, personalImportSchema,
		personalImportPayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"ingestion:import"}, evidence.Privacy.ZeroDataRetention, now,
	)
	decision.ImportSourceType = strings.TrimSpace(request.SourceType)
	return decision, nil
}

// PersonalImportExecutionIntent is the non-secret immutable information held
// by Imports Core after a gateway-authorized request. Its service credential
// can ask Control to re-resolve current policy immediately before a Data Plane
// write, but cannot change the original subject, scope, or request binding.
type PersonalImportExecutionIntent struct {
	OrgID            string
	SpaceRef         string
	SubjectID        string
	ActionSchemaHash string
	PayloadDigest    string
	IdempotencyKey   string
	SourceType       string
}

func (i PersonalImportExecutionIntent) Validate() error {
	for label, value := range map[string]string{
		"org_id": i.OrgID, "space_ref": i.SpaceRef, "subject_id": i.SubjectID,
		"action_schema_hash": i.ActionSchemaHash, "payload_digest": i.PayloadDigest,
		"idempotency_key": i.IdempotencyKey, "source_type": i.SourceType,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("personal import execution intent %s is required", label)
		}
	}
	if i.ActionSchemaHash != personalImportSchema || !strings.HasPrefix(i.PayloadDigest, "sha256:") {
		return fmt.Errorf("personal import execution intent contract is invalid")
	}
	if !validImportSourceType(i.SourceType) {
		return fmt.Errorf("personal import execution intent source_type is invalid")
	}
	return nil
}

// IssuePersonalImportExecutionDecision is target-bound to Data Plane. It
// refreshes current Control evidence but preserves the durable import intent's
// semantic payload binding, so an expired initial ingress grant is never used
// as a standing worker credential.
func IssuePersonalImportExecutionDecision(
	evidence PersonalThreadDecisionEvidence,
	intent PersonalImportExecutionIntent,
	decisionRef string,
	nonce string,
	now time.Time,
) (Decision, error) {
	if err := evidence.ValidateForImport(); err != nil {
		return Decision{}, err
	}
	if evidence.Privacy.ZeroDataRetention {
		return Decision{}, fmt.Errorf("zero data retention forbids durable import")
	}
	if err := intent.Validate(); err != nil {
		return Decision{}, err
	}
	if intent.OrgID != evidence.Membership.OrgID || intent.SpaceRef != evidence.Membership.SpaceRef || intent.SubjectID != evidence.Membership.SubjectID {
		return Decision{}, fmt.Errorf("personal import execution intent does not match current authority")
	}
	if strings.TrimSpace(decisionRef) == "" || strings.TrimSpace(nonce) == "" || now.IsZero() {
		return Decision{}, fmt.Errorf("personal import execution decision fields are required")
	}
	decision := newEvidenceDecision(
		evidence, decisionRef, personalImportExecutionAudience, personalImportAction, personalImportSchema,
		intent.PayloadDigest, intent.IdempotencyKey, nonce,
		[]string{"documents:write"}, evidence.Privacy.ZeroDataRetention, now,
	)
	decision.ImportSourceType = strings.TrimSpace(intent.SourceType)
	return decision, nil
}
