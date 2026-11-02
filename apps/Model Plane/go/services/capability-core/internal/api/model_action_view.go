package api

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/decisionkey"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

const (
	modelActionViewPath            = "/api/v1/internal/model-actions/run-view"
	modelActionViewVersion         = "model-action-view-v1"
	modelActionViewVersionV2       = "model-action-view-v2"
	modelActionViewServiceAudience = "model-plane-capability-core"
	modelActionViewPermission      = "model-action:view"
	modelActionAdmissionEnv        = "MODEL_ACTION_ADMISSION"
	modelActionAdmissionTable      = "table"
	ticketsCreateToolName          = "tickets.create"
	ticketsCreateCapabilityID      = "cap.tool.ticket.create"
	ticketsCreateActionSchemaHash  = "sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df"
	maxModelActionViewTokenBytes   = 16 << 10
	maxModelActionViewRequestBytes = 20 << 10
	maxModelActionViewLifetime     = 2 * time.Minute
	maxModelActionViewAdmissions   = 16
)

// This is a server-owned schema rather than a frontend Action Registry copy.
// It mirrors Execution Core's strictly parsed ticket adapter. Any schema change
// requires changing both this contract hash and the owner effect contract.
const ticketsCreateParametersJSON = `{"type":"object","additionalProperties":false,"properties":{"conversation_id":{"type":"string","maxLength":200},"work_type":{"type":"string","enum":["","customer_case","internal_work","incident"]},"priority":{"type":"string","enum":["","low","normal","high","urgent"]},"severity":{"type":"string","enum":["","low","medium","high","critical"]},"category":{"type":"string","maxLength":80},"intent":{"type":"string","maxLength":240}},"required":["conversation_id"]}`

const ticketsCreateDescription = "Create one governed ticket in a Conversation Core conversation. This always pauses for durable human approval and the owner independently rechecks current authority and its target resource grant before writing."

var schemaHashPattern = regexp.MustCompile(`^sha256:[0-9a-f]{64}$`)

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

// agentActionAdmissionBackend reads the enabled admission rows for a set of action ids.
type agentActionAdmissionBackend interface {
	ListEnabled(context.Context, []string) ([]*registry.AgentActionAdmission, error)
}

// errModelActionHashMismatch means a signed view and the admission table disagree on an
// action's schema hash. The whole view is refused: a disagreement is an integrity signal,
// never something to offer around.
var errModelActionHashMismatch = errors.New("model action schema hash does not match the admission table")

