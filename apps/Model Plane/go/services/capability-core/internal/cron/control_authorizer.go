package cron

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
	controlDecisionVersion    = "v2"
	controlCreateAction       = "model.cron.create"
	controlCreateAudience     = "model-plane-capability-core"
	controlCreateSchema       = "sha256:space-cron-create-v1"
	controlFireAction         = "model.cron.fire"
	controlFireAudience       = "model-plane-capability-core"
	controlFireSchema         = "sha256:space-cron-fire-v1"
	controlScheduledRunAction = "model.schedule.run"
	controlScheduledRunSchema = "sha256:space-scheduled-run-v1"
)

// ControlFireAuthorizer is Capability Core's narrow client for the Control
// schedule-fire decision endpoint. It never accepts a caller-provided token:
// each slot is reauthorized with the scheduler service credential and the
// returned asymmetric decision is verified locally before task creation.
type ControlFireAuthorizer struct {
	endpoint string
	token    string
	verifier *ControlDecisionVerifier
	client   *http.Client
}

// ScheduledRunPreparation is the verified one-call material for Session Core.
// Token is bearer authority and must never be persisted or forwarded to Temporal.
type ScheduledRunPreparation struct {
	Decision        controlDecision
	Token           string
	SystemThreadKey string
	RunID           string
}

