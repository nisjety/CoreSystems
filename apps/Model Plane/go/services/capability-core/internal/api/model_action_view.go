package api

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

const (
	modelActionViewPath            = "/api/v1/internal/model-actions/run-view"
	modelActionViewVersion         = "model-action-view-v1"
	modelActionViewServiceAudience = "model-plane-capability-core"
	modelActionViewPermission      = "model-action:view"
	ticketsCreateToolName          = "tickets.create"
	ticketsCreateCapabilityID      = "cap.tool.ticket.create"
	ticketsCreateActionSchemaHash  = "sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df"
	maxModelActionViewTokenBytes   = 16 << 10
	maxModelActionViewRequestBytes = 20 << 10
	maxModelActionViewLifetime     = 2 * time.Minute
)

// This is a server-owned schema rather than a frontend Action Registry copy.
// It mirrors Execution Core's strictly parsed ticket adapter. Any schema change
// requires changing both this contract hash and the owner effect contract.
const ticketsCreateParametersJSON = `{"type":"object","additionalProperties":false,"properties":{"conversation_id":{"type":"string","maxLength":200},"work_type":{"type":"string","enum":["","customer_case","internal_work","incident"]},"priority":{"type":"string","enum":["","low","normal","high","urgent"]},"severity":{"type":"string","enum":["","low","medium","high","critical"]},"category":{"type":"string","maxLength":80},"intent":{"type":"string","maxLength":240}},"required":["conversation_id"]}`

type modelActionViewRequest struct {
	RunID            string `json:"run_id"`
	ControlViewToken string `json:"control_view_token"`
}

type modelActionToolDefinition struct {
	Name             string `json:"name"`
	Description      string `json:"description"`
	ParametersJSON   string `json:"parameters_json"`
	ActionSchemaHash string `json:"action_schema_hash"`
	RequiresApproval bool   `json:"requires_approval"`
}

type modelActionViewData struct {
	RunID      string                      `json:"run_id"`
	Actions    []modelActionToolDefinition `json:"actions"`
	ReasonCode string                      `json:"reason_code,omitempty"`
}

type modelActionViewResponse struct {
	Data modelActionViewData `json:"data"`
}

// controlModelActionView is the exact signed Control envelope Capability Core
// accepts. It deliberately contains no payload digest/idempotency key and no
// owner-effect permission; the view determines presentation only.
type controlModelActionView struct {
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

// ControlModelActionViewVerifier trusts precisely one deployment-distributed
// Control public key. It never accepts a development fallback or key chosen by
// a request, avoiding an unsigned catalog path.
type ControlModelActionViewVerifier struct {
	keyID  string
	public ed25519.PublicKey
}

func NewControlModelActionViewVerifier(keyID, publicKeyBase64 string) (*ControlModelActionViewVerifier, error) {
	keyID = strings.TrimSpace(keyID)
	publicRaw, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(publicKeyBase64))
	if keyID == "" || err != nil || len(publicRaw) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("Control model action view public key is invalid")
	}
	return &ControlModelActionViewVerifier{keyID: keyID, public: ed25519.PublicKey(publicRaw)}, nil
}

func (h *CapabilitiesHandler) resolveModelActionView(w http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if h.availabilityStore == nil || h.modelActionViews == nil {
		jsonErr(w, "model action view is unavailable", http.StatusServiceUnavailable)
		return
	}
	request.Body = http.MaxBytesReader(w, request.Body, maxModelActionViewRequestBytes)
	decoder := json.NewDecoder(request.Body)
	decoder.DisallowUnknownFields()
	var input modelActionViewRequest
	if err := decoder.Decode(&input); err != nil || ensureJSONEOF(decoder) != nil {
		jsonErr(w, "invalid model action view request", http.StatusBadRequest)
		return
	}
	input.RunID = strings.TrimSpace(input.RunID)
	input.ControlViewToken = strings.TrimSpace(input.ControlViewToken)
	if input.RunID == "" || len(input.RunID) > 200 || input.ControlViewToken == "" || len(input.ControlViewToken) > maxModelActionViewTokenBytes {
		jsonErr(w, "invalid model action view request", http.StatusBadRequest)
		return
	}
	orgID := verifiedOrganizationID(request)
	if orgID == "" {
		jsonErr(w, "model action view is unavailable", http.StatusForbidden)
		return
	}
	view, err := h.modelActionViews.Verify(input.ControlViewToken, input.RunID, orgID, time.Now().UTC())
	if err != nil {
		jsonErr(w, "Control model action view is not authorized", http.StatusForbidden)
		return
	}
	row, err := h.availabilityStore.GetForOrg(request.Context(), ticketsCreateCapabilityID, orgID)
	if err != nil || row == nil || row.ID != ticketsCreateCapabilityID || (row.OrgID != orgID && row.OrgID != "global") {
		jsonErr(w, "model action capability is unavailable", http.StatusServiceUnavailable)
		return
	}
	availability := models.DeriveAvailability(&models.Capability{
		ID:                row.ID,
		Enabled:           row.Enabled,
		RiskLevel:         row.RiskLevel,
		RolloutState:      row.RolloutState,
		AvailabilityState: row.AvailabilityState,
		ReasonCode:        row.ReasonCode,
		Reason:            row.Reason,
		ExecutionMode:     row.ExecutionMode,
		CostClass:         row.CostClass,
		HealthCheckedAt:   row.HealthCheckedAt,
	})
	data := modelActionViewData{RunID: view.RunID, Actions: []modelActionToolDefinition{}}
	if availability.State != models.AvailabilityAvailable && availability.State != models.AvailabilityApprovalRequired {
		data.ReasonCode = availability.ReasonCode
		writeJSON(w, modelActionViewResponse{Data: data})
		return
	}
	if availability.ExecutionMode != models.ExecutionAgentic {
		data.ReasonCode = "execution_mode_unavailable"
		writeJSON(w, modelActionViewResponse{Data: data})
		return
	}
	data.Actions = append(data.Actions, modelActionToolDefinition{
		Name:             ticketsCreateToolName,
		Description:      "Create one governed ticket in a Conversation Core conversation. This always pauses for durable human approval and the owner independently rechecks current authority and its target resource grant before writing.",
		ParametersJSON:   ticketsCreateParametersJSON,
		ActionSchemaHash: ticketsCreateActionSchemaHash,
		RequiresApproval: true,
	})
	writeJSON(w, modelActionViewResponse{Data: data})
}

