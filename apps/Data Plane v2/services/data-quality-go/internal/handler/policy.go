package handler

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"
	chimw "github.com/go-chi/chi/v5/middleware"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/policy"
)

// PolicyStore is what the admin endpoints need of the policy table. The real
// implementation is policy.Store (row-level security, audit in the same
// transaction); the handler's own rules (who may call, which organization, what
// is valid) do not depend on it and are tested through the real middleware.
type PolicyStore interface {
	Get(ctx context.Context, orgID string) (policy.Record, error)
	Put(ctx context.Context, orgID string, in policy.Input, actor policy.Actor) (policy.Result, error)
}

// WithPolicyStore enables the processing-policy admin endpoints. Without a
// store they answer 503: the service never pretends to have changed a policy.
func (h *QualityHandler) WithPolicyStore(store PolicyStore) *QualityHandler {
	h.policies = store
	return h
}

func writeCodedError(w http.ResponseWriter, status int, code, msg string) {
	writeJSON(w, status, map[string]string{"error": msg, "code": code})
}

// requirePathOrg refuses a request whose {org} path segment is not a valid
// organization id (400) or is not the organization of the verified token (403).
//
// The path organization is NEVER the authority: the token's is. A policy is
// written for the organization the verified principal was minted for, so a token
// of organization A cannot write organization B's policy by naming B, and cannot
// even read it. (An operator reaches a customer organization the way every
// platform principal does, with a token minted for that organization.)
func requirePathOrg(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		claims, ok := authctx.FromContext(r.Context())
		if !ok {
			writeCodedError(w, http.StatusUnauthorized, "unauthorized", "authentication required")
			return
		}
		pathOrg := chi.URLParam(r, "org")
		if !policy.ValidOrgID(pathOrg) {
			writeCodedError(w, http.StatusBadRequest, "invalid_org", "the organization id in the path is not valid")
			return
		}
		if pathOrg != claims.OrgID {
			log.Warn().
				Str("principal", claims.PrincipalID()).
				Str("token_org_id", claims.OrgID).
				Str("path_org_id", pathOrg).
				Str("request_id", chimw.GetReqID(r.Context())).
				Msg("processing policy: a token named another organization; refused")
			writeCodedError(w, http.StatusForbidden, "tenant_mismatch", "the organization in the path does not match the verified identity")
			return
		}
		next.ServeHTTP(w, r)
	})
}

// requireOperator refuses everything but a verified SERVICE principal. A
// processing policy is an operator control, not a customer setting: an
// organization's own owner or admin holds data:quality:admin too (interactive
// tokens carry it), and must not be able to switch background processing off for
// their organization, or to lift a hold an operator placed. There is no
// organization-admin UI for this and the API does not give them one.
func requireOperator(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		claims, ok := authctx.FromContext(r.Context())
		if !ok {
			writeCodedError(w, http.StatusUnauthorized, "unauthorized", "authentication required")
			return
		}
		if !claims.IsService() {
			log.Warn().
				Str("principal", claims.PrincipalID()).
				Str("org_id", claims.OrgID).
				Str("request_id", chimw.GetReqID(r.Context())).
				Msg("processing policy: a user principal tried to change a policy; refused")
			writeCodedError(w, http.StatusForbidden, "operator_only", "a processing policy can only be changed by an operator service principal")
			return
		}
		next.ServeHTTP(w, r)
	})
}

type policyView struct {
	ContextualRetrieval  string     `json:"contextual_retrieval"`
	GraphExtraction      string     `json:"graph_extraction"`
	ContextPromptVersion *string    `json:"context_prompt_version"`
	BudgetMode           string     `json:"budget_mode"`
	MonthlyBudgetUSD     *float64   `json:"monthly_budget_usd"`
	Reason               *string    `json:"reason"`
	Source               string     `json:"source"`
	UpdatedBy            *string    `json:"updated_by"`
	UpdatedAt            *time.Time `json:"updated_at"`
}

