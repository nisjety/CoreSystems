// Package api — HTTP handlers for agent memory, tasks, and cron schedules.
package api

import (
	"encoding/json"
	"net/http"
	"strconv"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ---------------------------------------------------------------------------
// MemoryHandler  /api/v1/memory
// ---------------------------------------------------------------------------

// MemoryHandler handles CRUD for agent_memory.
type MemoryHandler struct {
	pool *pgxpool.Pool
}

// NewMemoryHandler constructs the handler.
func NewMemoryHandler(pool *pgxpool.Pool) *MemoryHandler {
	return &MemoryHandler{pool: pool}
}

// scopePrecedence defines the evaluation order for multiscope memory resolution.
// Lower index = higher precedence (narrower scope wins).
var scopePrecedence = []string{"run", "thread", "workspace", "user", "org", "global"}

// Register mounts routes.
func (h *MemoryHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/memory", h.listOrCreate)
	mux.HandleFunc("/api/v1/memory/resolve", h.resolve)
	mux.HandleFunc("/api/v1/memory/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/memory/"):]
		if id == "resolve" {
			h.resolve(w, r)
			return
		}
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

type memoryEntry struct {
	ID             string     `json:"id"`
	OrgID          string     `json:"org_id"`
	SessionID      *string    `json:"session_id,omitempty"`
	Scope          string     `json:"scope"`
	Key            string     `json:"key"`
	Content        string     `json:"content"`
	Kind           string     `json:"kind"`
	Confidence     float64    `json:"confidence"`
	Owner          string     `json:"owner"`
	SourceLinks    []string   `json:"source_links"`
	ReviewState    string     `json:"review_state"`
	Classification string     `json:"classification"`
	ExpiresAt      *time.Time `json:"expires_at,omitempty"`
	CreatedAt      time.Time  `json:"created_at"`
	UpdatedAt      time.Time  `json:"updated_at"`
}

func (h *MemoryHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		q := r.URL.Query()
		orgID := q.Get("org_id")
		sessionID := q.Get("session_id")
		scope := q.Get("scope")
		limit, _ := strconv.Atoi(q.Get("limit"))
		if limit == 0 {
			limit = 50
		}
		var rows interface{ Scan(...any) error }
		var err error
		if sessionID != "" {
			rows, err = h.pool.Query(r.Context(), `
				SELECT id, org_id, session_id, scope, key, content, kind, confidence,
				       owner, source_links, review_state, classification, expires_at,
				       created_at, updated_at
				FROM agent_memory
				WHERE org_id=$1 AND (session_id IS NULL OR session_id=$2)
				ORDER BY created_at LIMIT $3
			`, orgID, sessionID, limit)
		} else if scope != "" {
			rows, err = h.pool.Query(r.Context(), `
				SELECT id, org_id, session_id, scope, key, content, kind, confidence,
				       owner, source_links, review_state, classification, expires_at,
				       created_at, updated_at
				FROM agent_memory
				WHERE org_id=$1 AND scope=$2
				ORDER BY created_at LIMIT $3
			`, orgID, scope, limit)
		} else {
			rows, err = h.pool.Query(r.Context(), `
				SELECT id, org_id, session_id, scope, key, content, kind, confidence,
				       owner, source_links, review_state, classification, expires_at,
				       created_at, updated_at
				FROM agent_memory
				WHERE org_id=$1
				ORDER BY created_at LIMIT $2
			`, orgID, limit)
		}
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		// rows is pgx.Rows; iterate
		type pgxRows interface {
			Next() bool
			Scan(...any) error
			Close()
			Err() error
		}
		pgRows, ok := rows.(pgxRows)
		if !ok {
			jsonErr(w, "internal", http.StatusInternalServerError)
			return
		}
		defer pgRows.Close()
		var entries []memoryEntry
		for pgRows.Next() {
			var e memoryEntry
			if err := pgRows.Scan(&e.ID, &e.OrgID, &e.SessionID, &e.Scope, &e.Key, &e.Content,
				&e.Kind, &e.Confidence, &e.Owner, &e.SourceLinks, &e.ReviewState,
				&e.Classification, &e.ExpiresAt, &e.CreatedAt, &e.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			entries = append(entries, e)
		}
		writeJSON(w, map[string]any{"entries": entries, "count": len(entries)})
	case http.MethodPost:
		var e memoryEntry
		if err := json.NewDecoder(r.Body).Decode(&e); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		e.ID = "mem_" + uuid.New().String()
		if e.Scope == "" {
			e.Scope = "org"
		}
		if e.Kind == "" {
			e.Kind = "fact"
		}
		if e.Confidence == 0 {
			e.Confidence = 1.0
		}
		if e.ReviewState == "" {
			e.ReviewState = "accepted"
		}
		if e.SourceLinks == nil {
			e.SourceLinks = []string{}
		}
		now := time.Now().UTC()
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, kind,
			    confidence, owner, source_links, review_state, classification, expires_at,
			    created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
		`, e.ID, e.OrgID, e.SessionID, e.Scope, e.Key, e.Content, e.Kind,
			e.Confidence, e.Owner, e.SourceLinks, e.ReviewState, e.Classification,
			e.ExpiresAt, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": e.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *MemoryHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var e memoryEntry
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, session_id, scope, key, content, kind, confidence,
		       owner, source_links, review_state, classification, expires_at,
		       created_at, updated_at
		FROM agent_memory WHERE id=$1
	`, id).Scan(&e.ID, &e.OrgID, &e.SessionID, &e.Scope, &e.Key, &e.Content,
		&e.Kind, &e.Confidence, &e.Owner, &e.SourceLinks, &e.ReviewState,
		&e.Classification, &e.ExpiresAt, &e.CreatedAt, &e.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, e)
}

func (h *MemoryHandler) update(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Content     string `json:"content"`
		ReviewState string `json:"review_state"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	if update.Content != "" {
		_, _ = h.pool.Exec(r.Context(), `UPDATE agent_memory SET content=$1, updated_at=$2 WHERE id=$3`, update.Content, now, id)
	}
	if update.ReviewState != "" {
		_, _ = h.pool.Exec(r.Context(), `UPDATE agent_memory SET review_state=$1, updated_at=$2 WHERE id=$3`, update.ReviewState, now, id)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *MemoryHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	_, err := h.pool.Exec(r.Context(), `DELETE FROM agent_memory WHERE id=$1`, id)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// resolve returns memories merged across all scopes with precedence.
// Narrower scopes (run > thread > workspace > user > org > global) override
// broader ones when keys collide.
//
// Query params: org_id (required), key (optional filter), session_id, run_id, thread_id, workspace_id, user_id.
func (h *MemoryHandler) resolve(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	q := r.URL.Query()
	orgID := q.Get("org_id")
	if orgID == "" {
		jsonErr(w, "org_id is required", http.StatusBadRequest)
		return
	}
	keyFilter := q.Get("key")

	query := `
		SELECT id, org_id, session_id, scope, key, content, kind, confidence,
		       owner, source_links, review_state, classification, expires_at,
		       created_at, updated_at
		FROM agent_memory
		WHERE org_id=$1
		ORDER BY created_at DESC
	`
	rows, err := h.pool.Query(r.Context(), query, orgID)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	defer rows.Close()

	type pgxRows interface {
		Next() bool
		Scan(...any) error
		Close()
		Err() error
	}
	pgRows := rows.(pgxRows)

	// Collect all entries grouped by scope.
	scopeEntries := make(map[string][]memoryEntry)
	for pgRows.Next() {
		var e memoryEntry
		if err := pgRows.Scan(&e.ID, &e.OrgID, &e.SessionID, &e.Scope, &e.Key, &e.Content,
			&e.Kind, &e.Confidence, &e.Owner, &e.SourceLinks, &e.ReviewState,
			&e.Classification, &e.ExpiresAt, &e.CreatedAt, &e.UpdatedAt); err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if keyFilter != "" && e.Key != keyFilter {
			continue
		}
		scopeEntries[e.Scope] = append(scopeEntries[e.Scope], e)
	}

	// Merge with precedence: narrower scope wins when keys collide.
	seen := make(map[string]bool)
	var resolved []memoryEntry
	for _, scope := range scopePrecedence {
		entries, ok := scopeEntries[scope]
		if !ok {
			continue
		}
		for _, e := range entries {
			if seen[e.Key] {
				continue
			}
			seen[e.Key] = true
			resolved = append(resolved, e)
		}
	}

	writeJSON(w, map[string]any{
		"entries":    resolved,
		"count":     len(resolved),
		"precedence": scopePrecedence,
	})
}

// ---------------------------------------------------------------------------
// TasksHandler  /api/v1/tasks
// ---------------------------------------------------------------------------

// TasksHandler handles CRUD for tasks.
type TasksHandler struct {
	pool *pgxpool.Pool
}

// NewTasksHandler constructs the handler.
func NewTasksHandler(pool *pgxpool.Pool) *TasksHandler {
	return &TasksHandler{pool: pool}
}

// Register mounts routes.
func (h *TasksHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/tasks", h.listOrCreate)
	mux.HandleFunc("/api/v1/tasks/", func(w http.ResponseWriter, r *http.Request) {
		rest := r.URL.Path[len("/api/v1/tasks/"):]
		// /api/v1/tasks/:id/cancel  or  /api/v1/tasks/:id
		if len(rest) > 0 {
			switch {
			case len(rest) > 7 && rest[len(rest)-7:] == "/cancel":
				id := rest[:len(rest)-7]
				h.cancel(w, r, id)
			default:
				switch r.Method {
				case http.MethodGet:
					h.get(w, r, rest)
				case http.MethodPatch:
					h.patch(w, r, rest)
				default:
					http.NotFound(w, r)
				}
			}
		}
	})
}

type taskRow struct {
	ID             string     `json:"id"`
	OrgID          string     `json:"org_id"`
	RunID          *string    `json:"run_id,omitempty"`
	Kind           string     `json:"kind"`
	Title          string     `json:"title"`
	Description    string     `json:"description"`
	Assignee       string     `json:"assignee"`
	Status         string     `json:"status"`
	Priority       int        `json:"priority"`
	IdempotencyKey string     `json:"idempotency_key,omitempty"`
	ScheduledAt    *time.Time `json:"scheduled_at,omitempty"`
	StartedAt      *time.Time `json:"started_at,omitempty"`
	CompletedAt    *time.Time `json:"completed_at,omitempty"`
	CreatedAt      time.Time  `json:"created_at"`
	UpdatedAt      time.Time  `json:"updated_at"`
}

func (h *TasksHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		q := r.URL.Query()
		orgID := q.Get("org_id")
		status := q.Get("status")
		limit, _ := strconv.Atoi(q.Get("limit"))
		if limit == 0 {
			limit = 50
		}
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, run_id, kind, title, description, assignee, status,
			       priority, idempotency_key, scheduled_at, started_at, completed_at,
			       created_at, updated_at
			FROM tasks
			WHERE org_id=$1
			  AND ($2 = '' OR status=$2)
			  AND deleted_at IS NULL
			ORDER BY priority DESC, created_at DESC
			LIMIT $3
		`, orgID, status, limit)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var tasks []taskRow
		for rows.Next() {
			var t taskRow
			if err := rows.Scan(&t.ID, &t.OrgID, &t.RunID, &t.Kind, &t.Title, &t.Description,
				&t.Assignee, &t.Status, &t.Priority, &t.IdempotencyKey,
				&t.ScheduledAt, &t.StartedAt, &t.CompletedAt,
				&t.CreatedAt, &t.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			tasks = append(tasks, t)
		}
		writeJSON(w, map[string]any{"tasks": tasks, "count": len(tasks)})
	case http.MethodPost:
		var t taskRow
		if err := json.NewDecoder(r.Body).Decode(&t); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		if t.ID == "" {
			t.ID = "task_" + uuid.New().String()
		}
		if t.Kind == "" {
			t.Kind = "agent"
		}
		if t.Status == "" {
			t.Status = "created"
		}
		now := time.Now().UTC()
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO tasks (id, org_id, run_id, kind, title, description, assignee, status,
			    priority, idempotency_key, scheduled_at, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
		`, t.ID, t.OrgID, t.RunID, t.Kind, t.Title, t.Description, t.Assignee,
			t.Status, t.Priority, t.IdempotencyKey, t.ScheduledAt, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": t.ID, "status": t.Status})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *TasksHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var t taskRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, run_id, kind, title, description, assignee, status,
		       priority, idempotency_key, scheduled_at, started_at, completed_at,
		       created_at, updated_at
		FROM tasks WHERE id=$1 AND deleted_at IS NULL
	`, id).Scan(&t.ID, &t.OrgID, &t.RunID, &t.Kind, &t.Title, &t.Description,
		&t.Assignee, &t.Status, &t.Priority, &t.IdempotencyKey,
		&t.ScheduledAt, &t.StartedAt, &t.CompletedAt, &t.CreatedAt, &t.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, t)
}

func (h *TasksHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Status   string `json:"status"`
		Assignee string `json:"assignee"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	if update.Status != "" {
		_, _ = h.pool.Exec(r.Context(), `UPDATE tasks SET status=$1, updated_at=$2 WHERE id=$3`, update.Status, now, id)
	}
	if update.Assignee != "" {
		_, _ = h.pool.Exec(r.Context(), `UPDATE tasks SET assignee=$1, updated_at=$2 WHERE id=$3`, update.Assignee, now, id)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *TasksHandler) cancel(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	now := time.Now().UTC()
	_, err := h.pool.Exec(r.Context(),
		`UPDATE tasks SET status='cancelled', completed_at=$1, updated_at=$1 WHERE id=$2 AND deleted_at IS NULL`,
		now, id)
	if err != nil {
		jsonErr(w, err.Error(), http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"id": id, "status": "cancelled"})
}

// ---------------------------------------------------------------------------
// CronHandler  /api/v1/cron
// ---------------------------------------------------------------------------

// CronHandler handles CRUD for cron_schedules.
type CronHandler struct {
	pool *pgxpool.Pool
}

// NewCronHandler constructs the handler.
func NewCronHandler(pool *pgxpool.Pool) *CronHandler {
	return &CronHandler{pool: pool}
}

// Register mounts routes.
func (h *CronHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/cron", h.listOrCreate)
	mux.HandleFunc("/api/v1/cron/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/cron/"):]
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

type cronScheduleRow struct {
	ID           string     `json:"id"`
	OrgID        string     `json:"org_id"`
	Name         string     `json:"name"`
	Description  string     `json:"description"`
	ScheduleExpr string     `json:"schedule_expr"`
	Timezone     string     `json:"timezone"`
	TaskTemplate any        `json:"task_template"`
	Enabled      bool       `json:"enabled"`
	LastFireAt   *time.Time `json:"last_fire_at,omitempty"`
	NextFireAt   *time.Time `json:"next_fire_at,omitempty"`
	CreatedAt    time.Time  `json:"created_at"`
	UpdatedAt    time.Time  `json:"updated_at"`
}

func (h *CronHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := r.URL.Query().Get("org_id")
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, schedule_expr, timezone, task_template,
			       enabled, last_fire_at, next_fire_at, created_at, updated_at
			FROM cron_schedules WHERE org_id=$1 AND deleted_at IS NULL ORDER BY name
		`, orgID)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var schedules []cronScheduleRow
		for rows.Next() {
			var s cronScheduleRow
			if err := rows.Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.ScheduleExpr,
				&s.Timezone, &s.TaskTemplate, &s.Enabled, &s.LastFireAt, &s.NextFireAt,
				&s.CreatedAt, &s.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			schedules = append(schedules, s)
		}
		writeJSON(w, map[string]any{"schedules": schedules})
	case http.MethodPost:
		var s cronScheduleRow
		if err := json.NewDecoder(r.Body).Decode(&s); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		if s.ID == "" {
			s.ID = "cron_" + uuid.New().String()
		}
		if s.Timezone == "" {
			s.Timezone = "UTC"
		}
		tplJSON, _ := json.Marshal(s.TaskTemplate)
		now := time.Now().UTC()
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO cron_schedules (id, org_id, name, description, schedule_expr, timezone,
			    task_template, enabled, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
		`, s.ID, s.OrgID, s.Name, s.Description, s.ScheduleExpr, s.Timezone,
			tplJSON, s.Enabled, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": s.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *CronHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var s cronScheduleRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, schedule_expr, timezone, task_template,
		       enabled, last_fire_at, next_fire_at, created_at, updated_at
		FROM cron_schedules WHERE id=$1 AND deleted_at IS NULL
	`, id).Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.ScheduleExpr,
		&s.Timezone, &s.TaskTemplate, &s.Enabled, &s.LastFireAt, &s.NextFireAt,
		&s.CreatedAt, &s.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, s)
}

func (h *CronHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled      *bool  `json:"enabled"`
		ScheduleExpr string `json:"schedule_expr"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	if update.Enabled != nil {
		_, _ = h.pool.Exec(r.Context(), `UPDATE cron_schedules SET enabled=$1, updated_at=$2 WHERE id=$3`, *update.Enabled, now, id)
	}
	if update.ScheduleExpr != "" {
		_, _ = h.pool.Exec(r.Context(), `UPDATE cron_schedules SET schedule_expr=$1, updated_at=$2 WHERE id=$3`, update.ScheduleExpr, now, id)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *CronHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	now := time.Now().UTC()
	_, _ = h.pool.Exec(r.Context(), `UPDATE cron_schedules SET deleted_at=$1, updated_at=$1 WHERE id=$2`, now, id)
	w.WriteHeader(http.StatusNoContent)
}
