// Package api — HTTP handlers for agent memory, tasks, and cron schedules.
package api

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// ---------------------------------------------------------------------------
// MemoryHandler  /api/v1/memory
// ---------------------------------------------------------------------------

// MemoryHandler handles CRUD for agent_memory.
type MemoryHandler struct {
	pool registryDatabase
	// runs is Session Core's RunService client (MEM-2), used to authorize
	// run/thread/session-scoped memory writes before they persist. nil-able:
	// resource-scoped memory writes fail closed to 403/503 when unset or
	// unreachable rather than being silently allowed. Workspace has no
	// ownership contract anywhere in Model Plane and stays quarantined
	// regardless of this client's presence — see authorizeResourceOwner.
	runs mpv1.RunServiceClient
}

// NewMemoryHandler constructs the handler.
func NewMemoryHandler(pool *pgxpool.Pool, runs mpv1.RunServiceClient) *MemoryHandler {
	return &MemoryHandler{pool: pool, runs: runs}
}

// memoryScopePrecedence returns the scopes this endpoint can authorize with
// the supplied resource context, narrowest first: run > thread > session >
// user > org > global. Workspace has no ownership contract anywhere in Model
// Plane (see authorizeResourceOwner) and is never implied here.
func memoryScopePrecedence(sessionID, threadID, runID string) []string {
	precedence := make([]string, 0, 6)
	if runID != "" {
		precedence = append(precedence, "run")
	}
	if threadID != "" {
		precedence = append(precedence, "thread")
	}
	if sessionID != "" {
		precedence = append(precedence, "session")
	}
	return append(precedence, "user", "org", "global")
}

// memoryVisibilitySQL is a defense-in-depth row filter applied to every read
// and mutation. Tenant-shared org/global rows remain visible within the signed
// organization. Every private scope requires a non-empty owner matching the
// cryptographically verified actor, so legacy ownerless and unknown-scope rows
// fail closed.
func memoryVisibilitySQL(actorPlaceholder string) string {
	return `(scope IN ('org','global') OR (` +
		`scope IN ('run','thread','workspace','session','user') AND ` +
		`owner=` + actorPlaceholder + ` AND owner <> ''))`
}

func validMemoryScope(scope string) bool {
	switch scope {
	case "run", "thread", "workspace", "session", "user", "org", "global":
		return true
	default:
		return false
	}
}

func memoryScopeRequiresResourceAuthorization(scope string) bool {
	switch scope {
	case "run", "thread", "workspace", "session":
		return true
	default:
		return false
	}
}

func memoryEntryMatchesResolution(entry memoryEntry, sessionID, threadID, runID string) bool {
	switch entry.Scope {
	case "user", "org", "global":
		return entry.SessionID == nil
	case "session":
		return sessionID != "" && entry.SessionID != nil && *entry.SessionID == sessionID
	case "thread":
		return threadID != "" && entry.SessionID != nil && *entry.SessionID == threadID
	case "run":
		return runID != "" && entry.SessionID != nil && *entry.SessionID == runID
	default:
		return false
	}
}

// resourceAuthDecision is a three-state outcome, not a bool, because "denied"
// and "couldn't ask" are different operator-facing conditions that must map
// to different HTTP statuses (403 vs 503): collapsing them into a single
// false would report a session-core outage as a permission problem, hiding
// an infra issue behind a misleading authorization error.
type resourceAuthDecision int

const (
	// resourceAuthDenied: the authority was reached (or is not configured)
	// and the actor does not own the resource, or the scope is unsupported.
	resourceAuthDenied resourceAuthDecision = iota
	// resourceAuthAllowed: the authority affirmatively confirmed ownership.
	resourceAuthAllowed
	// resourceAuthUnavailable: the authority could not be asked at all — a
	// nil client (session-core dial disabled/unreachable at startup) or a
	// transport/RPC error talking to it. Never conflated with "denied".
	resourceAuthUnavailable
)

