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
)

// ScheduleCreateRequest binds Control's authorization to the schedule ID and
// a content-free digest of the exact task template that Capability Core will
// persist. Control therefore never needs the template body to enforce Space
// policy, while the Model Plane cannot swap in another job after approval.
type ScheduleCreateRequest struct {
	DecisionRef    string
	ScheduleID     string
	TemplateDigest string
	IdempotencyKey string
	Nonce          string
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
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef}, {"schedule_id", request.ScheduleID},
		{"template_digest", request.TemplateDigest}, {"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash}, {"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef}, {"action_schema_hash", scheduleCreateSchema},
		{"idempotency_key", request.IdempotencyKey},
	} {
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
	return newEvidenceDecision(
		evidence, request.DecisionRef, scheduleCreateAudience, scheduleCreateAction, scheduleCreateSchema,
		scheduleCreatePayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"cron:create"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
