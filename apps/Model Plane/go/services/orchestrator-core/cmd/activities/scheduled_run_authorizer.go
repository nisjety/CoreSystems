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
	controlScheduledRunDecisionVersion     = "v2"
	controlScheduledRunExecutionAction     = "model.schedule.execute"
	controlScheduledRunExecutionAudience   = "model-plane-session-core"
	controlScheduledRunExecutionSchema     = "sha256:space-scheduled-run-execute-v1"
	controlScheduledRunExecutorPrincipal   = "orchestrator-core"
	maxControlScheduledRunDecisionResponse = 64 << 10
)

// ScheduledRunExecutionIntent is the non-secret, immutable result of the
// preparation decision. It is safe to persist in Temporal so the activity can
// ask Control for fresh authority; the decision bearer itself never enters
// workflow history.
type ScheduledRunExecutionIntent struct {
	OrgID          string
	SpaceRef       string
	SubjectID      string
	ThreadID       string
	ScheduleID     string
	FireKey        string
	RunID          string
	TemplateDigest string
	IdempotencyKey string
}

func (i ScheduledRunExecutionIntent) validate() error {
	for label, value := range map[string]string{
		"org_id": i.OrgID, "space_ref": i.SpaceRef, "subject_id": i.SubjectID,
		"thread_id": i.ThreadID, "schedule_id": i.ScheduleID, "fire_key": i.FireKey,
		"run_id": i.RunID, "template_digest": i.TemplateDigest, "idempotency_key": i.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("scheduled run execution %s is required", label)
		}
	}
	if strings.Contains(i.ScheduleID, "/") || strings.Contains(i.FireKey, "/") {
		return fmt.Errorf("scheduled run execution identifiers cannot contain '/'")
	}
	if !strings.HasPrefix(i.TemplateDigest, "sha256:") || len(i.TemplateDigest) != len("sha256:")+64 {
		return fmt.Errorf("scheduled run execution template digest is invalid")
	}
	return nil
}

// ScheduledRunExecutionAuthorizer obtains the one direct-hop bearer that
// authorizes Session Core to create/reuse the prepared scheduled run.
type ScheduledRunExecutionAuthorizer interface {
	AuthorizeScheduledRunExecution(context.Context, ScheduledRunExecutionIntent) (string, error)
}

// ControlScheduledRunExecutionAuthorizer is the narrow Control client used by
// the Temporal activity immediately before its Session Core effect. A missing
// dependency fails closed; this code has no development key or standing grant.
type ControlScheduledRunExecutionAuthorizer struct {
	endpoint string
	token    string
	keyID    string
	public   ed25519.PublicKey
	client   *http.Client
}

func NewControlScheduledRunExecutionAuthorizer(endpoint, token, keyID, publicKeyBase64 string, client *http.Client) (*ControlScheduledRunExecutionAuthorizer, error) {
	endpoint = strings.TrimRight(strings.TrimSpace(endpoint), "/")
	token = strings.TrimSpace(token)
	keyID = strings.TrimSpace(keyID)
	rawKey, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(publicKeyBase64))
	if endpoint == "" || token == "" || keyID == "" || err != nil || len(rawKey) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("Control scheduled-run execution endpoint, credential, key ID, and public key are required")
	}
	if client == nil {
		client = &http.Client{Timeout: 5 * time.Second}
	}
	return &ControlScheduledRunExecutionAuthorizer{
		endpoint: endpoint, token: token, keyID: keyID, public: ed25519.PublicKey(rawKey), client: client,
	}, nil
}