func (a *ControlFireAuthorizer) AuthorizeScheduledRun(ctx context.Context, intent FireIntent, taskID string) (ScheduledRunPreparation, error) {
	if err := validateFireIntent(intent); err != nil {
		return ScheduledRunPreparation{}, err
	}
	if strings.TrimSpace(taskID) == "" {
		return ScheduledRunPreparation{}, fmt.Errorf("scheduled run task id is required")
	}
	body, err := json.Marshal(map[string]any{"intent": map[string]string{
		"org_id": intent.OrgID, "space_ref": intent.SpaceRef, "subject_id": intent.SubjectID,
		"schedule_id": intent.ScheduleID, "fire_key": intent.FireKey, "task_id": taskID,
		"template_digest": intent.TemplateDigest, "idempotency_key": intent.IdempotencyKey,
	}})
	if err != nil {
		return ScheduledRunPreparation{}, fmt.Errorf("encode scheduled run intent: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, a.endpoint+"/api/v1/internal/spaces/scheduled-run-decision", bytes.NewReader(body))
	if err != nil {
		return ScheduledRunPreparation{}, fmt.Errorf("build scheduled run authorization request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Token", a.token)
	response, err := a.client.Do(request)
	if err != nil {
		return ScheduledRunPreparation{}, fmt.Errorf("request scheduled run authority: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return ScheduledRunPreparation{}, fmt.Errorf("Control scheduled run authority rejected: status %d", response.StatusCode)
	}
	var envelope struct {
		Data struct {
			Decision        controlDecision `json:"decision"`
			Token           string          `json:"token"`
			SystemThreadKey string          `json:"system_thread_key"`
			RunID           string          `json:"run_id"`
		} `json:"data"`
	}
	if err := json.NewDecoder(io.LimitReader(response.Body, 64<<10)).Decode(&envelope); err != nil {
		return ScheduledRunPreparation{}, fmt.Errorf("decode scheduled run authority: %w", err)
	}
	verified, err := a.verifier.verify(envelope.Data.Token)
	if err != nil {
		return ScheduledRunPreparation{}, err
	}
	if envelope.Data.RunID != taskID || envelope.Data.SystemThreadKey != "schedule/"+intent.ScheduleID+"/"+intent.FireKey {
		return ScheduledRunPreparation{}, fmt.Errorf("Control scheduled run identifiers do not match the claimed task")
	}
	if verified.ActionID != controlScheduledRunAction || verified.ActionSchemaHash != controlScheduledRunSchema ||
		verified.ServiceAudience != controlFireAudience || verified.OrgID != intent.OrgID || verified.SpaceRef != intent.SpaceRef ||
		verified.SubjectID != intent.SubjectID || verified.IdempotencyKey != intent.IdempotencyKey || verified.ZeroDataRetention ||
		!time.Now().UTC().Before(verified.ExpiresAt) || !hasPermission(verified.Permissions, "schedule:run") ||
		verified.PayloadDigest != expectedScheduledRunPayloadDigest(verified, intent, taskID, envelope.Data.SystemThreadKey) {
		return ScheduledRunPreparation{}, fmt.Errorf("Control scheduled run decision does not bind this task")
	}
	envelope.Data.Decision = verified
	return ScheduledRunPreparation(envelope.Data), nil
}

// ControlDecisionVerifier verifies only Control's public, key-identified
// decision envelope. It holds no private key and is shared by schedule create
// admission and fire-time reauthorization.
type ControlDecisionVerifier struct {
	keyID  string
	public ed25519.PublicKey
}

// NewControlFireAuthorizer requires all deployment material explicitly. There
// is no development key or permissive network fallback because either would
// turn a missing Control dependency into standing cron authority.
func NewControlFireAuthorizer(endpoint, token, keyID, publicKeyBase64 string, client *http.Client) (*ControlFireAuthorizer, error) {
	endpoint = strings.TrimRight(strings.TrimSpace(endpoint), "/")
	token = strings.TrimSpace(token)
	keyID = strings.TrimSpace(keyID)
	if endpoint == "" || token == "" || keyID == "" {
		return nil, fmt.Errorf("Control schedule authorization endpoint, token, and key ID are required")
	}
	verifier, err := NewControlDecisionVerifier(keyID, publicKeyBase64)
	if err != nil {
		return nil, err
	}
	if client == nil {
		client = &http.Client{Timeout: 5 * time.Second}
	}
	return &ControlFireAuthorizer{endpoint: endpoint, token: token, verifier: verifier, client: client}, nil
}

func NewControlDecisionVerifier(keyID, publicKeyBase64 string) (*ControlDecisionVerifier, error) {
	keyID = strings.TrimSpace(keyID)
	publicRaw, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(publicKeyBase64))
	if keyID == "" || err != nil || len(publicRaw) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("Control schedule authorization public key is invalid")
	}
	return &ControlDecisionVerifier{keyID: keyID, public: ed25519.PublicKey(publicRaw)}, nil
}

type controlDecision struct {
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
	IssuedAt                  time.Time `json:"issued_at"`
	ExpiresAt                 time.Time `json:"expires_at"`
}

// CreateIntent is the non-secret persistence intent verified when a user
// creates a scoped schedule. The BFF obtains the signed Control token; this
// recipient recomputes the digest instead of trusting any of its fields.
type CreateIntent struct {
	OrgID          string
	SpaceRef       string
	SubjectID      string
	ScheduleID     string
	TemplateDigest string
	IdempotencyKey string
}

// ScheduleCreateBinding is the verified, non-secret projection that becomes
// part of the durable cron row. It is constructed only after an asymmetric
// Control decision verifies; callers never populate it directly.
type ScheduleCreateBinding struct {
	SpaceRef                  string
	SubjectID                 string
	RecipientAudienceRef      string
	RecipientAudienceHash     string
	ResourceAuthorizationRef  string
	PrivacyPolicyRef          string
	AuthorityRevision         int64
	MembershipRevision        int64
	PrivacyRevision           int64
	RecipientAudienceRevision int64
	EntitlementRevision       int64
}

func (v *ControlDecisionVerifier) VerifyScheduleCreate(token string, intent CreateIntent, now time.Time) (ScheduleCreateBinding, error) {
	if err := validateCreateIntent(intent); err != nil {
		return ScheduleCreateBinding{}, err
	}
	decision, err := v.verify(token)
	if err != nil {
		return ScheduleCreateBinding{}, err
	}
	if decision.OrgID != intent.OrgID || decision.SpaceRef != intent.SpaceRef || decision.SubjectID != intent.SubjectID ||
		decision.ServiceAudience != controlCreateAudience || decision.ActionID != controlCreateAction ||
		decision.ActionSchemaHash != controlCreateSchema || decision.IdempotencyKey != intent.IdempotencyKey {
		return ScheduleCreateBinding{}, fmt.Errorf("Control schedule create decision target does not match intent")
	}
	if decision.ZeroDataRetention || !now.Before(decision.ExpiresAt) || decision.IssuedAt.After(now.Add(time.Minute)) || !hasPermission(decision.Permissions, "cron:create") {
		return ScheduleCreateBinding{}, fmt.Errorf("Control schedule create decision is not currently usable")
	}
	if decision.PayloadDigest != expectedCreatePayloadDigest(decision, intent) {
		return ScheduleCreateBinding{}, fmt.Errorf("Control schedule create decision payload does not bind this schedule")
	}
	return ScheduleCreateBinding{
		SpaceRef: decision.SpaceRef, SubjectID: decision.SubjectID,
		RecipientAudienceRef: decision.RecipientAudienceRef, RecipientAudienceHash: decision.RecipientAudienceHash,
		ResourceAuthorizationRef: decision.ResourceAuthorizationRef, PrivacyPolicyRef: decision.PrivacyPolicyRef,
		AuthorityRevision: decision.AuthorityRevision, MembershipRevision: decision.MembershipRevision,
		PrivacyRevision: decision.PrivacyRevision, RecipientAudienceRevision: decision.RecipientAudienceRevision,
		EntitlementRevision: decision.EntitlementRevision,
	}, nil
}

func (a *ControlFireAuthorizer) AuthorizeFire(ctx context.Context, intent FireIntent) error {
	if err := validateFireIntent(intent); err != nil {
		return err
	}
	body, err := json.Marshal(map[string]any{"intent": map[string]string{
		"org_id": intent.OrgID, "space_ref": intent.SpaceRef, "subject_id": intent.SubjectID,
		"schedule_id": intent.ScheduleID, "fire_key": intent.FireKey, "template_digest": intent.TemplateDigest,
		"idempotency_key": intent.IdempotencyKey,
	}})
	if err != nil {
		return fmt.Errorf("encode schedule fire intent: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, a.endpoint+"/api/v1/internal/spaces/schedule-fire-decision", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build Control schedule authorization request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Token", a.token)
	response, err := a.client.Do(request)
	if err != nil {
		return fmt.Errorf("request fresh Control schedule authority: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return fmt.Errorf("Control schedule authority rejected fire: status %d", response.StatusCode)
	}
	var envelope struct {
		Data struct {
			Token string `json:"token"`
		} `json:"data"`
	}
	if err := json.NewDecoder(io.LimitReader(response.Body, 64<<10)).Decode(&envelope); err != nil {
		return fmt.Errorf("decode Control schedule authority response: %w", err)
	}
	decision, err := a.verifier.verify(envelope.Data.Token)
	if err != nil {
		return err
	}
	if err := validateFireDecision(decision, intent, time.Now().UTC()); err != nil {
		return err
	}
	return nil
}

func (v *ControlDecisionVerifier) verify(token string) (controlDecision, error) {
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != controlDecisionVersion {
		return controlDecision{}, fmt.Errorf("invalid Control schedule decision envelope")
	}
	encodedKeyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(encodedKeyID) != v.keyID {
		return controlDecision{}, fmt.Errorf("untrusted Control schedule decision key")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || !ed25519.Verify(v.public, []byte(strings.Join(parts[:3], ".")), signature) {
		return controlDecision{}, fmt.Errorf("invalid Control schedule decision signature")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return controlDecision{}, fmt.Errorf("invalid Control schedule decision payload")
	}
	var decision controlDecision
	if err := json.Unmarshal(payload, &decision); err != nil {
		return controlDecision{}, fmt.Errorf("invalid Control schedule decision JSON")
	}
	return decision, nil
}

func validateFireIntent(intent FireIntent) error {
	for name, value := range map[string]string{
		"org_id": intent.OrgID, "space_ref": intent.SpaceRef, "subject_id": intent.SubjectID,
		"schedule_id": intent.ScheduleID, "fire_key": intent.FireKey, "template_digest": intent.TemplateDigest,
		"idempotency_key": intent.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("schedule fire %s is required", name)
		}
	}
	if !strings.HasPrefix(intent.TemplateDigest, "sha256:") || len(intent.TemplateDigest) != len("sha256:")+64 {
		return fmt.Errorf("schedule fire template digest is invalid")
	}
	return nil
}

func validateCreateIntent(intent CreateIntent) error {
	for name, value := range map[string]string{
		"org_id": intent.OrgID, "space_ref": intent.SpaceRef, "subject_id": intent.SubjectID,
		"schedule_id": intent.ScheduleID, "template_digest": intent.TemplateDigest, "idempotency_key": intent.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("schedule create %s is required", name)
		}
	}
	if !strings.HasPrefix(intent.TemplateDigest, "sha256:") || len(intent.TemplateDigest) != len("sha256:")+64 {
		return fmt.Errorf("schedule create template digest is invalid")
	}
	return nil
}

func validateFireDecision(decision controlDecision, intent FireIntent, now time.Time) error {
	if decision.OrgID != intent.OrgID || decision.SpaceRef != intent.SpaceRef || decision.SubjectID != intent.SubjectID ||
		decision.ServiceAudience != controlFireAudience || decision.ActionID != controlFireAction ||
		decision.ActionSchemaHash != controlFireSchema || decision.IdempotencyKey != intent.IdempotencyKey {
		return fmt.Errorf("Control schedule decision target does not match the claimed fire")
	}
	if decision.ZeroDataRetention || !now.Before(decision.ExpiresAt) || decision.IssuedAt.After(now.Add(time.Minute)) || !hasPermission(decision.Permissions, "cron:fire") {
		return fmt.Errorf("Control schedule decision is not currently usable")
	}
	if decision.PayloadDigest != expectedFirePayloadDigest(decision, intent) {
		return fmt.Errorf("Control schedule decision payload does not bind this fire")
	}
	return nil
}

func hasPermission(permissions []string, required string) bool {
	for _, permission := range permissions {
		if permission == required {
			return true
		}
	}
	return false
}

// expectedFirePayloadDigest mirrors Control's length-prefixed public contract.
// Keep it local to the Model recipient so it does not import Control's private
// store or signing key; cross-plane drift is caught by the signed contract
// tests below and deployment compatibility checks.
func expectedFirePayloadDigest(decision controlDecision, intent FireIntent) string {
	hash := sha256.New()
	hash.Write([]byte("model.cron.fire\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", decision.OrgID}, {"user_id", decision.SubjectID}, {"space_id", decision.SpaceRef},
		{"schedule_id", intent.ScheduleID}, {"fire_key", intent.FireKey}, {"template_digest", intent.TemplateDigest},
		{"recipient_audience_ref", decision.RecipientAudienceRef}, {"recipient_audience_hash", decision.RecipientAudienceHash},
		{"privacy_policy_ref", decision.PrivacyPolicyRef}, {"resource_authorization_ref", decision.ResourceAuthorizationRef},
		{"action_schema_hash", controlFireSchema}, {"idempotency_key", intent.IdempotencyKey},
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

func expectedScheduledRunPayloadDigest(decision controlDecision, intent FireIntent, taskID, threadKey string) string {
	hash := sha256.New()
	hash.Write([]byte("model.schedule.run\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", decision.OrgID}, {"user_id", decision.SubjectID}, {"space_id", decision.SpaceRef},
		{"schedule_id", intent.ScheduleID}, {"fire_key", intent.FireKey}, {"run_id", taskID}, {"system_thread_key", threadKey},
		{"template_digest", intent.TemplateDigest}, {"recipient_audience_ref", decision.RecipientAudienceRef}, {"recipient_audience_hash", decision.RecipientAudienceHash},
		{"privacy_policy_ref", decision.PrivacyPolicyRef}, {"resource_authorization_ref", decision.ResourceAuthorizationRef},
		{"action_schema_hash", controlScheduledRunSchema}, {"idempotency_key", intent.IdempotencyKey},
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
		{"authority_revision", decision.AuthorityRevision}, {"membership_revision", decision.MembershipRevision}, {"privacy_revision", decision.PrivacyRevision},
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

func expectedCreatePayloadDigest(decision controlDecision, intent CreateIntent) string {
	hash := sha256.New()
	hash.Write([]byte("model.cron.create\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", decision.OrgID}, {"user_id", decision.SubjectID}, {"space_id", decision.SpaceRef},
		{"schedule_id", intent.ScheduleID}, {"template_digest", intent.TemplateDigest},
		{"recipient_audience_ref", decision.RecipientAudienceRef}, {"recipient_audience_hash", decision.RecipientAudienceHash},
		{"privacy_policy_ref", decision.PrivacyPolicyRef}, {"resource_authorization_ref", decision.ResourceAuthorizationRef},
		{"action_schema_hash", controlCreateSchema}, {"idempotency_key", intent.IdempotencyKey},
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
		{"authority_revision", decision.AuthorityRevision}, {"membership_revision", decision.MembershipRevision},
		{"privacy_revision", decision.PrivacyRevision}, {"recipient_audience_revision", decision.RecipientAudienceRevision},
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
