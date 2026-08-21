package api

import (
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

// ---------------------------------------------------------------------------
// RunWatchersHandler  /api/v1/runs/{run_id}/watchers
// ---------------------------------------------------------------------------
//
// Backs AUTO-2 ("notify me when a run finishes"): lets the calling user
// register/inspect/cancel a watch on one run, backed by
// run_watch_subscriptions (migrations/0012_run_watch_subscriptions.up.sql).
// internal/runwatch.Notifier is the consumer that actually fires the
// notification when the run reaches a terminal state; this handler only
// manages the subscription row.
//
// org_id and user_id come ONLY from the verified principal
// (verifiedOrganizationID/verifiedActorID) on every route — never from the
// request body or path — so one user can neither watch on another user's
// behalf nor read/cancel another user's watch.

// RunWatchersHandler handles the run-watch subscription lifecycle.
type RunWatchersHandler struct {
	pool registryDatabase
}

// NewRunWatchersHandler constructs the handler.
func NewRunWatchersHandler(pool registryDatabase) *RunWatchersHandler {
	return &RunWatchersHandler{pool: pool}
}

const runWatchersPathPrefix = "/api/v1/runs/"
const runWatchersPathSuffix = "/watchers"

// Register mounts /api/v1/runs/{run_id}/watchers.
func (h *RunWatchersHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc(runWatchersPathPrefix, func(w http.ResponseWriter, r *http.Request) {
		runID, ok := runIDFromWatchersPath(r.URL.Path)
		if !ok {
			http.NotFound(w, r)
			return
		}
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, runID)
		case http.MethodPost:
			h.create(w, r, runID)
		case http.MethodDelete:
			h.delete(w, r, runID)
		default:
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		}
	})
}

// runIDFromWatchersPath extracts {run_id} from
// /api/v1/runs/{run_id}/watchers, rejecting anything that is not exactly
// that shape (an empty run id, a missing /watchers suffix, or extra path
// segments) rather than guessing.
func runIDFromWatchersPath(path string) (string, bool) {
	rest, ok := strings.CutPrefix(path, runWatchersPathPrefix)
	if !ok {
		return "", false
	}
	runID, ok := strings.CutSuffix(rest, runWatchersPathSuffix)
	if !ok || runID == "" || strings.Contains(runID, "/") {
		return "", false
	}
	return runID, true
}

func verifiedWatcherIdentity(w http.ResponseWriter, r *http.Request) (orgID, userID string, ok bool) {
	orgID, userID = verifiedOrganizationID(r), verifiedActorID(r)
	if orgID == "" || userID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return "", "", false
	}
	return orgID, userID, true
}

// create registers (or re-arms) the caller's own watch on runID.
//
// This is intentionally idempotent via run_watch_subscriptions_run_user_uq
// (org_id, run_id, user_id) WHERE deleted_at IS NULL: ON CONFLICT DO NOTHING
// means a repeated POST while a watch is already active (pending OR already
// notified) never mutates it — in particular it can never silently re-arm an
// already-fired watch back to 'pending' as a side effect of a retried click.
// The response always reports the row's actual current status rather than
// assuming the INSERT branch ran.
//
// Re-subscribing after an explicit DELETE needs no special-casing here: the
// unique index's WHERE deleted_at IS NULL predicate excludes soft-deleted
// rows entirely, so a fresh INSERT for the same (org_id, run_id, user_id)
// never conflicts with an old, deleted row — it simply creates a new active
// one, with its own id, leaving the deleted row as history.
func (h *RunWatchersHandler) create(w http.ResponseWriter, r *http.Request, runID string) {
	orgID, userID, ok := verifiedWatcherIdentity(w, r)
	if !ok {
		return
	}
	now := time.Now().UTC()
	id := "runwatch_" + uuid.NewString()

	var status string
	err := h.pool.QueryRow(r.Context(), `
		INSERT INTO run_watch_subscriptions (id, org_id, run_id, user_id, status, created_at, updated_at)
		VALUES ($1, $2, $3, $4, 'pending', $5, $5)
		ON CONFLICT (org_id, run_id, user_id) WHERE deleted_at IS NULL DO NOTHING
		RETURNING status
	`, id, orgID, runID, userID, now).Scan(&status)
	if errors.Is(err, pgx.ErrNoRows) {
		// A conflict fired DO NOTHING: an active watch already exists. Report
		// its real current status instead of assuming 'pending'.
		err = h.pool.QueryRow(r.Context(), `
			SELECT status FROM run_watch_subscriptions
			WHERE org_id=$1 AND run_id=$2 AND user_id=$3 AND deleted_at IS NULL
		`, orgID, runID, userID).Scan(&status)
	}
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"run_id": runID, "watching": true, "status": status})
}

// get returns ONLY the caller's own watch state for runID — never another
// user's — so this endpoint cannot be used to enumerate who else is watching
// a run.
func (h *RunWatchersHandler) get(w http.ResponseWriter, r *http.Request, runID string) {
	orgID, userID, ok := verifiedWatcherIdentity(w, r)
	if !ok {
		return
	}
	var status string
	err := h.pool.QueryRow(r.Context(), `
		SELECT status FROM run_watch_subscriptions
		WHERE org_id=$1 AND run_id=$2 AND user_id=$3 AND deleted_at IS NULL
	`, orgID, runID, userID).Scan(&status)
	if errors.Is(err, pgx.ErrNoRows) {
		writeJSON(w, map[string]any{"run_id": runID, "watching": false})
		return
	}
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"run_id": runID, "watching": true, "status": status})
}

// delete soft-deletes ONLY the caller's own watch on runID, scoped by both
// org_id and user_id — a request naming another user's watch (there is no
// way to even name one, since the caller cannot select a target user) can
// only ever match zero rows of its own tenant, never someone else's.
func (h *RunWatchersHandler) delete(w http.ResponseWriter, r *http.Request, runID string) {
	orgID, userID, ok := verifiedWatcherIdentity(w, r)
	if !ok {
		return
	}
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(), `
		UPDATE run_watch_subscriptions
		SET deleted_at=$1, updated_at=$1
		WHERE org_id=$2 AND run_id=$3 AND user_id=$4 AND deleted_at IS NULL
	`, now, orgID, runID, userID)
	if !writeSingleScopedMutation(w, "run watcher", result, err) {
		return
	}
	w.WriteHeader(http.StatusNoContent)
}
