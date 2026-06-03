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

	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// CapabilitiesHandler provides REST endpoints for the capabilities table.
type CapabilitiesHandler struct {
	store *registry.CapabilitiesStore
}

// NewCapabilitiesHandler constructs the handler.
func NewCapabilitiesHandler(store *registry.CapabilitiesStore) *CapabilitiesHandler {
	return &CapabilitiesHandler{store: store}
}

// Register mounts routes on the provided mux under /api/v1/capabilities.
func (h *CapabilitiesHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/capabilities", h.list)
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
	orgID := q.Get("org_id")
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
	writeJSON(w, map[string]any{"capabilities": caps, "count": len(caps)})
}

func (h *CapabilitiesHandler) getByID(w http.ResponseWriter, r *http.Request, id string) {
	cap, err := h.store.Get(r.Context(), id)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, cap)
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
	if row.ID == "" {
		row.ID = "cap_" + uuid.New().String()
	}
	actor := r.Header.Get("X-Actor")
	if actor == "" {
		actor = "api"
	}
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
	actor := r.Header.Get("X-Actor")
	if actor == "" {
		actor = "api"
	}
	if err := h.store.SoftDelete(r.Context(), id); err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	_ = h.store.AppendAuditLog(r.Context(), "capability", id, "deleted", actor, "", nil)
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
		actor := r.Header.Get("X-Actor")
		if err := h.store.SetRolloutState(r.Context(), id, update.RolloutState, actor); err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		_ = h.store.AppendAuditLog(r.Context(), "capability", id, "rollout_changed", actor, "", nil)
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
	actor := r.Header.Get("X-Actor")
	if err := h.store.SetRolloutState(r.Context(), req.ID, req.State, actor); err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	_ = h.store.AppendAuditLog(r.Context(), "capability", req.ID, req.State, actor, "", nil)
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
	entries, err := h.store.QueryAuditLog(r.Context(), q.Get("entity_kind"), q.Get("entity_id"), limit)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"entries": entries, "count": len(entries)})
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