// controlModelActionAuthority is the authority every signed Control view carries, whatever
// actions it lists. It deliberately contains no payload digest/idempotency key and no
// owner-effect permission; the view determines presentation only.
type controlModelActionAuthority struct {
	DecisionRef                string    `json:"decision_ref"`
	RunID                      string    `json:"run_id"`
	ThreadID                   string    `json:"thread_id"`
	OrgID                      string    `json:"org_id"`
	SpaceRef                   string    `json:"space_ref"`
	SubjectID                  string    `json:"subject_id"`
	ServiceAudience            string    `json:"service_audience"`
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

// controlModelActionView is the v1 envelope: exactly one action, tickets.create.
type controlModelActionView struct {
	controlModelActionAuthority
	ActionID         string `json:"action_id"`
	ActionSchemaHash string `json:"action_schema_hash"`
}

// controlModelActionViewV2 names every action the view offers, each with the schema hash
// Control signed for it. Capability Core offers an action only when its admission row is
// enabled and carries that same hash.
type controlModelActionViewV2 struct {
	controlModelActionAuthority
	ActionSchemaHashes map[string]string `json:"action_schema_hashes"`
}

// verifiedModelActionView is a signed view after its signature, freshness and run binding
// have been checked: the envelope version and the schema hash signed for each action.
type verifiedModelActionView struct {
	version            string
	authority          controlModelActionAuthority
	actionSchemaHashes map[string]string
}

// ControlModelActionViewVerifier trusts precisely one deployment-distributed
// Control public key. It never accepts a development fallback or key chosen by
// a request, avoiding an unsigned catalog path.
type ControlModelActionViewVerifier struct {
	keyID  string
	public ed25519.PublicKey
	// admissions and admissionTable are set together by WithAgentActionAdmissions. With the
	// table off, only the signed v1 tickets.create view is accepted and offered, as before.
	admissions     agentActionAdmissionBackend
	admissionTable bool
}

func NewControlModelActionViewVerifier(keyID, publicKeyBase64 string) (*ControlModelActionViewVerifier, error) {
	keyID = strings.TrimSpace(keyID)
	publicRaw, _, err := decisionkey.DecodePublicKey(publicKeyBase64)
	if keyID == "" || err != nil {
		return nil, fmt.Errorf("Control model action view public key is invalid")
	}
	return &ControlModelActionViewVerifier{keyID: keyID, public: ed25519.PublicKey(publicRaw)}, nil
}

// WithAgentActionAdmissions offers the actions a signed view names from the admission table
// when table is true. ModelActionAdmissionTableFromLookup decides table; with it false a v2
// view is refused, so the table can be switched off without a redeploy of the verifier key.
func (v *ControlModelActionViewVerifier) WithAgentActionAdmissions(admissions agentActionAdmissionBackend, table bool) *ControlModelActionViewVerifier {
	v.admissions = admissions
	v.admissionTable = table
	return v
}

// ModelActionAdmissionTableFromLookup reads MODEL_ACTION_ADMISSION strictly: only the literal
// "table" enables the admission table. Any other value, including unset, keeps the
// tickets.create-only behaviour that shipped before the table existed.
func ModelActionAdmissionTableFromLookup(lookup func(string) string) bool {
	return lookup(modelActionAdmissionEnv) == modelActionAdmissionTable
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
	if h.modelActionViews.admissionTable && h.modelActionViews.admissions == nil {
		jsonErr(w, "model action admission is unavailable", http.StatusServiceUnavailable)
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
	if view.version == modelActionViewVersionV2 && !h.modelActionViews.admissionTable {
		jsonErr(w, "Control model action view is not authorized", http.StatusForbidden)
		return
	}
	admitted, err := h.modelActionViews.admittedRows(request.Context(), view)
	if errors.Is(err, errModelActionHashMismatch) {
		jsonErr(w, "Control model action view is not authorized", http.StatusForbidden)
		return
	}
	if err != nil {
		jsonErr(w, "model action admission is unavailable", http.StatusServiceUnavailable)
		return
	}
	data := modelActionViewData{RunID: view.authority.RunID, Actions: []modelActionToolDefinition{}}
	for _, row := range admitted {
		capability, err := h.availabilityStore.GetForOrg(request.Context(), row.CapabilityID, orgID)
		if err != nil || capability == nil || capability.ID != row.CapabilityID || (capability.OrgID != orgID && capability.OrgID != "global") {
			jsonErr(w, "model action capability is unavailable", http.StatusServiceUnavailable)
			return
		}
		availability := models.DeriveAvailability(&models.Capability{
			ID:                capability.ID,
			Enabled:           capability.Enabled,
			RiskLevel:         capability.RiskLevel,
			RolloutState:      capability.RolloutState,
			AvailabilityState: capability.AvailabilityState,
			ReasonCode:        capability.ReasonCode,
			Reason:            capability.Reason,
			ExecutionMode:     capability.ExecutionMode,
			CostClass:         capability.CostClass,
			HealthCheckedAt:   capability.HealthCheckedAt,
		})
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
			Name:             row.ToolName,
			Description:      row.Description,
			ParametersJSON:   row.ParametersJSON,
			ActionSchemaHash: row.ActionSchemaHash,
			RequiresApproval: row.RequiresApproval,
		})
	}
	writeJSON(w, modelActionViewResponse{Data: data})
}

