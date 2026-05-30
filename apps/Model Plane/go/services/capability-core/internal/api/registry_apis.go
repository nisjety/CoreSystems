// Package api — HTTP handlers for skills, MCP servers, plugins, routing, and safety.
package api

import (
	"encoding/json"
	"log/slog"
	"net/http"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

// ---------------------------------------------------------------------------
// SkillsHandler  /api/v1/skills
// ---------------------------------------------------------------------------

// SkillsHandler handles CRUD for agent_skills.
type SkillsHandler struct {
	pool *pgxpool.Pool
	pub  publisher.EventPublisher
}

// NewSkillsHandler constructs the handler.
func NewSkillsHandler(pool *pgxpool.Pool) *SkillsHandler {
	return &SkillsHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional, nil-safe.
func (h *SkillsHandler) WithPublisher(pub publisher.EventPublisher) *SkillsHandler {
	h.pub = pub
	return h
}

// Register mounts routes.
func (h *SkillsHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/skills", h.listOrCreate)
	mux.HandleFunc("/api/v1/skills/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/skills/"):]
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

type skillRow struct {
	ID                  string    `json:"id"`
	OrgID               string    `json:"org_id"`
	Name                string    `json:"name"`
	Description         string    `json:"description"`
	Content             string    `json:"content"`
	TriggerKeywords     []string  `json:"trigger_keywords"`
	TriggerFilePatterns []string  `json:"trigger_file_patterns"`
	ToolRestrictions    []string  `json:"tool_restrictions"`
	Enabled             bool      `json:"enabled"`
	CreatedAt           time.Time `json:"created_at"`
	UpdatedAt           time.Time `json:"updated_at"`
}

func (h *SkillsHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := r.URL.Query().Get("org_id")
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, content,
			       trigger_keywords, trigger_file_patterns, tool_restrictions,
			       enabled, created_at, updated_at
			FROM agent_skills WHERE org_id = $1 ORDER BY name
		`, orgID)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var skills []skillRow
		for rows.Next() {
			var s skillRow
			if err := rows.Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.Content,
				&s.TriggerKeywords, &s.TriggerFilePatterns, &s.ToolRestrictions,
				&s.Enabled, &s.CreatedAt, &s.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			skills = append(skills, s)
		}
		writeJSON(w, map[string]any{"skills": skills})
	case http.MethodPost:
		var s skillRow
		if err := json.NewDecoder(r.Body).Decode(&s); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		s.ID = "skill_" + uuid.New().String()
		now := time.Now().UTC()
		if s.TriggerKeywords == nil {
			s.TriggerKeywords = []string{}
		}
		if s.TriggerFilePatterns == nil {
			s.TriggerFilePatterns = []string{}
		}
		if s.ToolRestrictions == nil {
			s.ToolRestrictions = []string{}
		}
		kw, _ := json.Marshal(s.TriggerKeywords)
		fp, _ := json.Marshal(s.TriggerFilePatterns)
		tr, _ := json.Marshal(s.ToolRestrictions)
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO agent_skills (id, org_id, name, description, content,
			    trigger_keywords, trigger_file_patterns, tool_restrictions,
			    enabled, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		`, s.ID, s.OrgID, s.Name, s.Description, s.Content, kw, fp, tr, s.Enabled, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindSkill,
			reconcile.ActionRegistered, s.ID, s.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindSkill, "id", s.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": s.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *SkillsHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var s skillRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, content,
		       trigger_keywords, trigger_file_patterns, tool_restrictions,
		       enabled, created_at, updated_at
		FROM agent_skills WHERE id = $1
	`, id).Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.Content,
		&s.TriggerKeywords, &s.TriggerFilePatterns, &s.ToolRestrictions,
		&s.Enabled, &s.CreatedAt, &s.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, s)
}

func (h *SkillsHandler) update(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled     *bool  `json:"enabled"`
		Description string `json:"description"`
		Content     string `json:"content"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	if update.Enabled != nil {
		_, _ = h.pool.Exec(r.Context(), `UPDATE agent_skills SET enabled=$1, updated_at=$2 WHERE id=$3`, *update.Enabled, now, id)
	}
	if update.Description != "" {
		_, _ = h.pool.Exec(r.Context(), `UPDATE agent_skills SET description=$1, updated_at=$2 WHERE id=$3`, update.Description, now, id)
	}
	if update.Content != "" {
		_, _ = h.pool.Exec(r.Context(), `UPDATE agent_skills SET content=$1, updated_at=$2 WHERE id=$3`, update.Content, now, id)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *SkillsHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	_, err := h.pool.Exec(r.Context(), `DELETE FROM agent_skills WHERE id=$1`, id)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// ---------------------------------------------------------------------------
// MCPHandler  /api/v1/mcp
// ---------------------------------------------------------------------------

// MCPHandler handles CRUD for mcp_servers.
type MCPHandler struct {
	pool *pgxpool.Pool
	pub  publisher.EventPublisher
}

// NewMCPHandler constructs the handler.
func NewMCPHandler(pool *pgxpool.Pool) *MCPHandler {
	return &MCPHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional and
// nil-safe: without it, mutations simply don't emit and the gateway falls back
// to its cache TTL. Chainable: NewMCPHandler(pool).WithPublisher(pub).Register(mux).
func (h *MCPHandler) WithPublisher(pub publisher.EventPublisher) *MCPHandler {
	h.pub = pub
	return h
}

type mcpServerRow struct {
	ID           string    `json:"id"`
	OrgID        string    `json:"org_id"`
	Name         string    `json:"name"`
	Description  string    `json:"description"`
	EndpointURL  string    `json:"endpoint_url"`
	Transport    string    `json:"transport"`
	AuthKind     string    `json:"auth_kind"`
	ConfigJSON   any       `json:"config_json"`
	Scope        string    `json:"scope"`
	Enabled      bool      `json:"enabled"`
	RolloutState string    `json:"rollout_state"`
	RiskLevel    string    `json:"risk_level"`
	CreatedAt    time.Time `json:"created_at"`
	UpdatedAt    time.Time `json:"updated_at"`
}

// Register mounts routes.
func (h *MCPHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/mcp", h.listOrCreate)
	mux.HandleFunc("/api/v1/mcp/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/mcp/"):]
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, id)
		case http.MethodPatch:
			h.patch(w, r, id)
		case http.MethodDelete:
			h.delete(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
}

func (h *MCPHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := r.URL.Query().Get("org_id")
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, endpoint_url, transport, auth_kind,
			       config_json, scope, enabled, rollout_state, risk_level, created_at, updated_at
			FROM mcp_servers WHERE (org_id=$1 OR org_id='global') AND deleted_at IS NULL ORDER BY name
		`, orgID)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var servers []mcpServerRow
		for rows.Next() {
			var s mcpServerRow
			if err := rows.Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.EndpointURL,
				&s.Transport, &s.AuthKind, &s.ConfigJSON, &s.Scope, &s.Enabled,
				&s.RolloutState, &s.RiskLevel, &s.CreatedAt, &s.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			servers = append(servers, s)
		}
		writeJSON(w, map[string]any{"servers": servers})
	case http.MethodPost, http.MethodPut:
		var s mcpServerRow
		if err := json.NewDecoder(r.Body).Decode(&s); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		if s.ID == "" {
			s.ID = "mcp_" + uuid.New().String()
		}
		if s.Transport == "" {
			s.Transport = "http"
		}
		if s.RolloutState == "" {
			s.RolloutState = "stable"
		}
		if s.RiskLevel == "" {
			s.RiskLevel = "medium"
		}
		now := time.Now().UTC()
		cfgJSON, _ := json.Marshal(s.ConfigJSON)
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO mcp_servers (id, org_id, name, description, endpoint_url, transport, auth_kind,
			    config_json, scope, enabled, rollout_state, risk_level, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
			ON CONFLICT (org_id, name) WHERE deleted_at IS NULL DO UPDATE SET
			    endpoint_url=$5, transport=$6, config_json=$8, enabled=$10,
			    rollout_state=$11, risk_level=$12, updated_at=$14
		`, s.ID, s.OrgID, s.Name, s.Description, s.EndpointURL, s.Transport, s.AuthKind,
			cfgJSON, s.Scope, s.Enabled, s.RolloutState, s.RiskLevel, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		// Reconcile (matrix §4.3): notify cache holders an MCP server changed.
		// Best-effort — never block the mutation on event emission.
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindMCPServer,
			reconcile.ActionRegistered, s.ID, s.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindMCPServer, "id", s.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": s.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *MCPHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var s mcpServerRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, endpoint_url, transport, auth_kind,
		       config_json, scope, enabled, rollout_state, risk_level, created_at, updated_at
		FROM mcp_servers WHERE id=$1 AND deleted_at IS NULL
	`, id).Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.EndpointURL,
		&s.Transport, &s.AuthKind, &s.ConfigJSON, &s.Scope, &s.Enabled,
		&s.RolloutState, &s.RiskLevel, &s.CreatedAt, &s.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, s)
}

func (h *MCPHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled      *bool  `json:"enabled"`
		RolloutState string `json:"rollout_state"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	if update.Enabled != nil {
		_, _ = h.pool.Exec(r.Context(), `UPDATE mcp_servers SET enabled=$1, updated_at=$2 WHERE id=$3`, *update.Enabled, now, id)
	}
	if update.RolloutState != "" {
		_, _ = h.pool.Exec(r.Context(), `UPDATE mcp_servers SET rollout_state=$1, updated_at=$2 WHERE id=$3`, update.RolloutState, now, id)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *MCPHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	now := time.Now().UTC()
	_, err := h.pool.Exec(r.Context(), `UPDATE mcp_servers SET deleted_at=$1, updated_at=$1 WHERE id=$2`, now, id)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// ---------------------------------------------------------------------------
// RoutingHandler  /api/v1/routing
// ---------------------------------------------------------------------------

// RoutingHandler handles CRUD for routing_policies.
type RoutingHandler struct {
	pool *pgxpool.Pool
	pub  publisher.EventPublisher
}

// NewRoutingHandler constructs the handler.
func NewRoutingHandler(pool *pgxpool.Pool) *RoutingHandler {
	return &RoutingHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional, nil-safe.
func (h *RoutingHandler) WithPublisher(pub publisher.EventPublisher) *RoutingHandler {
	h.pub = pub
	return h
}

// Register mounts routes.
func (h *RoutingHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/routing", h.listOrCreate)
	mux.HandleFunc("/api/v1/routing/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/routing/"):]
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, id)
		case http.MethodPatch:
			h.patch(w, r, id)
		case http.MethodDelete:
			h.delete(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
}

type routingPolicyRow struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	Name        string    `json:"name"`
	Description string    `json:"description"`
	Strategy    string    `json:"strategy"`
	ConfigJSON  any       `json:"config_json"`
	ModelIDs    []string  `json:"model_ids"`
	Priority    int       `json:"priority"`
	Enabled     bool      `json:"enabled"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

func (h *RoutingHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := r.URL.Query().Get("org_id")
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, strategy, config_json, model_ids,
			       priority, enabled, created_at, updated_at
			FROM routing_policies WHERE (org_id=$1 OR org_id='global') AND deleted_at IS NULL
			ORDER BY priority DESC, name
		`, orgID)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var policies []routingPolicyRow
		for rows.Next() {
			var p routingPolicyRow
			if err := rows.Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Strategy,
				&p.ConfigJSON, &p.ModelIDs, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			policies = append(policies, p)
		}
		writeJSON(w, map[string]any{"policies": policies})
	case http.MethodPost:
		var p routingPolicyRow
		if err := json.NewDecoder(r.Body).Decode(&p); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		if p.ID == "" {
			p.ID = "rp_" + uuid.New().String()
		}
		now := time.Now().UTC()
		cfgJSON, _ := json.Marshal(p.ConfigJSON)
		if p.ModelIDs == nil {
			p.ModelIDs = []string{}
		}
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO routing_policies (id, org_id, name, description, strategy, config_json, model_ids,
			    priority, enabled, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		`, p.ID, p.OrgID, p.Name, p.Description, p.Strategy, cfgJSON, p.ModelIDs,
			p.Priority, p.Enabled, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindRoutingPolicy,
			reconcile.ActionRegistered, p.ID, p.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindRoutingPolicy, "id", p.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": p.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *RoutingHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var p routingPolicyRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, strategy, config_json, model_ids,
		       priority, enabled, created_at, updated_at
		FROM routing_policies WHERE id=$1 AND deleted_at IS NULL
	`, id).Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Strategy,
		&p.ConfigJSON, &p.ModelIDs, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, p)
}

func (h *RoutingHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled  *bool `json:"enabled"`
		Priority *int  `json:"priority"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	if update.Enabled != nil {
		_, _ = h.pool.Exec(r.Context(), `UPDATE routing_policies SET enabled=$1, updated_at=$2 WHERE id=$3`, *update.Enabled, now, id)
	}
	if update.Priority != nil {
		_, _ = h.pool.Exec(r.Context(), `UPDATE routing_policies SET priority=$1, updated_at=$2 WHERE id=$3`, *update.Priority, now, id)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *RoutingHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	now := time.Now().UTC()
	_, _ = h.pool.Exec(r.Context(), `UPDATE routing_policies SET deleted_at=$1, updated_at=$1 WHERE id=$2`, now, id)
	w.WriteHeader(http.StatusNoContent)
}

// ---------------------------------------------------------------------------
// SafetyHandler  /api/v1/safety
// ---------------------------------------------------------------------------

// SafetyHandler handles CRUD for safety_policies.
type SafetyHandler struct {
	pool *pgxpool.Pool
	pub  publisher.EventPublisher
}

// NewSafetyHandler constructs the handler.
func NewSafetyHandler(pool *pgxpool.Pool) *SafetyHandler {
	return &SafetyHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional, nil-safe.
func (h *SafetyHandler) WithPublisher(pub publisher.EventPublisher) *SafetyHandler {
	h.pub = pub
	return h
}

// Register mounts routes.
func (h *SafetyHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/safety", h.listOrCreate)
	mux.HandleFunc("/api/v1/safety/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/safety/"):]
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, id)
		case http.MethodPatch:
			h.patch(w, r, id)
		case http.MethodDelete:
			h.delete(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
}

type safetyPolicyRow struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	Name        string    `json:"name"`
	Description string    `json:"description"`
	Kind        string    `json:"kind"`
	ConfigJSON  any       `json:"config_json"`
	AppliesTo   []string  `json:"applies_to"`
	Priority    int       `json:"priority"`
	Enabled     bool      `json:"enabled"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

func (h *SafetyHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := r.URL.Query().Get("org_id")
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, kind, config_json, applies_to,
			       priority, enabled, created_at, updated_at
			FROM safety_policies WHERE (org_id=$1 OR org_id='global') AND deleted_at IS NULL
			ORDER BY priority DESC, name
		`, orgID)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var policies []safetyPolicyRow
		for rows.Next() {
			var p safetyPolicyRow
			if err := rows.Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Kind,
				&p.ConfigJSON, &p.AppliesTo, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			policies = append(policies, p)
		}
		writeJSON(w, map[string]any{"policies": policies})
	case http.MethodPost:
		var p safetyPolicyRow
		if err := json.NewDecoder(r.Body).Decode(&p); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		if p.ID == "" {
			p.ID = "sp_" + uuid.New().String()
		}
		now := time.Now().UTC()
		cfgJSON, _ := json.Marshal(p.ConfigJSON)
		if p.AppliesTo == nil {
			p.AppliesTo = []string{}
		}
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO safety_policies (id, org_id, name, description, kind, config_json, applies_to,
			    priority, enabled, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		`, p.ID, p.OrgID, p.Name, p.Description, p.Kind, cfgJSON, p.AppliesTo,
			p.Priority, p.Enabled, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindSafetyPolicy,
			reconcile.ActionRegistered, p.ID, p.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindSafetyPolicy, "id", p.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": p.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *SafetyHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var p safetyPolicyRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, kind, config_json, applies_to,
		       priority, enabled, created_at, updated_at
		FROM safety_policies WHERE id=$1 AND deleted_at IS NULL
	`, id).Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Kind,
		&p.ConfigJSON, &p.AppliesTo, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, p)
}

func (h *SafetyHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled  *bool `json:"enabled"`
		Priority *int  `json:"priority"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	if update.Enabled != nil {
		_, _ = h.pool.Exec(r.Context(), `UPDATE safety_policies SET enabled=$1, updated_at=$2 WHERE id=$3`, *update.Enabled, now, id)
	}
	if update.Priority != nil {
		_, _ = h.pool.Exec(r.Context(), `UPDATE safety_policies SET priority=$1, updated_at=$2 WHERE id=$3`, *update.Priority, now, id)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *SafetyHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	now := time.Now().UTC()
	_, _ = h.pool.Exec(r.Context(), `UPDATE safety_policies SET deleted_at=$1, updated_at=$1 WHERE id=$2`, now, id)
	w.WriteHeader(http.StatusNoContent)
}