func viewOf(r policy.Record) policyView {
	view := policyView{
		ContextualRetrieval:  r.ContextualRetrieval,
		GraphExtraction:      r.GraphExtraction,
		ContextPromptVersion: r.ContextPromptVersion,
		BudgetMode:           r.BudgetMode,
		MonthlyBudgetUSD:     r.MonthlyBudgetUSD,
		Reason:               r.Reason,
		Source:               "default",
	}
	if r.Exists {
		view.Source = "policy"
		updatedBy, updatedAt := r.UpdatedBy, r.UpdatedAt
		view.UpdatedBy, view.UpdatedAt = &updatedBy, &updatedAt
	}
	return view
}

type policyResponse struct {
	OrgID     string           `json:"org_id"`
	Policy    policyView       `json:"policy"`
	Effective policy.Effective `json:"effective"`
	Note      string           `json:"note"`
	Action    string           `json:"action,omitempty"`
	Previous  *policyView      `json:"previous,omitempty"`
}

const policyNote = "An absent policy means the system default, which is on. The global kill switch " +
	"(CONTEXTUAL_RETRIEVAL_ENABLED=false) overrides every organization value. Changes reach the embedding " +
	"engine within 30 seconds (its policy cache) and reach graph-index on its next pass (it reads the policy " +
	"before every extraction, with no cache). contextual_retrieval, context_prompt_version and graph_extraction " +
	"are enforced today: the budget fields accept only their defaults until the budget gate (CR-T9b) reads them."

func (h *QualityHandler) policyUnavailable(w http.ResponseWriter) bool {
	if h.policies == nil {
		writeCodedError(w, http.StatusServiceUnavailable, "policy_store_unavailable", "the processing policy store is not configured")
		return true
	}
	return false
}

// GetPolicy returns the organization's processing policy, or the system default.
func (h *QualityHandler) GetPolicy(w http.ResponseWriter, r *http.Request) {
	if h.policyUnavailable(w) {
		return
	}
	claims, ok := authctx.FromContext(r.Context())
	if !ok {
		writeCodedError(w, http.StatusUnauthorized, "unauthorized", "authentication required")
		return
	}
	orgID := claims.OrgID
	if chi.URLParam(r, "org") != orgID {
		writeCodedError(w, http.StatusForbidden, "tenant_mismatch", "the organization in the path does not match the verified identity")
		return
	}
	record, err := h.policies.Get(r.Context(), orgID)
	if err != nil {
		h.policyStoreFailure(w, err, orgID, "read")
		return
	}
	view := viewOf(record)
	if !claims.IsService() {
		// An organization's own admin may see how its data is processed, not an operator's internal
		// note about why or which operator wrote it.
		view.Reason, view.UpdatedBy = nil, nil
	}
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, policyResponse{
		OrgID:     orgID,
		Policy:    view,
		Effective: record.Effective(),
		Note:      policyNote,
	})
}

// maxRequestIDLength bounds what is stored in the audit trail from the client-supplied
// X-Request-Id header (chi's RequestID middleware keeps an incoming value as it is).
const maxRequestIDLength = 128

// sanitizeRequestID keeps a request id to printable ASCII of bounded length before it is audited.
func sanitizeRequestID(id string) string {
	out := make([]byte, 0, len(id))
	for i := 0; i < len(id) && len(out) < maxRequestIDLength; i++ {
		if c := id[i]; c > 0x20 && c < 0x7f {
			out = append(out, c)
		}
	}
	return string(out)
}

// putPolicyRequest has pointer fields so that a MISSING field is told apart from a
// field set to its default. The three switches are required: a PUT replaces the
// whole policy, and an omitted field must never lift a hold by accident.
type putPolicyRequest struct {
	ContextualRetrieval  *string  `json:"contextual_retrieval"`
	GraphExtraction      *string  `json:"graph_extraction"`
	ContextPromptVersion *string  `json:"context_prompt_version"`
	BudgetMode           *string  `json:"budget_mode"`
	MonthlyBudgetUSD     *float64 `json:"monthly_budget_usd"`
	Reason               *string  `json:"reason"`
}

const maxPolicyBodyBytes = 8 << 10

func decodePolicyRequest(w http.ResponseWriter, r *http.Request) (putPolicyRequest, error) {
	var req putPolicyRequest
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, maxPolicyBodyBytes))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&req); err != nil {
		return req, err
	}
	// Exactly one JSON value: trailing data is a malformed request, not extra input.
	if err := decoder.Decode(&struct{}{}); !errors.Is(err, io.EOF) {
		return req, errors.New("unexpected data after the JSON value")
	}
	return req, nil
}

