package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

const (
	scheduleCreateAction   = "model.cron.create"
	scheduleCreateAudience = "model-plane-capability-core"
	scheduleCreateSchema   = "sha256:space-cron-create-v1"
	// scheduleCreateContractSchema is the schema once a stored-consent contract is bound (DEC-16).
	scheduleCreateContractSchema = "sha256:space-cron-create-v2"
	scheduleContractMaxClass     = "read"
	// scheduleContractMaxDays is the longest contract the Control decision will sign.
	scheduleContractMaxDays = 90
	scheduleContractTime    = "2006-01-02T15:04:05Z"
)

// ScheduleCreateRequest binds Control's authorization to the schedule ID and
// a content-free digest of the exact task template that Capability Core will
// persist. Control therefore never needs the template body to enforce Space
// policy, while the Model Plane cannot swap in another job after approval.
//
// A stored-consent contract adds its digest and expiry, both signed into the
// payload digest. ContractsEnabled is server configuration (SCHEDULE_CONTRACT_ENABLED),
// never a browser field; off, a request carrying a contract is refused.
type ScheduleCreateRequest struct {
	DecisionRef       string
	ScheduleID        string
	TemplateDigest    string
	IdempotencyKey    string
	Nonce             string
	ContractDigest    string
	ContractExpiresAt string
	ContractsEnabled  bool
}

// scheduleCreateSchemaFor is the action schema Control signs for a request.
func scheduleCreateSchemaFor(request ScheduleCreateRequest) string {
	if request.ContractDigest != "" {
		return scheduleCreateContractSchema
	}
	return scheduleCreateSchema
}

// validateContract checks the contract fields Control signs: the digest shape,
// the canonical UTC expiry, and that the expiry is in the future and within the
// maximum window. The contract body is verified by capability-core, which
// recomputes the digest from the stored template.
func (r ScheduleCreateRequest) validateContract(now time.Time) error {
	if r.ContractDigest == "" && r.ContractExpiresAt == "" {
		return nil
	}
	if !r.ContractsEnabled {
		return fmt.Errorf("schedule contracts are not enabled")
	}
	if !strings.HasPrefix(r.ContractDigest, "sha256:") || len(r.ContractDigest) != len("sha256:")+64 {
		return fmt.Errorf("schedule contract digest is invalid")
	}
	expires, err := time.Parse(scheduleContractTime, r.ContractExpiresAt)
	if err != nil || expires.UTC().Format(scheduleContractTime) != r.ContractExpiresAt {
		return fmt.Errorf("schedule contract expiry must be a canonical UTC time")
	}
	if !expires.After(now) {
		return fmt.Errorf("schedule contract expiry must be in the future")
	}
	if expires.After(now.AddDate(0, 0, scheduleContractMaxDays)) {
		return fmt.Errorf("schedule contract expiry is beyond the %d day maximum", scheduleContractMaxDays)
	}
	return nil
}

func (r ScheduleCreateRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "schedule_id": r.ScheduleID,
		"template_digest": r.TemplateDigest, "idempotency_key": r.IdempotencyKey, "nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("schedule create decision %s is required", label)
		}
	}
	if !strings.HasPrefix(r.TemplateDigest, "sha256:") || len(r.TemplateDigest) != len("sha256:")+64 {
		return fmt.Errorf("schedule create template digest is invalid")
	}
	return nil
}

func scheduleCreatePayloadDigest(evidence PersonalThreadDecisionEvidence, request ScheduleCreateRequest) string {
	hash := sha256.New()
	hash.Write([]byte("model.cron.create\x00v1\x00"))
	fields := []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef}, {"schedule_id", request.ScheduleID},
		{"template_digest", request.TemplateDigest}, {"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash}, {"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef}, {"action_schema_hash", scheduleCreateSchemaFor(request)},
		{"idempotency_key", request.IdempotencyKey},
	}
	if request.ContractDigest != "" {
		fields = append(fields,
			struct{ name, value string }{"contract_digest", request.ContractDigest},
			struct{ name, value string }{"contract_expires_at", request.ContractExpiresAt},
			struct{ name, value string }{"contract_max_class", scheduleContractMaxClass},
		)
	}
	for _, field := range fields {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	writeDigestRevisions(hash,
		digestRevision{"authority_revision", evidence.Membership.Revisions.Authority},
		digestRevision{"membership_revision", evidence.Membership.Revisions.Membership},
		digestRevision{"privacy_revision", evidence.Membership.Revisions.Privacy},
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
		digestRevision{"entitlement_revision", evidence.Membership.Revisions.Entitlement},
	)
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueScheduleCreateDecision authorizes exactly one durable schedule create.
// It is distinct from schedule fire so a creator token cannot be replayed by a
// scheduler after a role, audience, or policy change.
func IssueScheduleCreateDecision(evidence PersonalThreadDecisionEvidence, request ScheduleCreateRequest, now time.Time) (Decision, error) {
	if err := evidence.validatePersonalAuthority(); err != nil {
		return Decision{}, err
	}
	if !matchesOneOf(evidence.Membership.Role, "editor", "manager", "owner") {
		return Decision{}, fmt.Errorf("space role %q cannot create a schedule", evidence.Membership.Role)
	}
	if !evidence.ScheduleFireEntitled {
		return Decision{}, fmt.Errorf("schedule creation entitlement is not active")
	}
	if evidence.Privacy.ZeroDataRetention {
		return Decision{}, fmt.Errorf("zero data retention forbids durable schedule creation")
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return Decision{}, fmt.Errorf("schedule create decision request is invalid")
	}
	if err := request.validateContract(now); err != nil {
		return Decision{}, err
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, scheduleCreateAudience, scheduleCreateAction, scheduleCreateSchemaFor(request),
		scheduleCreatePayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"cron:create"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