func (a *ControlScheduledRunExecutionAuthorizer) AuthorizeScheduledRunExecution(ctx context.Context, intent ScheduledRunExecutionIntent) (string, error) {
	if a == nil {
		return "", fmt.Errorf("Control scheduled-run execution authorizer is not configured")
	}
	if err := intent.validate(); err != nil {
		return "", err
	}
	body, err := json.Marshal(map[string]any{"intent": map[string]string{
		"org_id": intent.OrgID, "space_ref": intent.SpaceRef, "subject_id": intent.SubjectID,
		"thread_id": intent.ThreadID, "schedule_id": intent.ScheduleID, "fire_key": intent.FireKey,
		"task_id": intent.RunID, "template_digest": intent.TemplateDigest, "idempotency_key": intent.IdempotencyKey,
	}})
	if err != nil {
		return "", fmt.Errorf("encode scheduled-run execution intent: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, a.endpoint+"/api/v1/internal/spaces/scheduled-run-execution-decision", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("build Control scheduled-run execution request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Id", controlScheduledRunExecutorPrincipal)
	request.Header.Set("X-Service-Token", a.token)
	response, err := a.client.Do(request)
	if err != nil {
		return "", fmt.Errorf("request Control scheduled-run execution authority: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return "", fmt.Errorf("Control scheduled-run execution authority rejected: status %d", response.StatusCode)
	}
	var envelope struct {
		Data struct {
			Token string `json:"token"`
		} `json:"data"`
	}
	if err := json.NewDecoder(io.LimitReader(response.Body, maxControlScheduledRunDecisionResponse)).Decode(&envelope); err != nil {
		return "", fmt.Errorf("decode Control scheduled-run execution authority: %w", err)
	}
	if err := a.verify(envelope.Data.Token, intent, time.Now().UTC()); err != nil {
		return "", err
	}
	return envelope.Data.Token, nil
}

type scheduledRunExecutionDecision struct {
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

func (a *ControlScheduledRunExecutionAuthorizer) verify(token string, intent ScheduledRunExecutionIntent, now time.Time) error {
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != controlScheduledRunDecisionVersion {
		return fmt.Errorf("invalid Control scheduled-run execution decision envelope")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != a.keyID {
		return fmt.Errorf("untrusted Control scheduled-run execution decision key")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || !ed25519.Verify(a.public, []byte(strings.Join(parts[:3], ".")), signature) {
		return fmt.Errorf("invalid Control scheduled-run execution decision signature")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return fmt.Errorf("invalid Control scheduled-run execution decision payload")
	}
	var decision scheduledRunExecutionDecision
	if err := json.Unmarshal(payload, &decision); err != nil {
		return fmt.Errorf("invalid Control scheduled-run execution decision claims")
	}
	if decision.OrgID != intent.OrgID ||
		decision.SpaceRef != intent.SpaceRef ||
		decision.SubjectID != intent.SubjectID ||
		decision.ServiceAudience != controlScheduledRunExecutionAudience ||
		decision.ActionID != controlScheduledRunExecutionAction ||
		decision.ActionSchemaHash != controlScheduledRunExecutionSchema ||
		decision.IdempotencyKey != intent.IdempotencyKey ||
		decision.PayloadDigest != scheduledRunExecutionPayloadDigest(decision, intent) ||
		decision.ZeroDataRetention ||
		strings.TrimSpace(decision.DecisionRef) == "" ||
		strings.TrimSpace(decision.Nonce) == "" ||
		!hasScheduledRunExecutionPermission(decision.Permissions) ||
		!now.Before(decision.ExpiresAt) ||
		decision.IssuedAt.After(now.Add(time.Minute)) {
		return fmt.Errorf("Control decision does not authorize this scheduled-run execution")
	}
	return nil
}

func scheduledRunExecutionPayloadDigest(decision scheduledRunExecutionDecision, intent ScheduledRunExecutionIntent) string {
	hash := sha256.New()
	hash.Write([]byte(controlScheduledRunExecutionAction + "\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", decision.OrgID}, {"user_id", decision.SubjectID}, {"space_id", decision.SpaceRef},
		{"schedule_id", intent.ScheduleID}, {"fire_key", intent.FireKey}, {"run_id", intent.RunID},
		{"system_thread_key", "schedule/" + intent.ScheduleID + "/" + intent.FireKey},
		{"template_digest", intent.TemplateDigest}, {"recipient_audience_ref", decision.RecipientAudienceRef},
		{"recipient_audience_hash", decision.RecipientAudienceHash}, {"privacy_policy_ref", decision.PrivacyPolicyRef},
		{"resource_authorization_ref", decision.ResourceAuthorizationRef},
		{"action_schema_hash", controlScheduledRunExecutionSchema}, {"thread_id", intent.ThreadID},
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
		{"authority_revision", decision.AuthorityRevision},
		{"membership_revision", decision.MembershipRevision},
		{"privacy_revision", decision.PrivacyRevision},
		{"recipient_audience_revision", decision.RecipientAudienceRevision},
		{"entitlement_revision", decision.EntitlementRevision},
	} {
		hash.Write([]byte(revision.name))
		hash.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		hash.Write(encoded[:])
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

func hasScheduledRunExecutionPermission(permissions []string) bool {
	for _, permission := range permissions {
		if permission == "schedule:execute" {
			return true
		}
	}
	return false
}