// authorizeResourceOwner asks Session Core (via RunService) whether actorID
// owns resourceID for the given run/thread/session scope, before an INSERT is
// allowed to persist it. Reads never call this: memoryVisibilitySQL's
// owner=actor filter already pins every private-scope row to the actor who
// passed this exact check at write time, so re-verifying on every read would
// add a round trip for no additional safety (the same defense-in-depth
// pattern this handler already uses for scope='user'). Fails closed on every
// path other than an explicit `authorized: true` from a reachable authority.
func (h *MemoryHandler) authorizeResourceOwner(ctx context.Context, scope, resourceID, orgID, actorID string) resourceAuthDecision {
	if h.runs == nil {
		// The dependency capability-core needs to answer this question is not
		// configured/reachable at all — that is an availability problem, not
		// a "no" answer, so it must not read back as an ordinary 403.
		return resourceAuthUnavailable
	}
	switch scope {
	case "run":
		resp, err := h.runs.ResolveRunOwner(ctx, &mpv1.ResolveRunOwnerRequest{
			RunId: resourceID, OrgId: orgID, UserId: actorID,
		})
		if err != nil {
			return resourceAuthUnavailable
		}
		if resp.GetAuthorized() {
			return resourceAuthAllowed
		}
		return resourceAuthDenied
	case "thread", "session":
		resp, err := h.runs.ResolveThreadOwner(ctx, &mpv1.ResolveThreadOwnerRequest{
			ThreadId: resourceID, OrgId: orgID, UserId: actorID,
		})
		if err != nil {
			return resourceAuthUnavailable
		}
		if resp.GetAuthorized() {
			return resourceAuthAllowed
		}
		return resourceAuthDenied
	default:
		return resourceAuthDenied
	}
}

func verifiedMemoryIdentity(w http.ResponseWriter, request *http.Request) (string, string, bool) {
	organizationID := verifiedOrganizationID(request)
	actorID := verifiedActorID(request)
	if organizationID == "" || actorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return "", "", false
	}
	return organizationID, actorID, true
}

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

const maxMemoryResolveRows = 200