// PutPolicy replaces the organization's processing policy. Operator-only (a
// service principal with data:quality:admin) and only for the token's own
// organization; the change and its audit row are one transaction.
func (h *QualityHandler) PutPolicy(w http.ResponseWriter, r *http.Request) {
	if h.policyUnavailable(w) {
		return
	}
	claims, ok := authctx.FromContext(r.Context())
	if !ok {
		writeCodedError(w, http.StatusUnauthorized, "unauthorized", "authentication required")
		return
	}
	// Defence in depth: the route's middleware already refused these, and the
	// handler does not rely on being mounted behind it.
	if !claims.IsService() {
		writeCodedError(w, http.StatusForbidden, "operator_only", "a processing policy can only be changed by an operator service principal")
		return
	}
	orgID := claims.OrgID
	if chi.URLParam(r, "org") != orgID {
		writeCodedError(w, http.StatusForbidden, "tenant_mismatch", "the organization in the path does not match the verified identity")
		return
	}

	req, err := decodePolicyRequest(w, r)
	if err != nil {
		writeCodedError(w, http.StatusBadRequest, "invalid_request", "the body must be one JSON object with only the documented fields")
		return
	}
	for _, required := range []struct {
		field string
		value *string
	}{
		{"contextual_retrieval", req.ContextualRetrieval},
		{"graph_extraction", req.GraphExtraction},
		{"budget_mode", req.BudgetMode},
	} {
		if required.value == nil {
			writeCodedError(w, http.StatusBadRequest, "invalid_request", required.field+" is required: a PUT replaces the whole policy, state every switch (inherit is the default)")
			return
		}
	}
	input, err := policy.Input{
		ContextualRetrieval:  *req.ContextualRetrieval,
		GraphExtraction:      *req.GraphExtraction,
		ContextPromptVersion: req.ContextPromptVersion,
		BudgetMode:           *req.BudgetMode,
		MonthlyBudgetUSD:     req.MonthlyBudgetUSD,
		Reason:               req.Reason,
	}.Validate()
	if err != nil {
		message := "the policy is not valid"
		if fe, isField := policy.IsFieldError(err); isField {
			message = fe.Error()
		}
		writeCodedError(w, http.StatusBadRequest, "invalid_request", message)
		return
	}

	result, err := h.policies.Put(r.Context(), orgID, input, policy.Actor{
		ID:        claims.PrincipalID(),
		Type:      claims.PrincipalType,
		RequestID: sanitizeRequestID(chimw.GetReqID(r.Context())),
	})
	if err != nil {
		h.policyStoreFailure(w, err, orgID, "write")
		return
	}
	log.Info().
		Str("org_id", orgID).
		Str("principal", claims.PrincipalID()).
		Str("action", result.Action).
		Str("contextual_retrieval", result.After.ContextualRetrieval).
		Str("graph_extraction", result.After.GraphExtraction).
		Str("budget_mode", result.After.BudgetMode).
		Str("request_id", chimw.GetReqID(r.Context())).
		Msg("processing policy changed")
	previous := viewOf(result.Before)
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, policyResponse{
		OrgID:     orgID,
		Policy:    viewOf(result.After),
		Effective: result.After.Effective(),
		Note:      policyNote,
		Action:    result.Action,
		Previous:  &previous,
	})
}

func (h *QualityHandler) policyStoreFailure(w http.ResponseWriter, err error, orgID, op string) {
	if errors.Is(err, policy.ErrNotMigrated) {
		log.Error().Err(err).Str("org_id", orgID).Msg("processing policy: the policy table is missing")
		writeCodedError(w, http.StatusServiceUnavailable, "policy_store_unavailable", "the processing policy table is not migrated in this database")
		return
	}
	log.Error().Err(err).Str("org_id", orgID).Str("op", op).Msg("processing policy: store failure")
	message := "the processing policy could not be read"
	if op == "write" {
		message = "the processing policy could not be written"
	}
	writeCodedError(w, http.StatusInternalServerError, "policy_store_failed", message)
}
