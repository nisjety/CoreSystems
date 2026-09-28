package spaces

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"time"
)

// mail_task_thread_decision.go — delegated-identity design, Gap B (§2.2 of
// 03-design-2-token-exchange.md). A LEAN, purpose-built claim family for one
// RPC: session-core's PrepareUserOwnedThread when origin=="mail_task".
//
// Deliberately NOT built on top of the full Space Decision (decision.go):
// mail-task has no Space, no membership, no recipient audience, no resource
// authorization ref — reusing Decision would mean synthesizing ten
// mandatory-but-unchecked fields (security-review §B-1) and sharing the
// "v2" envelope tag with five unrelated decision families whose verifiers
// this one must never be confusable with (§B-2). This reuses only the
// signing/verification primitives from decision.go (SigningKey,
// LoadSigningKeyFromEnv, the version.keyid.payload.signature envelope
// shape) — a distinct version tag makes it structurally impossible for a
// mail-task-v1 decision to verify against any other decision family's
// verifier, or vice versa.
const (
	mailTaskThreadDecisionVersion = "mail-task-v1"
	mailTaskThreadPrepareAction   = "mail_task:thread_prepare"
	mailTaskThreadPrepareAudience = "model-plane-session-core"
	// Longer than the Gap A token-exchange's 90s (design §2.6): this gates an
	// asynchronous pipeline step (Postmark webhook redelivery, at-least-once
	// processing), not one live HTTP round trip.
	mailTaskDecisionLifetime = 5 * time.Minute
)

// MailTaskThreadDecision is the signed, single-purpose claim that binds one
// resolved mail-task alias to one prepared session-core thread. Every field
// the eventual PrepareUserOwnedThreadRequest carries is checked field-for-
// field by session-core's verifier (grpc.rs) — a decision minted for one
// (org, user, session_key) can never be pointed at a different one.
type MailTaskThreadDecision struct {
	DecisionRef       string    `json:"decision_ref"`
	OrgID             string    `json:"org_id"`
	UserID            string    `json:"user_id"`
	AliasID           string    `json:"alias_id"`
	ProviderMessageID string    `json:"provider_message_id"`
	SessionKey        string    `json:"session_key"`
	Origin            string    `json:"origin"`
	ServiceAudience   string    `json:"service_audience"`
	ActionID          string    `json:"action_id"`
	Nonce             string    `json:"nonce"`
	IssuedAt          time.Time `json:"issued_at"`
	ExpiresAt         time.Time `json:"expires_at"`
}

// MailTaskSessionKey computes the identical session_key formula the gateway
// BFF's IngestRequest::session_key() uses (mail_task.rs): "mail_task/" +
// sha256(org_id + provider_message_id), so the decision and the eventual RPC
// request are checkable field-for-field, exactly like
// verify_scheduled_run_decision_with_keys compares every field.
func MailTaskSessionKey(orgID, providerMessageID string) string {
	digest := sha256.Sum256([]byte(orgID + providerMessageID))
	return "mail_task/" + hex.EncodeToString(digest[:])
}

func (d MailTaskThreadDecision) Validate() error {
	for name, value := range map[string]string{
		"decision_ref":        d.DecisionRef,
		"org_id":              d.OrgID,
		"user_id":             d.UserID,
		"alias_id":            d.AliasID,
		"provider_message_id": d.ProviderMessageID,
		"session_key":         d.SessionKey,
		"origin":              d.Origin,
		"service_audience":    d.ServiceAudience,
		"action_id":           d.ActionID,
		"nonce":               d.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("mail task thread decision %s is required", name)
		}
	}
	if d.Origin != "mail_task" {
		return fmt.Errorf("mail task thread decision origin must be mail_task")
	}
	if d.ServiceAudience != mailTaskThreadPrepareAudience {
		return fmt.Errorf("mail task thread decision service audience is invalid")
	}
	if d.ActionID != mailTaskThreadPrepareAction {
		return fmt.Errorf("mail task thread decision action is invalid")
	}
	if d.SessionKey != MailTaskSessionKey(d.OrgID, d.ProviderMessageID) {
		return fmt.Errorf("mail task thread decision session key does not match its own org/message")
	}
	if d.IssuedAt.IsZero() || d.ExpiresAt.IsZero() || !d.ExpiresAt.After(d.IssuedAt) {
		return fmt.Errorf("mail task thread decision expiry must be after issuance")
	}
	return nil
}

