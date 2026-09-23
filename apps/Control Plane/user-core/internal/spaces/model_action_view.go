package spaces

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// A model-action view is deliberately a separate contract from a target-bound
// RunActionDecision. It lets Capability Core decide whether the Model may be
// shown one fixed action for one current run; it never authorizes an owner
// resource effect. Execution still obtains a new RunActionDecision with a
// payload digest for every attempted ticket create, and Conversation Core
// rechecks current authority and its own resource grant before writing.
const (
	modelActionViewVersion          = "model-action-view-v1"
	modelActionViewServiceAudience  = "model-plane-capability-core"
	modelActionViewPermission       = "model-action:view"
	ticketsCreateActionSchemaSHA256 = "sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df"
)

// ModelActionViewRequest contains no actor, target resource, or action name.
// Control resolves the single supported owner action in server code rather
// than accepting a caller-selected catalog entry.
type ModelActionViewRequest struct {
	DecisionRef string
	Nonce       string
}

func (r ModelActionViewRequest) Validate() error {
	for name, value := range map[string]string{
		"decision_ref": r.DecisionRef,
		"nonce":        r.Nonce,
	} {
		value = strings.TrimSpace(value)
		if value == "" || len(value) > 200 {
			return fmt.Errorf("model action view %s is invalid", name)
		}
	}
	return nil
}

// ModelActionView is a short-lived, signed run-context view of the only
// owner-approved Model action. All identity, audience, privacy, retention, and
// source-context fields are carried so Capability Core can reject a token that
// does not exactly correspond to the requesting tenant/run. The permission is
// intentionally view-only and cannot be substituted for owner-effect authority.
type ModelActionView struct {
	DecisionRef                string    `json:"decision_ref"`
	RunID                      string    `json:"run_id"`
	ThreadID                   string    `json:"thread_id"`
	OrgID                      string    `json:"org_id"`
	SpaceRef                   string    `json:"space_ref"`
	SubjectID                  string    `json:"subject_id"`
	ServiceAudience            string    `json:"service_audience"`
	ActionID                   string    `json:"action_id"`
	ActionSchemaHash           string    `json:"action_schema_hash"`
	RecipientAudienceRef       string    `json:"recipient_audience_ref"`
	RecipientAudienceHash      string    `json:"recipient_audience_hash"`
	RecipientAudienceRevision  int64     `json:"recipient_audience_revision"`
	PrivacyPolicyRef           string    `json:"privacy_policy_ref"`
	RunContextAuthorizationRef string    `json:"run_context_authorization_ref"`
	AuthorityRevision          int64     `json:"authority_revision"`
	Permissions                []string  `json:"permissions"`
	Purpose                    string    `json:"purpose"`
	LawfulBasis                string    `json:"lawful_basis"`
	PrivacyClass               string    `json:"privacy_class"`
	ThirdPartyAllowed          bool      `json:"third_party_processing_allowed"`
	RetentionClass             string    `json:"retention_class"`
	Residency                  string    `json:"residency"`
	DeletionScope              string    `json:"deletion_scope"`
	ZeroDataRetention          bool      `json:"zero_data_retention"`
	IssuedAt                   time.Time `json:"issued_at"`
	ExpiresAt                  time.Time `json:"expires_at"`
	Nonce                      string    `json:"nonce"`
}

func (v ModelActionView) Validate() error {
	for name, value := range map[string]string{
		"decision_ref":                  v.DecisionRef,
		"run_id":                        v.RunID,
		"thread_id":                     v.ThreadID,
		"org_id":                        v.OrgID,
		"space_ref":                     v.SpaceRef,
		"subject_id":                    v.SubjectID,
		"service_audience":              v.ServiceAudience,
		"action_id":                     v.ActionID,
		"action_schema_hash":            v.ActionSchemaHash,
		"recipient_audience_ref":        v.RecipientAudienceRef,
		"recipient_audience_hash":       v.RecipientAudienceHash,
		"privacy_policy_ref":            v.PrivacyPolicyRef,
		"run_context_authorization_ref": v.RunContextAuthorizationRef,
		"purpose":                       v.Purpose,
		"lawful_basis":                  v.LawfulBasis,
		"privacy_class":                 v.PrivacyClass,
		"retention_class":               v.RetentionClass,
		"residency":                     v.Residency,
		"deletion_scope":                v.DeletionScope,
		"nonce":                         v.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("model action view %s is required", name)
		}
	}
	if v.ServiceAudience != modelActionViewServiceAudience ||
		v.ActionID != "tickets.create" ||
		v.ActionSchemaHash != ticketsCreateActionSchemaSHA256 ||
		v.RecipientAudienceRevision <= 0 || v.AuthorityRevision <= 0 ||
		len(v.Permissions) != 1 || v.Permissions[0] != modelActionViewPermission ||
		v.ZeroDataRetention || v.IssuedAt.IsZero() || v.ExpiresAt.IsZero() ||
		!v.ExpiresAt.After(v.IssuedAt) || v.ExpiresAt.Sub(v.IssuedAt) > personalDecisionLifetime {
		return fmt.Errorf("model action view authority is invalid")
	}
	return nil
}

