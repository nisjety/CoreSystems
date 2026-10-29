package activities

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

const (
	controlScheduledStepDecisionVersion     = "v2"
	controlScheduledStepAction              = "model.schedule.step"
	controlScheduledStepAudience            = "model-plane-execution-core"
	controlScheduledStepSchema              = "sha256:space-scheduled-step-v1"
	controlScheduledStepExecutorPrincipal   = "orchestrator-core"
	maxControlScheduledStepDecisionResponse = 64 << 10
)

// ScheduledStepExecutionIntent is safe to persist in Temporal history. It
// contains only immutable identifiers and digests; the signed decision is
// obtained afresh for each activity and never enters workflow input/history.
type ScheduledStepExecutionIntent struct {
	OrgID          string
	SpaceRef       string
	SubjectID      string
	RunID          string
	ThreadID       string
	ScheduleID     string
	FireKey        string
	TemplateDigest string
	StepID         string
	StepIndex      uint32
	PolicyDigest   string
	IdempotencyKey string
}

func (i ScheduledStepExecutionIntent) validate() error {
	for label, value := range map[string]string{
		"org_id": i.OrgID, "space_ref": i.SpaceRef, "subject_id": i.SubjectID,
		"run_id": i.RunID, "thread_id": i.ThreadID, "schedule_id": i.ScheduleID,
		"fire_key": i.FireKey, "template_digest": i.TemplateDigest,
		"step_id": i.StepID, "policy_digest": i.PolicyDigest,
		"idempotency_key": i.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("scheduled step %s is required", label)
		}
	}
	if strings.ContainsAny(i.ScheduleID+i.FireKey, "/.>* \t\r\n") {
		return fmt.Errorf("scheduled step identifiers are invalid")
	}
	for label, digest := range map[string]string{"template_digest": i.TemplateDigest, "policy_digest": i.PolicyDigest} {
		if !strings.HasPrefix(digest, "sha256:") || len(digest) != len("sha256:")+64 {
			return fmt.Errorf("scheduled step %s is invalid", label)
		}
	}
	return nil
}

type ScheduledStepDecisionAuthorizer interface {
	AuthorizeScheduledStep(context.Context, ScheduledStepExecutionIntent) (string, error)
}

type ControlScheduledStepAuthorizer struct {
	endpoint string
	token    string
	keyID    string
	public   ed25519.PublicKey
	client   *http.Client
}

func NewControlScheduledStepAuthorizer(endpoint, token, keyID, publicKeyBase64 string, client *http.Client) (*ControlScheduledStepAuthorizer, error) {
	endpoint = strings.TrimRight(strings.TrimSpace(endpoint), "/")
	token = strings.TrimSpace(token)
	keyID = strings.TrimSpace(keyID)
	rawKey, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(publicKeyBase64))
	if endpoint == "" || token == "" || keyID == "" || err != nil || len(rawKey) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("Control scheduled-step endpoint, credential, key ID, and public key are required")
	}
	if client == nil {
		client = &http.Client{Timeout: 5 * time.Second}
	}
	return &ControlScheduledStepAuthorizer{endpoint: endpoint, token: token, keyID: keyID, public: ed25519.PublicKey(rawKey), client: client}, nil
}