// IssueMailTaskThreadDecision mints a mail-task-v1 decision from the org's
// current, freshly resolved effect policy. Refuses for a ZDR-scoped org
// (mirrors issueScheduledRunDecision's existing rule, scheduled_run_decision.go
// — "zero data retention forbids durable scheduled runs") and for an org
// without ThreadCreateEntitled — the two owner-controlled floors this
// deliberately answers rather than silently resolving (design §2.8). Both are
// safe, fail-closed refusals: the caller (mail_task_handlers.go's
// resolveMailTaskAlias) simply omits thread_decision from its response,
// which downstream keeps `MailTaskPipeline` unable to reach
// PrepareUserOwnedThread for that inbound message, exactly the existing
// 503 mail_task_pipeline_not_configured default.
func IssueMailTaskThreadDecision(policy EffectPolicy, key SigningKey, orgID, userID, aliasID, providerMessageID string, now time.Time) (string, error) {
	orgID = strings.TrimSpace(orgID)
	userID = strings.TrimSpace(userID)
	aliasID = strings.TrimSpace(aliasID)
	providerMessageID = strings.TrimSpace(providerMessageID)
	if orgID == "" || userID == "" || aliasID == "" || providerMessageID == "" || now.IsZero() {
		return "", fmt.Errorf("mail task thread decision request is incomplete")
	}
	if policy.OrgID != orgID {
		return "", fmt.Errorf("mail task thread decision effect policy does not match org")
	}
	if policy.ZeroDataRetention {
		return "", fmt.Errorf("zero data retention forbids a mail task thread decision")
	}
	if !policy.ThreadCreateEntitled {
		return "", fmt.Errorf("thread creation is not entitled for this organization")
	}
	decisionRef, err := randomMailTaskDecisionPart()
	if err != nil {
		return "", fmt.Errorf("mail task decision reference entropy: %w", err)
	}
	nonce, err := randomMailTaskDecisionPart()
	if err != nil {
		return "", fmt.Errorf("mail task decision nonce entropy: %w", err)
	}
	decision := MailTaskThreadDecision{
		DecisionRef:       decisionRef,
		OrgID:             orgID,
		UserID:            userID,
		AliasID:           aliasID,
		ProviderMessageID: providerMessageID,
		SessionKey:        MailTaskSessionKey(orgID, providerMessageID),
		Origin:            "mail_task",
		ServiceAudience:   mailTaskThreadPrepareAudience,
		ActionID:          mailTaskThreadPrepareAction,
		Nonce:             nonce,
		IssuedAt:          now.UTC(),
		ExpiresAt:         now.UTC().Add(mailTaskDecisionLifetime),
	}
	return SignMailTaskThreadDecision(key, decision)
}

// SignMailTaskThreadDecision uses a distinct envelope version from every
// other Control decision family (B-2): a dropped field-equality check in a
// future shared-verifier refactor of the Space-decision family cannot
// accidentally accept a mail-task-v1 token, or vice versa, because the
// version tag itself is checked first and never overlaps.
func SignMailTaskThreadDecision(key SigningKey, decision MailTaskThreadDecision) (string, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		return "", fmt.Errorf("valid mail task decision signing key is required")
	}
	if err := decision.Validate(); err != nil {
		return "", err
	}
	payload, err := json.Marshal(decision)
	if err != nil {
		return "", fmt.Errorf("marshal mail task thread decision: %w", err)
	}
	signed := mailTaskThreadDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(key.ID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(key.PrivateKey, []byte(signed))), nil
}

// VerifyMailTaskThreadDecision is user-core's own round-trip check (used by
// this package's tests to prove Issue/Sign produces exactly what
// session-core's Rust verify_mail_task_thread_decision_with_keys expects —
// same version.keyid.payload.signature parsing, same field set). Production
// verification of this token happens in session-core (grpc.rs), not here;
// user-core never re-verifies its own issuance in the live request path.
func VerifyMailTaskThreadDecision(key VerificationKey, token string, now time.Time) (MailTaskThreadDecision, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PublicKey) != ed25519.PublicKeySize || now.IsZero() {
		return MailTaskThreadDecision{}, fmt.Errorf("valid mail task decision verification key is required")
	}
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != mailTaskThreadDecisionVersion {
		return MailTaskThreadDecision{}, fmt.Errorf("mail task thread decision envelope is invalid")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != key.ID {
		return MailTaskThreadDecision{}, fmt.Errorf("mail task thread decision key is invalid")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return MailTaskThreadDecision{}, fmt.Errorf("mail task thread decision payload is invalid")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || len(signature) != ed25519.SignatureSize ||
		!ed25519.Verify(key.PublicKey, []byte(parts[0]+"."+parts[1]+"."+parts[2]), signature) {
		return MailTaskThreadDecision{}, fmt.Errorf("mail task thread decision signature is invalid")
	}
	decoder := json.NewDecoder(strings.NewReader(string(payload)))
	decoder.DisallowUnknownFields()
	var decision MailTaskThreadDecision
	if err := decoder.Decode(&decision); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return MailTaskThreadDecision{}, fmt.Errorf("mail task thread decision claims are invalid")
	}
	if err := decision.Validate(); err != nil {
		return MailTaskThreadDecision{}, err
	}
	if decision.ExpiresAt.Before(now) {
		return MailTaskThreadDecision{}, fmt.Errorf("mail task thread decision is expired")
	}
	if decision.IssuedAt.After(now.Add(time.Minute)) {
		return MailTaskThreadDecision{}, fmt.Errorf("mail task thread decision issued in the future")
	}
	return decision, nil
}

func randomMailTaskDecisionPart() (string, error) {
	bytes := make([]byte, 24)
	if _, err := rand.Read(bytes); err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(bytes), nil
}