// Verify returns a view only when it was signed by the configured Control key,
// is fresh, and exactly matches the service's verified tenant and request run.
func (v *ControlModelActionViewVerifier) Verify(token, runID, orgID string, now time.Time) (controlModelActionView, error) {
	if v == nil || strings.TrimSpace(v.keyID) == "" || len(v.public) != ed25519.PublicKeySize {
		return controlModelActionView{}, fmt.Errorf("Control model action view verifier is unavailable")
	}
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != modelActionViewVersion {
		return controlModelActionView{}, fmt.Errorf("invalid Control model action view envelope")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != v.keyID {
		return controlModelActionView{}, fmt.Errorf("untrusted Control model action view key")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || !ed25519.Verify(v.public, []byte(strings.Join(parts[:3], ".")), signature) {
		return controlModelActionView{}, fmt.Errorf("invalid Control model action view signature")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil || len(payload) == 0 || len(payload) > maxModelActionViewTokenBytes {
		return controlModelActionView{}, fmt.Errorf("invalid Control model action view payload")
	}
	decoder := json.NewDecoder(strings.NewReader(string(payload)))
	decoder.DisallowUnknownFields()
	var view controlModelActionView
	if err := decoder.Decode(&view); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return controlModelActionView{}, fmt.Errorf("invalid Control model action view payload")
	}
	if err := view.Validate(now); err != nil || view.RunID != strings.TrimSpace(runID) || view.OrgID != strings.TrimSpace(orgID) {
		return controlModelActionView{}, fmt.Errorf("Control model action view is not bound to this request")
	}
	return view, nil
}

func (v controlModelActionView) Validate(now time.Time) error {
	for name, value := range map[string]string{
		"decision_ref": v.DecisionRef, "run_id": v.RunID, "thread_id": v.ThreadID, "org_id": v.OrgID,
		"space_ref": v.SpaceRef, "subject_id": v.SubjectID, "recipient_audience_ref": v.RecipientAudienceRef,
		"recipient_audience_hash": v.RecipientAudienceHash, "privacy_policy_ref": v.PrivacyPolicyRef,
		"run_context_authorization_ref": v.RunContextAuthorizationRef, "purpose": v.Purpose,
		"lawful_basis": v.LawfulBasis, "privacy_class": v.PrivacyClass, "retention_class": v.RetentionClass,
		"residency": v.Residency, "deletion_scope": v.DeletionScope, "nonce": v.Nonce,
	} {
		if strings.TrimSpace(value) == "" || len(strings.TrimSpace(value)) > 200 {
			return fmt.Errorf("Control model action view %s is invalid", name)
		}
	}
	if v.ServiceAudience != modelActionViewServiceAudience || v.ActionID != ticketsCreateToolName ||
		v.ActionSchemaHash != ticketsCreateActionSchemaHash || v.RecipientAudienceRevision <= 0 || v.AuthorityRevision <= 0 ||
		len(v.Permissions) != 1 || v.Permissions[0] != modelActionViewPermission || v.ZeroDataRetention ||
		v.IssuedAt.IsZero() || v.ExpiresAt.IsZero() || !v.ExpiresAt.After(v.IssuedAt) ||
		v.ExpiresAt.Sub(v.IssuedAt) > maxModelActionViewLifetime || now.Before(v.IssuedAt.Add(-30*time.Second)) || !now.Before(v.ExpiresAt) {
		return fmt.Errorf("Control model action view authority is invalid")
	}
	return nil
}