func (a *ControlScheduledStepAuthorizer) AuthorizeScheduledStep(ctx context.Context, intent ScheduledStepExecutionIntent) (string, error) {
	if a == nil {
		return "", fmt.Errorf("Control scheduled-step authorizer is not configured")
	}
	if err := intent.validate(); err != nil {
		return "", err
	}
	body, err := json.Marshal(map[string]any{"intent": map[string]any{
		"org_id": intent.OrgID, "space_ref": intent.SpaceRef, "subject_id": intent.SubjectID,
		"run_id": intent.RunID, "thread_id": intent.ThreadID, "schedule_id": intent.ScheduleID,
		"fire_key": intent.FireKey, "template_digest": intent.TemplateDigest,
		"step_id": intent.StepID, "step_index": intent.StepIndex,
		"policy_digest": intent.PolicyDigest, "idempotency_key": intent.IdempotencyKey,
	}})
	if err != nil {
		return "", fmt.Errorf("encode scheduled-step intent: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, a.endpoint+"/api/v1/internal/spaces/scheduled-step-decision", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("build scheduled-step decision request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Id", controlScheduledStepExecutorPrincipal)
	request.Header.Set("X-Service-Token", a.token)
	response, err := a.client.Do(request)
	if err != nil {
		return "", fmt.Errorf("request scheduled-step decision: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return "", fmt.Errorf("Control scheduled-step authority rejected: status %d", response.StatusCode)
	}
	var envelope struct {
		Data struct {
			Token string `json:"token"`
		} `json:"data"`
	}
	if err := json.NewDecoder(io.LimitReader(response.Body, maxControlScheduledStepDecisionResponse)).Decode(&envelope); err != nil {
		return "", fmt.Errorf("decode scheduled-step authority: %w", err)
	}
	if err := a.verify(envelope.Data.Token, intent, time.Now().UTC()); err != nil {
		return "", err
	}
	return envelope.Data.Token, nil
}

type scheduledStepDecision struct {
	DecisionRef               string    `json:"decision_ref"`
	OrgID                     string    `json:"org_id"`
	SpaceRef                  string    `json:"space_ref"`
	SubjectID                 string    `json:"subject_id"`
	ServiceAudience           string    `json:"service_audience"`
	ActionID                  string    `json:"action_id"`
	ActionSchemaHash          string    `json:"action_schema_hash"`
	PayloadDigest             string    `json:"payload_digest"`
	IdempotencyKey            string    `json:"idempotency_key"`
	RecipientAudienceRef      string    `json:"recipient_audience_ref"`
	RecipientAudienceHash     string    `json:"recipient_audience_hash"`
	PrivacyPolicyRef          string    `json:"privacy_policy_ref"`
	ResourceAuthorizationRef  string    `json:"resource_authorization_ref"`
	AuthorityRevision         int64     `json:"authority_revision"`
	MembershipRevision        int64     `json:"membership_revision"`
	PrivacyRevision           int64     `json:"privacy_revision"`
	RecipientAudienceRevision int64     `json:"recipient_audience_revision"`
	EntitlementRevision       int64     `json:"entitlement_revision"`
	Permissions               []string  `json:"permissions"`
	ZeroDataRetention         bool      `json:"zero_data_retention"`
	Nonce                     string    `json:"nonce"`
	IssuedAt                  time.Time `json:"issued_at"`
	ExpiresAt                 time.Time `json:"expires_at"`
}

func (a *ControlScheduledStepAuthorizer) verify(token string, intent ScheduledStepExecutionIntent, now time.Time) error {
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != controlScheduledStepDecisionVersion {
		return fmt.Errorf("invalid Control scheduled-step decision envelope")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != a.keyID {
		return fmt.Errorf("untrusted Control scheduled-step decision key")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || !ed25519.Verify(a.public, []byte(strings.Join(parts[:3], ".")), signature) {
		return fmt.Errorf("invalid Control scheduled-step decision signature")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return fmt.Errorf("invalid Control scheduled-step decision payload")
	}
	var decision scheduledStepDecision
	if err := json.Unmarshal(payload, &decision); err != nil {
		return fmt.Errorf("invalid Control scheduled-step decision claims")
	}
	if decision.OrgID != intent.OrgID || decision.SpaceRef != intent.SpaceRef || decision.SubjectID != intent.SubjectID ||
		decision.ServiceAudience != controlScheduledStepAudience || decision.ActionID != controlScheduledStepAction ||
		decision.ActionSchemaHash != controlScheduledStepSchema || decision.IdempotencyKey != intent.IdempotencyKey ||
		decision.PayloadDigest != scheduledStepPayloadDigest(decision, intent) || decision.ZeroDataRetention ||
		strings.TrimSpace(decision.DecisionRef) == "" || strings.TrimSpace(decision.Nonce) == "" ||
		!hasScheduledStepPermission(decision.Permissions) || !now.Before(decision.ExpiresAt) || decision.IssuedAt.After(now.Add(time.Minute)) {
		return fmt.Errorf("Control decision does not authorize this scheduled step")
	}
	return nil
}

func scheduledStepPayloadDigest(decision scheduledStepDecision, intent ScheduledStepExecutionIntent) string {
	hash := sha256.New()
	hash.Write([]byte(controlScheduledStepAction + "\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", decision.OrgID}, {"user_id", decision.SubjectID}, {"space_id", decision.SpaceRef},
		{"run_id", intent.RunID}, {"thread_id", intent.ThreadID}, {"schedule_id", intent.ScheduleID},
		{"fire_key", intent.FireKey}, {"template_digest", intent.TemplateDigest}, {"step_id", intent.StepID},
		{"policy_digest", intent.PolicyDigest}, {"idempotency_key", intent.IdempotencyKey},
		{"recipient_audience_ref", decision.RecipientAudienceRef}, {"recipient_audience_hash", decision.RecipientAudienceHash},
		{"privacy_policy_ref", decision.PrivacyPolicyRef}, {"resource_authorization_ref", decision.ResourceAuthorizationRef},
		{"action_schema_hash", controlScheduledStepSchema},
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
		{"step_index", int64(intent.StepIndex)}, {"authority_revision", decision.AuthorityRevision},
		{"membership_revision", decision.MembershipRevision}, {"privacy_revision", decision.PrivacyRevision},
		{"recipient_audience_revision", decision.RecipientAudienceRevision}, {"entitlement_revision", decision.EntitlementRevision},
	} {
		hash.Write([]byte(revision.name))
		hash.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		hash.Write(encoded[:])
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

func hasScheduledStepPermission(permissions []string) bool {
	for _, permission := range permissions {
		if permission == "schedule:step" {
			return true
		}
	}
	return false
}
