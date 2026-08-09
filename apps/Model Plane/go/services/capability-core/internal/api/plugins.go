package api

import (
	"encoding/json"
	"log/slog"
	"net/http"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

// PluginsHandler — HTTP CRUD for the durable per-org plugin registry
// (`plugin_packages`). A plugin package is a manifest that can contribute tools,
// skills, and hooks; this handler is the system-of-record. Registering a plugin
// is inert until a host loads its manifest — see the package doc in registry_apis.go.
//
// Mirrors SkillsHandler/MCPHandler: org-scoped, reconcile-event on mutation,
// soft-delete (plugin_packages carries deleted_at + a unique (org,name,version)
// index scoped to live rows). Plugins default disabled+unpinned (safe rollout).
type PluginsHandler struct {
	pool registryDatabase
	pub  publisher.EventPublisher
}

// NewPluginsHandler constructs the handler.
func NewPluginsHandler(pool registryDatabase) *PluginsHandler {
	return &PluginsHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional, nil-safe.
func (h *PluginsHandler) WithPublisher(pub publisher.EventPublisher) *PluginsHandler {
	h.pub = pub
	return h
}

// Register mounts routes.
func (h *PluginsHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/plugins", h.listOrCreate)
	mux.HandleFunc("/api/v1/plugins/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/plugins/"):]
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, id)
		case http.MethodPatch:
			h.update(w, r, id)
		case http.MethodDelete:
			h.delete(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
}

type pluginRow struct {
	ID           string          `json:"id"`
	OrgID        string          `json:"org_id"`
	Name         string          `json:"name"`
	Version      string          `json:"version"`
	Description  string          `json:"description"`
	ManifestJSON json.RawMessage `json:"manifest_json"`
	RiskLevel    string          `json:"risk_level"`
	Enabled      bool            `json:"enabled"`
	Pinned       bool            `json:"pinned"`
	RolloutState string          `json:"rollout_state"`
	CreatedAt    time.Time       `json:"created_at"`
	UpdatedAt    time.Time       `json:"updated_at"`
}

func (h *PluginsHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := verifiedOrganizationID(r)
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, version, description, manifest_json,
			       risk_level, enabled, pinned, rollout_state, created_at, updated_at
			FROM plugin_packages
			WHERE (org_id = $1 OR org_id = 'global') AND deleted_at IS NULL
			ORDER BY name, version
		`, orgID)
		if err != nil {
			slog.Error("list plugins failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		plugins := []pluginRow{}
		for rows.Next() {
			var p pluginRow
			if err := rows.Scan(&p.ID, &p.OrgID, &p.Name, &p.Version, &p.Description,
				&p.ManifestJSON, &p.RiskLevel, &p.Enabled, &p.Pinned, &p.RolloutState,
				&p.CreatedAt, &p.UpdatedAt); err != nil {
				slog.Error("scan plugin failed", "error", err)
				jsonErr(w, "database unavailable", http.StatusInternalServerError)
				return
			}
			plugins = append(plugins, p)
		}
		writeJSON(w, map[string]any{"plugins": plugins})
	case http.MethodPost:
		var p pluginRow
		if err := json.NewDecoder(r.Body).Decode(&p); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		if p.Name == "" || p.Version == "" {
			jsonErr(w, "name and version are required", http.StatusBadRequest)
			return
		}
		if len(p.ManifestJSON) == 0 {
			p.ManifestJSON = json.RawMessage("{}")
		}
		if p.RiskLevel == "" {
			p.RiskLevel = "high"
		}
		if p.RolloutState == "" {
			p.RolloutState = "canary"
		}
		p.OrgID = verifiedOrganizationID(r)
		p.ID = "plugin_" + uuid.New().String()
		now := time.Now().UTC()
		// New plugins default DISABLED (safe rollout): a plugin is inert until an
		// admin explicitly enables (and a host loads) it.
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO plugin_packages (id, org_id, name, version, description,
			    manifest_json, risk_level, enabled, pinned, rollout_state,
			    created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
		`, p.ID, p.OrgID, p.Name, p.Version, p.Description, p.ManifestJSON,
			p.RiskLevel, false, false, p.RolloutState, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin,
			reconcile.ActionRegistered, p.ID, p.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", p.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": p.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *PluginsHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var p pluginRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, version, description, manifest_json,
		       risk_level, enabled, pinned, rollout_state, created_at, updated_at
		FROM plugin_packages WHERE id = $1 AND org_id = $2 AND deleted_at IS NULL
	`, id, verifiedOrganizationID(r)).Scan(&p.ID, &p.OrgID, &p.Name, &p.Version,
		&p.Description, &p.ManifestJSON, &p.RiskLevel, &p.Enabled, &p.Pinned,
		&p.RolloutState, &p.CreatedAt, &p.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, p)
}

func (h *PluginsHandler) update(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled      *bool  `json:"enabled"`
		Pinned       *bool  `json:"pinned"`
		Description  string `json:"description"`
		RolloutState string `json:"rollout_state"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if update.Enabled == nil && update.Pinned == nil && update.Description == "" && update.RolloutState == "" {
		jsonErr(w, "at least one plugin field is required", http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	orgID := verifiedOrganizationID(r)
	var enabled any
	if update.Enabled != nil {
		enabled = *update.Enabled
	}
	var pinned any
	if update.Pinned != nil {
		pinned = *update.Pinned
	}
	result, err := h.pool.Exec(r.Context(), `
		UPDATE plugin_packages
		SET enabled=COALESCE($1, enabled), pinned=COALESCE($2, pinned),
			description=COALESCE(NULLIF($3, ''), description),
			rollout_state=COALESCE(NULLIF($4, ''), rollout_state), updated_at=$5
		WHERE id=$6 AND org_id=$7 AND deleted_at IS NULL
	`, enabled, pinned, update.Description, update.RolloutState, now, id, orgID)
	if !writeSingleScopedMutation(w, "plugin package", result, err) {
		return
	}
	if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin,
		reconcile.ActionUpdated, id, orgID); eerr != nil {
		slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", id, "error", eerr)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *PluginsHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	orgID := verifiedOrganizationID(r)
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(),
		`UPDATE plugin_packages SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL`,
		now, id, orgID)
	if !writeSingleScopedMutation(w, "plugin package", result, err) {
		return
	}
	if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin,
		reconcile.ActionRemoved, id, orgID); eerr != nil {
		slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", id, "error", eerr)
	}
	w.WriteHeader(http.StatusNoContent)
}
