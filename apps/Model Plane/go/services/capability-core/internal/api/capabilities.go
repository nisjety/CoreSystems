// Package api — HTTP handlers for the capability-core public API.
//
// Mounted under /api/v1 on the :8085 health server.
// All write endpoints emit a registry_audit_log entry via the store.
package api

import (
	"encoding/json"
	"net/http"
	"strconv"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// CapabilitiesHandler provides REST endpoints for the capabilities table.
type CapabilitiesHandler struct {
	store             *registry.CapabilitiesStore
	availabilityStore availabilityStoreBackend
	scopes            *registry.ScopeStore // optional: enables scope grant/revoke/resolve
}

// NewCapabilitiesHandler constructs the handler.
func NewCapabilitiesHandler(store *registry.CapabilitiesStore) *CapabilitiesHandler {
	return &CapabilitiesHandler{store: store, availabilityStore: store}
}

// WithScopeStore attaches the durable scope-grant store, enabling the
// /scopes, /scopes/grant, /scopes/revoke, and /scopes/resolve endpoints and the
// /ranked listing. Returns the handler for chaining.
func (h *CapabilitiesHandler) WithScopeStore(s *registry.ScopeStore) *CapabilitiesHandler {
	h.scopes = s
	return h
}

// Register mounts routes on the provided mux under /api/v1/capabilities.
func (h *CapabilitiesHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/capabilities", h.list)
	mux.HandleFunc("/api/v1/capabilities/availability", h.attestAvailability)
	mux.HandleFunc("/api/v1/capabilities/ranked", h.ranked)
	mux.HandleFunc("/api/v1/capabilities/scopes", h.listScopes)
	mux.HandleFunc("/api/v1/capabilities/scopes/grant", h.grantScope)
	mux.HandleFunc("/api/v1/capabilities/scopes/revoke", h.revokeScope)
	mux.HandleFunc("/api/v1/capabilities/scopes/resolve", h.resolveScope)
	mux.HandleFunc("/api/v1/capabilities/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/capabilities/"):]
		switch {
		case r.Method == http.MethodGet && id != "":
			h.getByID(w, r, id)
		case r.Method == http.MethodDelete && id != "":
			h.softDelete(w, r, id)
		case r.Method == http.MethodPatch && id != "":
			h.patch(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
	mux.HandleFunc("/api/v1/capabilities/upsert", h.upsert)
	mux.HandleFunc("/api/v1/capabilities/rollout", h.setRollout)
	mux.HandleFunc("/api/v1/capabilities/audit", h.auditLog)
}

func (h *CapabilitiesHandler) list(w http.ResponseWriter, r *http.Request) {
	if r.Method == http.MethodPost {
		h.upsert(w, r)
		return
	}
	q := r.URL.Query()
	orgID := verifiedOrganizationID(r)
	kind := q.Get("kind")
	rollout := q.Get("rollout_state")
	onlyEnabled := q.Get("enabled") == "true"
	limit, _ := strconv.Atoi(q.Get("limit"))
	offset, _ := strconv.Atoi(q.Get("offset"))
	if limit == 0 {
		limit = 50
	}

	caps, err := h.store.List(r.Context(), orgID, kind, rollout, onlyEnabled, limit, offset)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	views := make([]map[string]any, 0, len(caps))
	for _, capability := range caps {
		views = append(views, capabilityWireView(capability))
	}
	writeJSON(w, map[string]any{"capabilities": views, "count": len(views)})
}

func (h *CapabilitiesHandler) getByID(w http.ResponseWriter, r *http.Request, id string) {
	cap, err := h.store.GetForOrg(r.Context(), id, verifiedOrganizationID(r))
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, capabilityWireView(cap))
}

func (h *CapabilitiesHandler) upsert(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost && r.Method != http.MethodPut {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var row registry.CapabilityRow
	if err := json.NewDecoder(r.Body).Decode(&row); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if !models.IsSupportedRiskLevel(row.RiskLevel) {
		jsonErr(w, "risk_level must be one of: low, medium, high", http.StatusBadRequest)
		return
	}
	if row.ID == "" {
		row.ID = "cap_" + uuid.New().String()
	}
	row.OrgID = verifiedOrganizationID(r)
	actor := verifiedActorID(r)
	row.CreatedBy = actor
	if err := h.store.Upsert(r.Context(), &row); err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	_ = h.store.AppendAuditLog(r.Context(), "capability", row.ID, "upserted", actor, row.OrgID, nil)
	w.WriteHeader(http.StatusCreated)
	writeJSON(w, map[string]any{"id": row.ID, "status": "ok"})
}

func (h *CapabilitiesHandler) softDelete(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodDelete {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	actor := verifiedActorID(r)
	orgID := verifiedOrganizationID(r)
	if err := h.store.SoftDeleteForOrg(r.Context(), id, orgID); err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	_ = h.store.AppendAuditLog(r.Context(), "capability", id, "deleted", actor, orgID, nil)
	w.WriteHeader(http.StatusNoContent)
}

func (h *CapabilitiesHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodPatch {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var update struct {
		Enabled      *bool  `json:"enabled"`
		RolloutState string `json:"rollout_state"`
		Description  string `json:"description"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if update.RolloutState != "" {
		actor := verifiedActorID(r)
		orgID := verifiedOrganizationID(r)
		if err := h.store.SetRolloutStateForOrg(r.Context(), id, orgID, update.RolloutState, actor); err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		_ = h.store.AppendAuditLog(r.Context(), "capability", id, "rollout_changed", actor, orgID, nil)
	}
	writeJSON(w, map[string]any{"id": id, "status": "updated"})
}

type rolloutRequest struct {
	ID    string `json:"id"`
	State string `json:"state"` // stable | canary | quarantine | deprecated
}

func (h *CapabilitiesHandler) setRollout(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var req rolloutRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	actor := verifiedActorID(r)
	orgID := verifiedOrganizationID(r)
	if err := h.store.SetRolloutStateForOrg(r.Context(), req.ID, orgID, req.State, actor); err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	_ = h.store.AppendAuditLog(r.Context(), "capability", req.ID, req.State, actor, orgID, nil)
	writeJSON(w, map[string]any{"id": req.ID, "rollout_state": req.State, "updated_at": time.Now().UTC()})
}

// auditLog handles GET /api/v1/capabilities/audit — registry audit entries
// newest-first, filterable by ?entity_kind=, ?entity_id=, and ?limit= (≤500).
func (h *CapabilitiesHandler) auditLog(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		w.Header().Set("Allow", http.MethodGet)
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	q := r.URL.Query()
	limit := 0
	if l := q.Get("limit"); l != "" {
		if n, err := strconv.Atoi(l); err == nil {
			limit = n
		}
	}
	entries, err := h.store.QueryAuditLogForOrg(r.Context(), verifiedOrganizationID(r), q.Get("entity_kind"), q.Get("entity_id"), limit)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"entries": entries, "count": len(entries)})
}

// ranked handles GET /api/v1/capabilities/ranked — capabilities ordered by
// descending composite score. Filters: ?org_id= ?kind= ?limit=.
func (h *CapabilitiesHandler) ranked(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		w.Header().Set("Allow", http.MethodGet)
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	q := r.URL.Query()
	limit, _ := strconv.Atoi(q.Get("limit"))
	scored, err := h.store.RankedList(r.Context(), verifiedOrganizationID(r), q.Get("kind"), nil, limit)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	views := make([]map[string]any, 0, len(scored))
	for _, item := range scored {
		views = append(views, map[string]any{
			"capability": capabilityWireView(item.Row),
			"score":      item.Score,
		})
	}
	writeJSON(w, map[string]any{"capabilities": views, "count": len(views)})
}

// listScopes handles GET /api/v1/capabilities/scopes?capability_id= — active
// grants for one capability.
func (h *CapabilitiesHandler) listScopes(w http.ResponseWriter, r *http.Request) {
	if h.scopes == nil {
		jsonErr(w, "scope store not configured", http.StatusServiceUnavailable)
		return
	}
	if r.Method != http.MethodGet {
		w.Header().Set("Allow", http.MethodGet)
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	capID := r.URL.Query().Get("capability_id")
	if capID == "" {
		jsonErr(w, "capability_id is required", http.StatusBadRequest)
		return
	}
	if _, err := h.store.GetForOrg(r.Context(), capID, verifiedOrganizationID(r)); err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	grants, err := h.scopes.ListForCapabilityForOrg(r.Context(), capID, verifiedOrganizationID(r))
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"grants": grants, "count": len(grants)})
}

type scopeGrantRequest struct {
	CapabilityID string `json:"capability_id"`
	ScopeKind    string `json:"scope_kind"`  // org | agent | workspace | user | global
	ScopeValue   string `json:"scope_value"` // specific id, or "*" / "" for all
}

// grantScope handles POST /api/v1/capabilities/scopes/grant.
func (h *CapabilitiesHandler) grantScope(w http.ResponseWriter, r *http.Request) {
	if h.scopes == nil {
		jsonErr(w, "scope store not configured", http.StatusServiceUnavailable)
		return
	}
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var req scopeGrantRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	orgID := verifiedOrganizationID(r)
	if _, err := h.store.GetForOrg(r.Context(), req.CapabilityID, orgID); err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if req.ScopeKind != registry.ScopeKindOrg {
		jsonErr(w, "only tenant-derived org grants are supported", http.StatusForbidden)
		return
	}
	req.ScopeValue = orgID
	actor := verifiedActorID(r)
	grant, err := h.scopes.Grant(r.Context(), "", orgID, req.CapabilityID, req.ScopeKind, req.ScopeValue, actor)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	_ = h.store.AppendAuditLog(r.Context(), "capability_scope", req.CapabilityID, "granted", actor, orgID, scopeDiff(grant.ScopeKind, grant.ScopeValue))
	w.WriteHeader(http.StatusCreated)
	writeJSON(w, grant)
}

// revokeScope handles POST /api/v1/capabilities/scopes/revoke.
func (h *CapabilitiesHandler) revokeScope(w http.ResponseWriter, r *http.Request) {
	if h.scopes == nil {
		jsonErr(w, "scope store not configured", http.StatusServiceUnavailable)
		return
	}
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var req scopeGrantRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	orgID := verifiedOrganizationID(r)
	if _, err := h.store.GetForOrg(r.Context(), req.CapabilityID, orgID); err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if req.ScopeKind != registry.ScopeKindOrg {
		jsonErr(w, "only tenant-derived org grants are supported", http.StatusForbidden)
		return
	}
	req.ScopeValue = orgID
	actor := verifiedActorID(r)
	n, err := h.scopes.Revoke(r.Context(), orgID, req.CapabilityID, req.ScopeKind, req.ScopeValue)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	_ = h.store.AppendAuditLog(r.Context(), "capability_scope", req.CapabilityID, "revoked", actor, orgID, scopeDiff(req.ScopeKind, req.ScopeValue))
	writeJSON(w, map[string]any{"capability_id": req.CapabilityID, "revoked": n})
}

// resolveScope handles GET /api/v1/capabilities/scopes/resolve?scope_kind=&scope_value=
// — the capability IDs granted for an (org/agent/...) tuple.
func (h *CapabilitiesHandler) resolveScope(w http.ResponseWriter, r *http.Request) {
	if h.scopes == nil {
		jsonErr(w, "scope store not configured", http.StatusServiceUnavailable)
		return
	}
	if r.Method != http.MethodGet {
		w.Header().Set("Allow", http.MethodGet)
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	q := r.URL.Query()
	kind := q.Get("scope_kind")
	if kind == "" {
		jsonErr(w, "scope_kind is required", http.StatusBadRequest)
		return
	}
	if kind != registry.ScopeKindOrg {
		jsonErr(w, "only tenant-derived org scope resolution is supported", http.StatusForbidden)
		return
	}
	value := verifiedOrganizationID(r)
	ids, err := h.scopes.ResolveForScopeForOrg(r.Context(), verifiedOrganizationID(r), kind, value)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"capability_ids": ids, "count": len(ids)})
}

func scopeDiff(kind, value string) []byte {
	b, _ := json.Marshal(map[string]string{"scope_kind": kind, "scope_value": value})
	return b
}

// -- helpers ------------------------------------------------------------------

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(v)
}

func jsonErr(w http.ResponseWriter, msg string, code int) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(map[string]string{"error": msg})
}
