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
	scheduledRunAction            = "model.schedule.run"
	scheduledRunAudience          = "model-plane-capability-core"
	scheduledRunSchema            = "sha256:space-scheduled-run-v1"
	scheduledRunExecutionAction   = "model.schedule.execute"
	scheduledRunExecutionAudience = "model-plane-session-core"
	scheduledRunExecutionSchema   = "sha256:space-scheduled-run-execute-v1"
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

func scheduledRunPayloadDigest(action, schema string, evidence PersonalThreadDecisionEvidence, intent ScheduledRunIntent, threadID string) string {
	hash := sha256.New()
	hash.Write([]byte(action + "\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID}, {"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef}, {"schedule_id", intent.ScheduleID},
		{"fire_key", intent.FireKey}, {"run_id", intent.TaskID}, {"system_thread_key", intent.SystemThreadKey()},
		{"template_digest", intent.TemplateDigest}, {"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash}, {"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef}, {"action_schema_hash", schema},
		{"thread_id", threadID},
		{"idempotency_key", intent.IdempotencyKey},
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

// IssueScheduledRunDecision authorizes preparation of one service-owned run
// thread. It is not user delegation and does not authorize any provider effect.
func IssueScheduledRunDecision(evidence PersonalThreadDecisionEvidence, intent ScheduledRunIntent, decisionRef, nonce string, now time.Time) (Decision, error) {
	return issueScheduledRunDecision(
		evidence, intent, decisionRef, nonce, now,
		scheduledRunAction, scheduledRunAudience, scheduledRunSchema, "schedule:run", "",
	)
}

// IssueScheduledRunExecutionDecision re-resolves an owner's active Space
// authority immediately before Orchestrator Core starts a prepared run. It is
// a distinct Session-targeted grant; preparation authority cannot be replayed
// as execution authority.
func IssueScheduledRunExecutionDecision(evidence PersonalThreadDecisionEvidence, intent ScheduledRunIntent, threadID, decisionRef, nonce string, now time.Time) (Decision, error) {
	if strings.TrimSpace(threadID) == "" {
		return Decision{}, fmt.Errorf("scheduled run execution thread is required")
	}
	return issueScheduledRunDecision(
		evidence, intent, decisionRef, nonce, now,
		scheduledRunExecutionAction, scheduledRunExecutionAudience, scheduledRunExecutionSchema, "schedule:execute", strings.TrimSpace(threadID),
	)
}

func issueScheduledRunDecision(
	evidence PersonalThreadDecisionEvidence,
	intent ScheduledRunIntent,
	decisionRef, nonce string,
	now time.Time,
	action, audience, schema, permission, threadID string,
) (Decision, error) {
	if err := evidence.validatePersonalAuthority(); err != nil {
		return Decision{}, err
	}
	if !matchesOneOf(evidence.Membership.Role, "editor", "manager", "owner") {
		return Decision{}, fmt.Errorf("space role cannot run a schedule")
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
	return newEvidenceDecision(
		evidence, decisionRef, audience, action, schema,
		scheduledRunPayloadDigest(action, schema, evidence, intent, threadID), intent.IdempotencyKey, nonce,
		[]string{permission}, false, now,
	), nil
}
