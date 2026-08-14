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
	scheduledRunAction   = "model.schedule.run"
	scheduledRunAudience = "model-plane-capability-core"
	scheduledRunSchema   = "sha256:space-scheduled-run-v1"
)

// ScheduledRunIntent is the immutable, non-secret owner effect Capability Core
// presents after claiming one fire. TaskID is also the Model run id.
type ScheduledRunIntent struct {
	OrgID          string
	SpaceRef       string
	SubjectID      string
	ScheduleID     string
	FireKey        string
	TaskID         string
	TemplateDigest string
	IdempotencyKey string
}

func (i ScheduledRunIntent) SystemThreadKey() string {
	return "schedule/" + strings.TrimSpace(i.ScheduleID) + "/" + strings.TrimSpace(i.FireKey)
}

func (i ScheduledRunIntent) Validate() error {
	for label, value := range map[string]string{
		"org_id": i.OrgID, "space_ref": i.SpaceRef, "subject_id": i.SubjectID,
		"schedule_id": i.ScheduleID, "fire_key": i.FireKey, "task_id": i.TaskID,
		"template_digest": i.TemplateDigest, "idempotency_key": i.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("scheduled run intent %s is required", label)
		}
	}
	if strings.Contains(i.ScheduleID, "/") || strings.Contains(i.FireKey, "/") {
		return fmt.Errorf("scheduled run identifiers cannot contain '/'")
	}
	if !strings.HasPrefix(i.TemplateDigest, "sha256:") || len(i.TemplateDigest) != len("sha256:")+64 {
		return fmt.Errorf("scheduled run template digest is invalid")
	}
	return nil
}

func scheduledRunPayloadDigest(evidence PersonalThreadDecisionEvidence, intent ScheduledRunIntent) string {
	hash := sha256.New()
	hash.Write([]byte("model.schedule.run\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef}, {"schedule_id", intent.ScheduleID},
		{"fire_key", intent.FireKey}, {"run_id", intent.TaskID}, {"system_thread_key", intent.SystemThreadKey()},
		{"template_digest", intent.TemplateDigest}, {"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash}, {"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef}, {"action_schema_hash", scheduledRunSchema},
		{"idempotency_key", intent.IdempotencyKey},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	for _, revision := range []struct {
		name  string
		value int64
	}{
		{"authority_revision", evidence.Membership.Revisions.Authority},
		{"membership_revision", evidence.Membership.Revisions.Membership},
		{"privacy_revision", evidence.Membership.Revisions.Privacy},
		{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
		{"entitlement_revision", evidence.Membership.Revisions.Entitlement},
	} {
		hash.Write([]byte(revision.name))
		hash.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		hash.Write(encoded[:])
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueScheduledRunDecision authorizes preparation of one service-owned run
// thread. It is not user delegation and does not authorize any provider effect.
func IssueScheduledRunDecision(evidence PersonalThreadDecisionEvidence, intent ScheduledRunIntent, decisionRef, nonce string, now time.Time) (Decision, error) {
	if err := evidence.validatePersonalAuthority(); err != nil {
		return Decision{}, err
	}
	if !matchesOneOf(evidence.Membership.Role, "editor", "manager", "owner") {
		return Decision{}, fmt.Errorf("Space role cannot run a schedule")
	}
	if !evidence.ScheduleFireEntitled {
		return Decision{}, fmt.Errorf("schedule fire entitlement is not active")
	}
	if evidence.Privacy.ZeroDataRetention {
		return Decision{}, fmt.Errorf("zero data retention forbids durable scheduled runs")
	}
	if err := intent.Validate(); err != nil {
		return Decision{}, err
	}
	if intent.OrgID != evidence.Membership.OrgID || intent.SpaceRef != evidence.Membership.SpaceRef || intent.SubjectID != evidence.Membership.SubjectID {
		return Decision{}, fmt.Errorf("scheduled run intent does not match current authority")
	}
	if strings.TrimSpace(decisionRef) == "" || strings.TrimSpace(nonce) == "" || now.IsZero() {
		return Decision{}, fmt.Errorf("scheduled run decision fields are required")
	}
	return Decision{
		DecisionRef: strings.TrimSpace(decisionRef), OrgID: evidence.Membership.OrgID, SpaceRef: evidence.Membership.SpaceRef,
		SubjectID: evidence.Membership.SubjectID, ServiceAudience: scheduledRunAudience, ActionID: scheduledRunAction,
		ActionSchemaHash: scheduledRunSchema, PayloadDigest: scheduledRunPayloadDigest(evidence, intent), IdempotencyKey: strings.TrimSpace(intent.IdempotencyKey),
		RecipientAudienceRef: strings.TrimSpace(evidence.RecipientAudienceRef), RecipientAudienceHash: strings.TrimSpace(evidence.RecipientAudienceHash),
		PrivacyPolicyRef: strings.TrimSpace(evidence.Privacy.PolicyRef), ResourceAuthorizationRef: strings.TrimSpace(evidence.ResourceAuthorizationRef),
		AuthorityRevision: evidence.Membership.Revisions.Authority, MembershipRevision: evidence.Membership.Revisions.Membership,
		PrivacyRevision: evidence.Membership.Revisions.Privacy, RecipientAudienceRevision: evidence.Membership.Revisions.RecipientAudience,
		EntitlementRevision: evidence.Membership.Revisions.Entitlement, Permissions: []string{"schedule:run"},
		Purpose: strings.TrimSpace(evidence.Privacy.Purpose), LawfulBasis: strings.TrimSpace(evidence.Privacy.LawfulBasis),
		PrivacyClass: strings.TrimSpace(evidence.Privacy.PrivacyClass), ThirdPartyAllowed: evidence.Privacy.ThirdPartyAllowed,
		RetentionClass: strings.TrimSpace(evidence.Privacy.RetentionClass), Residency: strings.TrimSpace(evidence.Privacy.Residency),
		DeletionScope: strings.TrimSpace(evidence.Privacy.DeletionScope), ZeroDataRetention: false,
		IssuedAt: now.UTC(), ExpiresAt: now.UTC().Add(personalDecisionLifetime), Nonce: strings.TrimSpace(nonce),
	}, nil
}
