package api

import (
	"encoding/json"
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

const spaceDeletionCoordinator = "service:control-space-deletion"

type spaceCronDeletionRequest struct {
	OrgID             string `json:"org_id"`
	SpaceRef          string `json:"space_ref"`
	OwnerPrincipalID  string `json:"owner_principal_id"`
	DeletionRequestID string `json:"deletion_request_id"`
}

// RegisterDeletionAdapter mounts the coordinator-only, request-bound schedule
// cancellation endpoint. It is deliberately not part of the user cron CRUD
// surface: a user delete and a durable Space purge have different authority,
// retry, and receipt semantics.
func (h *CronHandler) RegisterDeletionAdapter(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/internal/space-deletion/cron", h.deleteSpaceSchedules)
}

func (h *CronHandler) deleteSpaceSchedules(w http.ResponseWriter, request *http.Request) {
	principal, ok := authctx.PrincipalFromContext(request.Context())
	if !ok || principal.PrincipalType != "service" || principal.ActorID != spaceDeletionCoordinator || !principal.HasScope(authz.SpaceDeletionScope) {
		jsonErr(w, "Space deletion coordinator required", http.StatusForbidden)
		return
	}
	var body spaceCronDeletionRequest
	if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
		jsonErr(w, "invalid Space schedule deletion request", http.StatusBadRequest)
		return
	}
	if strings.TrimSpace(body.OrgID) == "" || strings.TrimSpace(body.SpaceRef) == "" || strings.TrimSpace(body.OwnerPrincipalID) == "" || strings.TrimSpace(body.DeletionRequestID) == "" || body.OrgID != principal.OrganizationID {
		jsonErr(w, "Space schedule deletion request is not authorized", http.StatusForbidden)
		return
	}
	// A schedule cancellation by itself is insufficient: a due fire may already
	// have materialised a task that the executor can claim.  Keep the schedule
	// and its not-yet-running fires in one statement so the canonical Model
	// receipt never says the Space is clean while a pending cron task remains.
	// Running tasks are deliberately not rewritten here: their effect may
	// already be uncertain, so a later owner receipt must report that state
	// rather than falsely claiming they were stopped.
	result, err := h.pool.Exec(request.Context(), `
		WITH scoped_schedules AS (
			SELECT id
			FROM cron_schedules
			WHERE org_id=$2 AND space_ref=$3 AND creator_subject_id=$4
		), cancelled_tasks AS (
			UPDATE tasks
			SET status='cancelled', completed_at=$1, updated_at=$1,
				config_json = config_json || jsonb_build_object(
					'cancellation_reason', 'space deleted',
					'space_deletion_request_id', $5
				)
			WHERE status IN ('created', 'assigned', 'blocked')
			  AND id IN (
					SELECT task_id FROM cron_fires
					WHERE schedule_id IN (SELECT id FROM scoped_schedules)
					  AND task_id IS NOT NULL
				)
			RETURNING id
		)
		UPDATE cron_schedules
		SET enabled=false, deleted_at=COALESCE(deleted_at, $1), updated_at=$1
		WHERE id IN (SELECT id FROM scoped_schedules) AND deleted_at IS NULL`,
		time.Now().UTC(), body.OrgID, body.SpaceRef, body.OwnerPrincipalID, body.DeletionRequestID)
	if err != nil {
		jsonErr(w, "Space schedule deletion unavailable", http.StatusServiceUnavailable)
		return
	}
	writeJSON(w, map[string]any{
		"request_id":  body.DeletionRequestID,
		"owner_plane": "model",
		// Session, external memory, and other Model owners have separate
		// receipts, so this local schedule adapter never overclaims completion.
		"outcome":        "partial",
		"canceled_count": result.RowsAffected(),
		"detail":         "space_cron_schedules_and_pending_fires_canceled",
	})
}