// authorityMatchesEvidence reports whether Session Core's run authority
// exactly corresponds to the fresh Control evidence and its source-context
// binding.
func authorityMatchesEvidence(authority RunActionAuthority, evidence PersonalThreadDecisionEvidence, expectedSourceRef string) bool {
	return authority.OrgID == evidence.Membership.OrgID && authority.SubjectID == evidence.Membership.SubjectID &&
		authority.SpaceRef == evidence.Membership.SpaceRef && authority.RecipientAudienceRef == evidence.RecipientAudienceRef &&
		authority.RecipientAudienceRevision == evidence.Membership.Revisions.RecipientAudience &&
		authority.RecipientAudienceHash == evidence.RecipientAudienceHash &&
		authority.PrivacyPolicyRef == evidence.Privacy.PolicyRef &&
		authority.AuthorityRevision == evidence.Membership.Revisions.Authority &&
		authority.RunContextAuthorizationRef == expectedSourceRef &&
		evidence.ResourceAuthorizationRef == expectedSourceRef
}

// newModelActionView builds the evidence-bound fields of the view. The caller
// fills the request- and authority-carried identifiers.
func newModelActionView(evidence PersonalThreadDecisionEvidence, expectedSourceRef string, now time.Time) ModelActionView {
	return ModelActionView{
		OrgID:                      evidence.Membership.OrgID,
		SpaceRef:                   evidence.Membership.SpaceRef,
		SubjectID:                  evidence.Membership.SubjectID,
		ServiceAudience:            modelActionViewServiceAudience,
		ActionID:                   "tickets.create",
		ActionSchemaHash:           ticketsCreateActionSchemaSHA256,
		RecipientAudienceRef:       evidence.RecipientAudienceRef,
		RecipientAudienceHash:      evidence.RecipientAudienceHash,
		RecipientAudienceRevision:  evidence.Membership.Revisions.RecipientAudience,
		PrivacyPolicyRef:           evidence.Privacy.PolicyRef,
		RunContextAuthorizationRef: expectedSourceRef,
		AuthorityRevision:          evidence.Membership.Revisions.Authority,
		Permissions:                []string{modelActionViewPermission},
		Purpose:                    evidence.Privacy.Purpose,
		LawfulBasis:                evidence.Privacy.LawfulBasis,
		PrivacyClass:               evidence.Privacy.PrivacyClass,
		ThirdPartyAllowed:          evidence.Privacy.ThirdPartyAllowed,
		RetentionClass:             evidence.Privacy.RetentionClass,
		Residency:                  evidence.Privacy.Residency,
		DeletionScope:              evidence.Privacy.DeletionScope,
		ZeroDataRetention:          false,
		IssuedAt:                   now.UTC(),
		ExpiresAt:                  now.UTC().Add(personalDecisionLifetime),
	}
}

// IssueModelActionView intersects fresh Control evidence with Session Core's
// content-free run authority. No browser/Model caller chooses the action,
// subject, Space, recipient audience, or policy attributes.
func IssueModelActionView(evidence PersonalThreadDecisionEvidence, authority RunActionAuthority, request ModelActionViewRequest, now time.Time) (ModelActionView, error) {
	if err := evidence.ValidateForAgentAction(); err != nil {
		return ModelActionView{}, err
	}
	if err := authority.Validate(); err != nil {
		return ModelActionView{}, err
	}
	if err := request.Validate(); err != nil || now.IsZero() {
		return ModelActionView{}, fmt.Errorf("model action view request is invalid")
	}
	expectedSourceRef := fmt.Sprintf("control:%s:thread-create:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if !authorityMatchesEvidence(authority, evidence, expectedSourceRef) {
		return ModelActionView{}, fmt.Errorf("model action view authority does not match current Control authority")
	}
	view := newModelActionView(evidence, expectedSourceRef, now)
	view.DecisionRef = strings.TrimSpace(request.DecisionRef)
	view.RunID = strings.TrimSpace(authority.RunID)
	view.ThreadID = strings.TrimSpace(authority.ThreadID)
	view.Nonce = strings.TrimSpace(request.Nonce)
	return view, nil
}

// SignModelActionView has a dedicated envelope. A recipient must never accept
// a target-effect decision as a model catalog view, or vice versa.
func SignModelActionView(key SigningKey, view ModelActionView) (string, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		return "", fmt.Errorf("valid model action view signing key is required")
	}
	if err := view.Validate(); err != nil {
		return "", err
	}
	payload, err := json.Marshal(view)
	if err != nil {
		return "", fmt.Errorf("marshal model action view: %w", err)
	}
	signed := modelActionViewVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(key.ID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(key.PrivateKey, []byte(signed))), nil
}
