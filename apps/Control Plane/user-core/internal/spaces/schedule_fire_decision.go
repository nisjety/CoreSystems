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
	scheduleFireAction   = "model.cron.fire"
	scheduleFireAudience = "model-plane-capability-core"
	scheduleFireSchema   = "sha256:space-cron-fire-v1"
)

// ScheduleFireIntent is the non-secret, immutable schedule record view that a
// scheduler presents to Control immediately before one fire. It deliberately
// excludes any prior decision and mutable authority data: Control re-resolves
// those from its own store on every attempt.
type ScheduleFireIntent struct {
	OrgID          string
	SpaceRef       string
	SubjectID      string
	ScheduleID     string
	FireKey        string
	TemplateDigest string
	IdempotencyKey string
}

func (i ScheduleFireIntent) Validate() error {
	for label, value := range map[string]string{
		"org_id": i.OrgID, "space_ref": i.SpaceRef, "subject_id": i.SubjectID,
		"schedule_id": i.ScheduleID, "fire_key": i.FireKey,
		"template_digest": i.TemplateDigest, "idempotency_key": i.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("schedule fire intent %s is required", label)
		}
	}
	if !strings.HasPrefix(i.TemplateDigest, "sha256:") || len(i.TemplateDigest) != len("sha256:")+64 {
		return fmt.Errorf("schedule fire intent template digest is invalid")
	}
	return nil
}

// scheduleFirePayloadDigest commits a decision to precisely one claimed fire
// of precisely one durable schedule. It is length-prefixed to retain the
// collision resistance of the newer thread append contract.
func scheduleFirePayloadDigest(evidence PersonalThreadDecisionEvidence, intent ScheduleFireIntent) string {
	hash := sha256.New()
	hash.Write([]byte("model.cron.fire\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef}, {"schedule_id", intent.ScheduleID},
		{"fire_key", intent.FireKey}, {"template_digest", intent.TemplateDigest},
		{"recipient_audience_ref", evidence.RecipientAudienceRef}, {"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef}, {"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"action_schema_hash", scheduleFireSchema}, {"idempotency_key", intent.IdempotencyKey},
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

// IssueScheduleFireDecision is a fresh, one-fire Model Plane decision. A
// schedule creator's original request never serves as a standing worker
// grant: membership, privacy, audience, resource access, and the distinct
// schedule-fire entitlement are all checked again at fire time.
func IssueScheduleFireDecision(
	evidence PersonalThreadDecisionEvidence,
	intent ScheduleFireIntent,
	decisionRef string,
	nonce string,
	now time.Time,
) (Decision, error) {
	if err := evidence.validatePersonalAuthority(); err != nil {
		return Decision{}, err
	}
	if !matchesOneOf(evidence.Membership.Role, "editor", "manager", "owner") {
		return Decision{}, fmt.Errorf("space role %q cannot fire a schedule", evidence.Membership.Role)
	}
	if !evidence.ScheduleFireEntitled {
		return Decision{}, fmt.Errorf("schedule fire entitlement is not active")
	}
	if evidence.Privacy.ZeroDataRetention {
		return Decision{}, fmt.Errorf("zero data retention forbids durable schedule fire")
	}
	if err := intent.Validate(); err != nil {
		return Decision{}, err
	}
	if intent.OrgID != evidence.Membership.OrgID || intent.SpaceRef != evidence.Membership.SpaceRef || intent.SubjectID != evidence.Membership.SubjectID {
		return Decision{}, fmt.Errorf("schedule fire intent does not match current authority")
	}
	if strings.TrimSpace(decisionRef) == "" || strings.TrimSpace(nonce) == "" || now.IsZero() {
		return Decision{}, fmt.Errorf("schedule fire decision fields are required")
	}
	return newEvidenceDecision(
		evidence, decisionRef, scheduleFireAudience, scheduleFireAction, scheduleFireSchema,
		scheduleFirePayloadDigest(evidence, intent), intent.IdempotencyKey, nonce,
		[]string{"cron:fire"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