// admittedRows returns the admission rows the verified view may offer, in action-id order. With
// the table off that is the one constant tickets.create row. With it on, a row is offered only
// when it is enabled and the view signed its hash; a row whose hash differs from the signed one
// fails the whole view.
func (v *ControlModelActionViewVerifier) admittedRows(ctx context.Context, view verifiedModelActionView) ([]*registry.AgentActionAdmission, error) {
	if !v.admissionTable {
		return []*registry.AgentActionAdmission{ticketsCreateAdmission()}, nil
	}
	ids := make([]string, 0, len(view.actionSchemaHashes))
	for id := range view.actionSchemaHashes {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	stored, err := v.admissions.ListEnabled(ctx, ids)
	if err != nil {
		return nil, err
	}
	rows := make([]*registry.AgentActionAdmission, 0, len(stored))
	for _, row := range stored {
		if row == nil || row.Status != registry.AgentActionAdmissionEnabled {
			continue
		}
		signed, listed := view.actionSchemaHashes[row.ActionID]
		if !listed {
			continue
		}
		if signed != row.ActionSchemaHash {
			return nil, errModelActionHashMismatch
		}
		rows = append(rows, row)
	}
	return rows, nil
}

// ticketsCreateAdmission is the tickets.create row as the constants define it. The migration
// seeds the same row, and a test pins the two together.
func ticketsCreateAdmission() *registry.AgentActionAdmission {
	return &registry.AgentActionAdmission{
		ActionID:         ticketsCreateToolName,
		ToolName:         ticketsCreateToolName,
		CapabilityID:     ticketsCreateCapabilityID,
		ModelAccess:      "write_approval",
		SideEffectClass:  "owner_effect",
		ApprovalFloor:    "durable_owner",
		Authority:        "control_run_decision",
		HashBasis:        "owner_contract",
		ActionSchemaHash: ticketsCreateActionSchemaHash,
		ParametersJSON:   ticketsCreateParametersJSON,
		Description:      ticketsCreateDescription,
		RequiresApproval: true,
		Status:           registry.AgentActionAdmissionEnabled,
	}
}

// Verify returns a view only when it was signed by the configured Control key,
// is fresh, and exactly matches the service's verified tenant and request run.
func (v *ControlModelActionViewVerifier) Verify(token, runID, orgID string, now time.Time) (verifiedModelActionView, error) {
	if v == nil || strings.TrimSpace(v.keyID) == "" || len(v.public) != ed25519.PublicKeySize {
		return verifiedModelActionView{}, fmt.Errorf("Control model action view verifier is unavailable")
	}
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || (parts[0] != modelActionViewVersion && parts[0] != modelActionViewVersionV2) {
		return verifiedModelActionView{}, fmt.Errorf("invalid Control model action view envelope")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != v.keyID {
		if err == nil {
			decisionkey.ReportUnknownKey("model-action-view", string(keyID), v.keyID)
		}
		return verifiedModelActionView{}, fmt.Errorf("untrusted Control model action view key")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || !ed25519.Verify(v.public, []byte(strings.Join(parts[:3], ".")), signature) {
		return verifiedModelActionView{}, fmt.Errorf("invalid Control model action view signature")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil || len(payload) == 0 || len(payload) > maxModelActionViewTokenBytes {
		return verifiedModelActionView{}, fmt.Errorf("invalid Control model action view payload")
	}
	var verified verifiedModelActionView
	switch parts[0] {
	case modelActionViewVersion:
		var view controlModelActionView
		if err := decodeModelActionViewPayload(payload, &view); err != nil {
			return verifiedModelActionView{}, err
		}
		if err := view.Validate(now); err != nil {
			return verifiedModelActionView{}, err
		}
		verified = verifiedModelActionView{
			version:            parts[0],
			authority:          view.controlModelActionAuthority,
			actionSchemaHashes: map[string]string{view.ActionID: view.ActionSchemaHash},
		}
	case modelActionViewVersionV2:
		var view controlModelActionViewV2
		if err := decodeModelActionViewPayload(payload, &view); err != nil {
			return verifiedModelActionView{}, err
		}
		if err := view.Validate(now); err != nil {
			return verifiedModelActionView{}, err
		}
		verified = verifiedModelActionView{
			version:            parts[0],
			authority:          view.controlModelActionAuthority,
			actionSchemaHashes: view.ActionSchemaHashes,
		}
	}
	if verified.authority.RunID != strings.TrimSpace(runID) || verified.authority.OrgID != strings.TrimSpace(orgID) {
		return verifiedModelActionView{}, fmt.Errorf("Control model action view is not bound to this request")
	}
	return verified, nil
}

func decodeModelActionViewPayload(payload []byte, target any) error {
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return fmt.Errorf("invalid Control model action view payload")
	}
	return nil
}

func (v controlModelActionView) Validate(now time.Time) error {
	if err := v.controlModelActionAuthority.validate(now); err != nil {
		return err
	}
	if v.ActionID != ticketsCreateToolName || v.ActionSchemaHash != ticketsCreateActionSchemaHash {
		return fmt.Errorf("Control model action view authority is invalid")
	}
	return nil
}

func (v controlModelActionViewV2) Validate(now time.Time) error {
	if err := v.controlModelActionAuthority.validate(now); err != nil {
		return err
	}
	if len(v.ActionSchemaHashes) == 0 || len(v.ActionSchemaHashes) > maxModelActionViewAdmissions {
		return fmt.Errorf("Control model action view authority is invalid")
	}
	for actionID, hash := range v.ActionSchemaHashes {
		if strings.TrimSpace(actionID) == "" || len(actionID) > 200 || !schemaHashPattern.MatchString(hash) {
			return fmt.Errorf("Control model action view authority is invalid")
		}
	}
	return nil
}

func (v controlModelActionAuthority) validate(now time.Time) error {
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
	if v.ServiceAudience != modelActionViewServiceAudience || v.RecipientAudienceRevision <= 0 || v.AuthorityRevision <= 0 ||
		len(v.Permissions) != 1 || v.Permissions[0] != modelActionViewPermission || v.ZeroDataRetention ||
		v.IssuedAt.IsZero() || v.ExpiresAt.IsZero() || !v.ExpiresAt.After(v.IssuedAt) ||
		v.ExpiresAt.Sub(v.IssuedAt) > maxModelActionViewLifetime || now.Before(v.IssuedAt.Add(-30*time.Second)) || !now.Before(v.ExpiresAt) {
		return fmt.Errorf("Control model action view authority is invalid")
	}
	return nil
}