func (h *MemoryHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	orgID, actorID, ok := verifiedMemoryIdentity(w, r)
	if !ok {
		return
	}
	switch r.Method {
	case http.MethodGet:
		q := r.URL.Query()
		sessionID := q.Get("session_id")
		scope := q.Get("scope")
		limit, _ := strconv.Atoi(q.Get("limit"))
		if limit <= 0 || limit > 200 {
			limit = 50
		}
		var rows interface {
			Next() bool
			Scan(...any) error
			Close()
			Err() error
		}
		var err error
		if sessionID != "" {
			rows, err = h.pool.Query(r.Context(), `
				SELECT id, org_id, session_id, scope, key, content, kind, confidence,
				       owner, source_links, review_state, classification, expires_at,
				       created_at, updated_at
				FROM agent_memory
				WHERE org_id=$1 AND `+memoryVisibilitySQL("$2")+`
				  AND (session_id IS NULL OR session_id=$3)
				ORDER BY created_at LIMIT $4
			`, orgID, actorID, sessionID, limit)
		} else if scope != "" {
			if !validMemoryScope(scope) {
				jsonErr(w, "invalid memory scope", http.StatusBadRequest)
				return
			}
			rows, err = h.pool.Query(r.Context(), `
				SELECT id, org_id, session_id, scope, key, content, kind, confidence,
				       owner, source_links, review_state, classification, expires_at,
				       created_at, updated_at
				FROM agent_memory
				WHERE org_id=$1 AND `+memoryVisibilitySQL("$2")+` AND scope=$3
				ORDER BY created_at LIMIT $4
			`, orgID, actorID, scope, limit)
		} else {
			rows, err = h.pool.Query(r.Context(), `
				SELECT id, org_id, session_id, scope, key, content, kind, confidence,
				       owner, source_links, review_state, classification, expires_at,
				       created_at, updated_at
				FROM agent_memory
				WHERE org_id=$1 AND `+memoryVisibilitySQL("$2")+`
				ORDER BY created_at LIMIT $3
			`, orgID, actorID, limit)
		}
		if err != nil {
			jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
			return
		}
		defer rows.Close()
		var entries []memoryEntry
		for rows.Next() {
			var e memoryEntry
			if err := rows.Scan(&e.ID, &e.OrgID, &e.SessionID, &e.Scope, &e.Key, &e.Content,
				&e.Kind, &e.Confidence, &e.Owner, &e.SourceLinks, &e.ReviewState,
				&e.Classification, &e.ExpiresAt, &e.CreatedAt, &e.UpdatedAt); err != nil {
				jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
				return
			}
			entries = append(entries, e)
		}
		if rows.Err() != nil {
			jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
			return
		}
		writeJSON(w, map[string]any{"entries": entries, "count": len(entries)})
	case http.MethodPost:
		var e memoryEntry
		if err := json.NewDecoder(r.Body).Decode(&e); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		e.OrgID = orgID
		e.Owner = actorID
		e.ID = "mem_" + uuid.New().String()
		e.Scope = strings.TrimSpace(e.Scope)
		if e.Scope == "" {
			e.Scope = "org"
		}
		if !validMemoryScope(e.Scope) {
			jsonErr(w, "invalid memory scope", http.StatusBadRequest)
			return
		}
		switch {
		case e.Scope == "workspace":
			// No ownership concept exists for workspace anywhere in Model
			// Plane (MEM-2 investigation: session-core's workspace_id is a
			// content-selection hint only, and capability-core's own
			// ScopeKindWorkspace is rejected at capability-invocation time —
			// there is no table, owner column, or RPC anywhere that resolves
			// "who owns workspace X"). Building a check here would mean
			// fabricating an authorization concept that doesn't exist in the
			// data model, so this stays quarantined exactly as before,
			// unconditionally, pending a separate ticket that first decides
			// what workspace ownership even means in Model Plane.
			jsonErr(w, "workspace-scoped memory writes require an ownership model that does not exist yet", http.StatusServiceUnavailable)
			return
		case memoryScopeRequiresResourceAuthorization(e.Scope):
			// run, thread, session (MEM-2): authorize against Session Core's
			// RunService before the INSERT below is allowed to run. The
			// caller's session_id field carries the resource id — a run_id
			// for scope=run, a thread_id for scope IN (thread, session): see
			// agent_memory's own writer (session-core's dreaming.rs) for the
			// precedent of storing a thread_id in this same column under
			// scope='thread'.
			resourceID := ""
			if e.SessionID != nil {
				resourceID = strings.TrimSpace(*e.SessionID)
			}
			if resourceID == "" {
				jsonErr(w, "session_id is required for run/thread/session scoped memory", http.StatusBadRequest)
				return
			}
			switch h.authorizeResourceOwner(r.Context(), e.Scope, resourceID, orgID, actorID) {
			case resourceAuthAllowed:
				// fall through to the insert below.
			case resourceAuthUnavailable:
				jsonErr(w, "session core ownership authority unavailable", http.StatusServiceUnavailable)
				return
			default:
				jsonErr(w, "resource ownership check failed or denied", http.StatusForbidden)
				return
			}
		case e.SessionID != nil:
			// user/org/global writes must never carry a caller-supplied
			// resource binding — unchanged quarantine from before MEM-2.
			jsonErr(w, "resource-scoped memory writes require Session Core authorization", http.StatusServiceUnavailable)
			return
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
			jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
			return
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": e.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *MemoryHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	orgID, actorID, ok := verifiedMemoryIdentity(w, r)
	if !ok {
		return
	}
	var e memoryEntry
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, session_id, scope, key, content, kind, confidence,
		       owner, source_links, review_state, classification, expires_at,
		       created_at, updated_at
		FROM agent_memory
		WHERE id=$1 AND org_id=$2 AND `+memoryVisibilitySQL("$3")+`
	`, id, orgID, actorID).Scan(&e.ID, &e.OrgID, &e.SessionID, &e.Scope, &e.Key, &e.Content,
		&e.Kind, &e.Confidence, &e.Owner, &e.SourceLinks, &e.ReviewState,
		&e.Classification, &e.ExpiresAt, &e.CreatedAt, &e.UpdatedAt)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			jsonErr(w, "not found", http.StatusNotFound)
		} else {
			jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
		}
		return
	}
	writeJSON(w, e)
}

func (h *MemoryHandler) update(w http.ResponseWriter, r *http.Request, id string) {
	orgID, actorID, ok := verifiedMemoryIdentity(w, r)
	if !ok {
		return
	}
	var update struct {
		Content     *string `json:"content"`
		ReviewState *string `json:"review_state"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	if update.Content == nil && update.ReviewState == nil {
		jsonErr(w, "no supported fields", http.StatusBadRequest)
		return
	}
	content := ""
	if update.Content != nil {
		content = *update.Content
	}
	reviewState := ""
	if update.ReviewState != nil {
		reviewState = *update.ReviewState
	}
	tag, err := h.pool.Exec(r.Context(), `
		UPDATE agent_memory
		SET content=CASE WHEN $1 THEN $2 ELSE content END,
		    review_state=CASE WHEN $3 THEN $4 ELSE review_state END,
		    updated_at=$5
		WHERE id=$6 AND org_id=$7 AND `+memoryVisibilitySQL("$8"),
		update.Content != nil, content, update.ReviewState != nil, reviewState, now, id, orgID, actorID)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
		return
	}
	if tag.RowsAffected() == 0 {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *MemoryHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	orgID, actorID, ok := verifiedMemoryIdentity(w, r)
	if !ok {
		return
	}
	tag, err := h.pool.Exec(r.Context(), `
		DELETE FROM agent_memory
		WHERE id=$1 AND org_id=$2 AND `+memoryVisibilitySQL("$3"), id, orgID, actorID)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
		return
	}
	if tag.RowsAffected() == 0 {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// resolve returns memories merged across all scopes with precedence.
// Narrower scopes (run > thread > session > user > org > global) override
// broader ones when keys collide. workspace has no ownership contract
// anywhere in Model Plane (MEM-2) and is never selectable here.
//
// Query params: org_id (required), key (optional filter), session_id, run_id, thread_id, workspace_id (rejected, 501), user_id.
func (h *MemoryHandler) resolve(w http.ResponseWriter, r *http.Request) {
	orgID, actorID, ok := verifiedMemoryIdentity(w, r)
	if !ok {
		return
	}
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	q := r.URL.Query()
	keyFilter := q.Get("key")
	if strings.TrimSpace(q.Get("workspace_id")) != "" {
		// No ownership concept exists for workspace anywhere in Model Plane
		// (see the write-path gate's comment in listOrCreate for the full
		// rationale) — stays quarantined, distinctly from the now-supported
		// run_id/thread_id selectors below.
		jsonErr(w, "workspace-scoped memory resolution requires an ownership model that does not exist yet", http.StatusNotImplemented)
		return
	}
	sessionID := strings.TrimSpace(q.Get("session_id"))
	threadID := strings.TrimSpace(q.Get("thread_id"))
	runID := strings.TrimSpace(q.Get("run_id"))

	// Build the scope clause list: the tenant-shared base clause plus one
	// clause per resource selector actually supplied, OR'd together. This is
	// read-only: memoryVisibilitySQL's owner=actor filter already restricts
	// every private-scope row (including run/thread) to rows owned by the
	// verified actor, so no additional Session Core round trip is needed
	// here — the write path already verified ownership before the row could
	// exist with owner=actorID (see authorizeResourceOwner).
	clauses := []string{"(scope IN ('user','org','global') AND session_id IS NULL)"}
	queryArgs := []any{orgID, actorID}
	nextArg := 3
	if sessionID != "" {
		clauses = append(clauses, fmt.Sprintf("(scope='session' AND session_id=$%d)", nextArg))
		queryArgs = append(queryArgs, sessionID)
		nextArg++
	}
	if threadID != "" {
		clauses = append(clauses, fmt.Sprintf("(scope='thread' AND session_id=$%d)", nextArg))
		queryArgs = append(queryArgs, threadID)
		nextArg++
	}
	if runID != "" {
		clauses = append(clauses, fmt.Sprintf("(scope='run' AND session_id=$%d)", nextArg))
		queryArgs = append(queryArgs, runID)
		nextArg++
	}
	keyArg, limitArg := nextArg, nextArg+1
	queryArgs = append(queryArgs, keyFilter, maxMemoryResolveRows+1)
	query := fmt.Sprintf(`
		SELECT id, org_id, session_id, scope, key, content, kind, confidence,
		       owner, source_links, review_state, classification, expires_at,
		       created_at, updated_at
		FROM agent_memory
		WHERE org_id=$1 AND %s
		  AND (%s)
		  AND ($%d='' OR key=$%d)
		ORDER BY created_at DESC
		LIMIT $%d
	`, memoryVisibilitySQL("$2"), strings.Join(clauses, " OR "), keyArg, keyArg, limitArg)
	rows, err := h.pool.Query(r.Context(), query, queryArgs...)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
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
	rowCount := 0
	for pgRows.Next() {
		rowCount++
		if rowCount > maxMemoryResolveRows {
			jsonErr(w, "memory resolution exceeds the bounded result limit", http.StatusUnprocessableEntity)
			return
		}
		var e memoryEntry
		if err := pgRows.Scan(&e.ID, &e.OrgID, &e.SessionID, &e.Scope, &e.Key, &e.Content,
			&e.Kind, &e.Confidence, &e.Owner, &e.SourceLinks, &e.ReviewState,
			&e.Classification, &e.ExpiresAt, &e.CreatedAt, &e.UpdatedAt); err != nil {
			jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
			return
		}
		if keyFilter != "" && e.Key != keyFilter {
			continue
		}
		if !memoryEntryMatchesResolution(e, sessionID, threadID, runID) {
			continue
		}
		scopeEntries[e.Scope] = append(scopeEntries[e.Scope], e)
	}
	if pgRows.Err() != nil {
		jsonErr(w, "database unavailable", http.StatusServiceUnavailable)
		return
	}

	// Merge with precedence: narrower scope wins when keys collide.
	precedence := memoryScopePrecedence(sessionID, threadID, runID)
	seen := make(map[string]bool)
	var resolved []memoryEntry
	for _, scope := range precedence {
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
		"count":      len(resolved),
		"precedence": precedence,
	})
}

// ---------------------------------------------------------------------------
// TasksHandler  /api/v1/tasks
// ---------------------------------------------------------------------------

// TasksHandler handles CRUD for tasks.
type TasksHandler struct {
	pool registryDatabase
}

// NewTasksHandler constructs the handler.
func NewTasksHandler(pool registryDatabase) *TasksHandler {
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
		orgID := verifiedOrganizationID(r)
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
		t.OrgID = verifiedOrganizationID(r)
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
		FROM tasks WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL
	`, id, verifiedOrganizationID(r)).Scan(&t.ID, &t.OrgID, &t.RunID, &t.Kind, &t.Title, &t.Description,
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
		Status   *string `json:"status"`
		Assignee *string `json:"assignee"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if update.Status == nil && update.Assignee == nil {
		jsonErr(w, "at least one field is required", http.StatusBadRequest)
		return
	}
	if update.Status != nil && strings.TrimSpace(*update.Status) == "" {
		jsonErr(w, "status must not be empty", http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(), `
		UPDATE tasks
		SET status=COALESCE($1, status), assignee=COALESCE($2, assignee), updated_at=$3
		WHERE id=$4 AND org_id=$5 AND deleted_at IS NULL
	`, update.Status, update.Assignee, now, id, verifiedOrganizationID(r))
	if !writeSingleScopedMutation(w, "task", result, err) {
		return
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *TasksHandler) cancel(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(),
		`UPDATE tasks SET status='cancelled', completed_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL AND status NOT IN ('completed', 'cancelled', 'failed')`,
		now, id, verifiedOrganizationID(r))
	if !writeTaskCancellationMutation(w, r, h.pool, id, result, err) {
		return
	}
	writeJSON(w, map[string]any{"id": id, "status": "cancelled"})
}

func writeTaskCancellationMutation(w http.ResponseWriter, r *http.Request, database registryDatabase, id string, result pgconn.CommandTag, err error) bool {
	if err != nil {
		return writeSingleScopedMutation(w, "task", result, err)
	}
	if result.RowsAffected() == 1 {
		return true
	}

	var status string
	err = database.QueryRow(r.Context(), `SELECT status FROM tasks WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL`, id, verifiedOrganizationID(r)).Scan(&status)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return false
	}
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return false
	}

	jsonErr(w, "task cannot be cancelled in its current state", http.StatusConflict)
	return false
}

// ---------------------------------------------------------------------------
// CronHandler  /api/v1/cron
// ---------------------------------------------------------------------------

// CronHandler handles CRUD for cron_schedules.
type CronHandler struct {
	pool     registryDatabase
	verifier *cron.ControlDecisionVerifier
}

// NewCronHandler constructs the handler.
func NewCronHandler(pool registryDatabase) *CronHandler {
	return &CronHandler{pool: pool}
}

func (h *CronHandler) WithSpaceDecisionVerifier(verifier *cron.ControlDecisionVerifier) *CronHandler {
	h.verifier = verifier
	return h
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
	SpaceRef     string     `json:"space_ref"`
	CreatorID    string     `json:"creator_subject_id"`
	// SpaceDecisionToken is accepted only at creation and never stored or
	// returned. Capability Core verifies it before extracting durable claims.
	SpaceDecisionToken string `json:"space_schedule_create_decision,omitempty"`
	IdempotencyKey     string `json:"idempotency_key,omitempty"`
}

func (h *CronHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := verifiedOrganizationID(r)
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, schedule_expr, timezone, task_template,
			       enabled, last_fire_at, next_fire_at, created_at, updated_at, space_ref, creator_subject_id
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
				&s.CreatedAt, &s.UpdatedAt, &s.SpaceRef, &s.CreatorID); err != nil {
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
		s.OrgID = verifiedOrganizationID(r)
		if strings.TrimSpace(s.ID) == "" {
			jsonErr(w, "a caller-generated schedule id is required for scoped authorization", http.StatusBadRequest)
			return
		}
		if s.Timezone == "" {
			s.Timezone = "UTC"
		}
		tplJSON, _ := json.Marshal(s.TaskTemplate)
		if h.verifier == nil {
			jsonErr(w, "Space schedule authorization is unavailable", http.StatusServiceUnavailable)
			return
		}
		if strings.TrimSpace(s.SpaceRef) == "" || strings.TrimSpace(s.CreatorID) == "" || strings.TrimSpace(s.SpaceDecisionToken) == "" || strings.TrimSpace(s.IdempotencyKey) == "" {
			jsonErr(w, "space_ref, creator_subject_id, schedule decision, and idempotency_key are required", http.StatusBadRequest)
			return
		}
		templateDigest := fmt.Sprintf("sha256:%x", sha256.Sum256(tplJSON))
		binding, err := h.verifier.VerifyScheduleCreate(s.SpaceDecisionToken, cron.CreateIntent{
			OrgID: verifiedOrganizationID(r), SpaceRef: s.SpaceRef, SubjectID: s.CreatorID,
			ScheduleID: s.ID, TemplateDigest: templateDigest, IdempotencyKey: s.IdempotencyKey,
		}, time.Now().UTC())
		if err != nil {
			jsonErr(w, "Space schedule creation is not authorized", http.StatusForbidden)
			return
		}
		now := time.Now().UTC()
		// Validate the cron expression up front and seed next_fire_at so the UI
		// shows the next run and the sweeper advances it correctly.
		next, nerr := cron.NextFrom(s.ScheduleExpr, s.Timezone, now)
		if nerr != nil {
			jsonErr(w, "invalid cron expression: "+nerr.Error(), http.StatusBadRequest)
			return
		}
		_, err = h.pool.Exec(r.Context(), `
			INSERT INTO cron_schedules (id, org_id, name, description, schedule_expr, timezone,
			    task_template, enabled, next_fire_at, created_at, updated_at, space_ref, creator_subject_id,
			    recipient_audience_ref, recipient_audience_hash, resource_authorization_ref, privacy_policy_ref,
			    authority_revision, membership_revision, privacy_revision, recipient_audience_revision,
			    entitlement_revision, template_digest)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)
		`, s.ID, s.OrgID, s.Name, s.Description, s.ScheduleExpr, s.Timezone,
			tplJSON, s.Enabled, next, now, now, binding.SpaceRef, binding.SubjectID,
			binding.RecipientAudienceRef, binding.RecipientAudienceHash, binding.ResourceAuthorizationRef, binding.PrivacyPolicyRef,
			binding.AuthorityRevision, binding.MembershipRevision, binding.PrivacyRevision, binding.RecipientAudienceRevision,
			binding.EntitlementRevision, templateDigest)
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
		       enabled, last_fire_at, next_fire_at, created_at, updated_at, space_ref, creator_subject_id
		FROM cron_schedules WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL
	`, id, verifiedOrganizationID(r)).Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.ScheduleExpr,
		&s.Timezone, &s.TaskTemplate, &s.Enabled, &s.LastFireAt, &s.NextFireAt,
		&s.CreatedAt, &s.UpdatedAt, &s.SpaceRef, &s.CreatorID)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, s)
}

func (h *CronHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled      *bool   `json:"enabled"`
		ScheduleExpr *string `json:"schedule_expr"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if update.Enabled == nil && update.ScheduleExpr == nil {
		jsonErr(w, "at least one field is required", http.StatusBadRequest)
		return
	}
	if update.ScheduleExpr != nil && strings.TrimSpace(*update.ScheduleExpr) == "" {
		jsonErr(w, "schedule expression must not be empty", http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	var nextFireAt any
	if update.ScheduleExpr != nil {
		var timezone string
		err := h.pool.QueryRow(r.Context(), `SELECT timezone FROM cron_schedules WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL`, id, verifiedOrganizationID(r)).Scan(&timezone)
		if errors.Is(err, pgx.ErrNoRows) {
			jsonErr(w, "not found", http.StatusNotFound)
			return
		}
		if err != nil {
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		next, err := cron.NextFrom(*update.ScheduleExpr, timezone, now)
		if err != nil {
			jsonErr(w, "invalid cron expression: "+err.Error(), http.StatusBadRequest)
			return
		}
		nextFireAt = next
	}
	result, err := h.pool.Exec(r.Context(), `
		UPDATE cron_schedules
		SET enabled=COALESCE($1, enabled), schedule_expr=COALESCE($2, schedule_expr),
		    next_fire_at=COALESCE($3, next_fire_at), updated_at=$4
		WHERE id=$5 AND org_id=$6 AND deleted_at IS NULL
	`, update.Enabled, update.ScheduleExpr, nextFireAt, now, id, verifiedOrganizationID(r))
	if !writeSingleScopedMutation(w, "cron schedule", result, err) {
		return
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *CronHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(), `UPDATE cron_schedules SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL`, now, id, verifiedOrganizationID(r))
	if !writeSingleScopedMutation(w, "cron schedule", result, err) {
		return
	}
	w.WriteHeader(http.StatusNoContent)
}
