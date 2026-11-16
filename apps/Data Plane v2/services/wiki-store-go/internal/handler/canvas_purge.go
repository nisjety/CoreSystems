package handler

import (
	"net/http"
	"strings"
	"unicode/utf8"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
)

// spaceDeletionCoordinator is the one service principal Control's Space-deletion coordinator authenticates as. The retrieval
// engine and the owner planes pin the same subject.
const spaceDeletionCoordinator = "service:control-space-deletion"

// canvasPurgeScope is the scope the coordinator's bearer must carry for the canvas purge (WS19-S8). Control registers it
// on the coordinator principal. No Control mint site for the Data deletion bearer exists in this tree, so the grant is
// UNVERIFIED until Control's deployment registers it.
const canvasPurgeScope = "data:space-canvas:delete"

// CanvasPurgeHandler serves the Space-deletion leg of the room canvas (WS19-S8).
type CanvasPurgeHandler struct {
	repo *canvas.Repo
}

// NewCanvasPurgeHandler returns the purge handler over the canvas store.
func NewCanvasPurgeHandler(repo *canvas.Repo) *CanvasPurgeHandler {
	return &CanvasPurgeHandler{repo: repo}
}

// MountCanvasPurgeRoute installs POST /v1/internal/space-canvas/purge. The route is not behind WIKISTORE_SPACE_CANVAS. A purge
// only deletes, a service with the flag off must still erase canvases stored while the flag was on, and a 404 here would
// stall every Space deletion at the Application leg.
func MountCanvasPurgeRoute(r chi.Router, authMiddleware func(http.Handler) http.Handler, h *CanvasPurgeHandler) {
	r.With(authMiddleware, authctx.RequireScope(canvasPurgeScope)).Post("/v1/internal/space-canvas/purge", h.Purge)
}

// purgeBody is the coordinator's request. Unknown fields are refused.
type purgeBody struct {
	DeletionRequestID string `json:"deletion_request_id"`
	SpaceRef          string `json:"space_ref"`
	IdempotencyKey    string `json:"idempotency_key"`
}

// Purge hard-deletes the canvas and every version of one Space in the bearer's org, then fences the Space. Only the pinned
// coordinator principal may call it. The org comes from the verified bearer and never from the body. The purge is
// idempotent by construction: a replay finds no rows and returns zero counts, so the idempotency key is validated and not
// stored. The receipt is top-level, not wrapped in data, because the Application deletion leg reads it that way.
func (h *CanvasPurgeHandler) Purge(w http.ResponseWriter, r *http.Request) {
	bearer, ok := bearerOf(w, r)
	if !ok {
		return
	}
	if !bearer.IsService() || bearer.ServiceID != spaceDeletionCoordinator {
		writeCanvasError(w, http.StatusForbidden, "canvas_purge_not_authorized",
			"only the Control Space-deletion coordinator may purge a Space canvas", nil)
		return
	}
	var body purgeBody
	if !decodeCanvasBody(w, r, &body) {
		return
	}
	if !validDeletionRequestID(body.DeletionRequestID) {
		writeCanvasError(w, http.StatusBadRequest, canvas.CodeInvalidRequest, "deletion_request_id is not valid", nil)
		return
	}
	if err := canvas.ValidateIdempotencyKey(body.IdempotencyKey); err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	counts, err := h.repo.PurgeSpace(r.Context(), bearer.OrgID, body.SpaceRef)
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"request_id":      body.DeletionRequestID,
		"owner_plane":     "data",
		"space_ref":       body.SpaceRef,
		"canvases_purged": counts.CanvasesPurged,
		"versions_purged": counts.VersionsPurged,
		"owner_outcome":   "succeeded",
	})
}

// maxDeletionRequestIDBytes bounds the coordinator's request id. Control builds it as "space:<space_ref>:deletion:<key>", where
// the key is at most 160 characters, so this leaves room for the Space ref. A tighter bound would refuse real requests and
// stall Space deletion.
const maxDeletionRequestIDBytes = 512

// validDeletionRequestID accepts opaque text of at most maxDeletionRequestIDBytes bytes with no padding, no NUL and valid UTF-8.
func validDeletionRequestID(id string) bool {
	return id != "" && len(id) <= maxDeletionRequestIDBytes && strings.TrimSpace(id) == id &&
		utf8.ValidString(id) && !strings.ContainsRune(id, 0)
}
