package handler

// WS19-S8 and W3-FIX-17: the Space-deletion purge answers only the pinned Control coordinator, is mounted without the canvas flag, and
// its receipt names the verified bearer's org and the request's Space, which the Application deletion leg checks against the delivery.

import (
	"context"
	"encoding/json"
	"net/http"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
)

const canvasPurgePath = "/v1/internal/space-canvas/purge"

// canvasPurgeRouter mounts the purge route over repo. MountCanvasPurgeRoute takes no canvas flag, so this is the route as the service
// mounts it with the flag off.
func canvasPurgeRouter(t *testing.T, repo *canvas.Repo) http.Handler {
	t.Helper()
	_, verifier := canvasTestSigner(t)
	router := chi.NewRouter()
	MountCanvasPurgeRoute(router, authctx.Middleware(verifier), NewCanvasPurgeHandler(repo))
	return router
}

// canvasPurgeBody is a valid purge request for space, with edit applied to its fields when given.
func canvasPurgeBody(t *testing.T, space string, edit func(map[string]any)) string {
	t.Helper()
	body := map[string]any{
		"deletion_request_id": "space:" + space + ":deletion:req-1",
		"space_ref":           space,
		"idempotency_key":     "purge-key-1",
	}
	if edit != nil {
		edit(body)
	}
	return canvasBody(t, body)
}

// TestCanvasPurgeRefusesAUserAndAnotherServiceBeforeTheBody: only the pinned coordinator may purge. A user token that carries the purge
// scope, and another service that carries it too, both get 403 canvas_purge_not_authorized.
func TestCanvasPurgeRefusesAUserAndAnotherServicePrincipal(t *testing.T) {
	router := canvasPurgeRouter(t, canvas.NewRepo(nil))
	body := canvasPurgeBody(t, canvasTestSpace, nil)
	for _, tc := range []struct {
		name, bearer string
	}{
		{"user token with the purge scope", canvasBearer(t, canvasTestOrg, []string{canvasPurgeScope}, false)},
		{"other service with the purge scope", canvasServiceBearer(t, "service:other-plane", canvasTestOrg, []string{canvasPurgeScope})},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := serveCanvas(t, router, http.MethodPost, canvasPurgePath, tc.bearer, "", body)
			wantCanvasRefusal(t, rec, http.StatusForbidden, "canvas_purge_not_authorized")
		})
	}
}

// TestCanvasPurgeNeedsTheCoordinatorsScope: the coordinator itself is refused without data:space-canvas:delete, by the scope check.
func TestCanvasPurgeNeedsTheCoordinatorsScope(t *testing.T) {
	router := canvasPurgeRouter(t, canvas.NewRepo(nil))
	bearer := canvasServiceBearer(t, spaceDeletionCoordinator, canvasTestOrg, []string{"wiki.read"})
	rec := serveCanvas(t, router, http.MethodPost, canvasPurgePath, bearer, "", canvasPurgeBody(t, canvasTestSpace, nil))
	wantCanvasRefusal(t, rec, http.StatusForbidden, "insufficient_scope")
}

// TestCanvasPurgeIsMountedWithoutTheCanvasFlag: the purge is not behind WIKISTORE_SPACE_CANVAS. With the flag off it answers the
// credential and principal refusals of a live route, never the 404 a disabled canvas route gives.
func TestCanvasPurgeIsMountedWithoutTheCanvasFlag(t *testing.T) {
	router := canvasPurgeRouter(t, canvas.NewRepo(nil))
	body := canvasPurgeBody(t, canvasTestSpace, nil)
	rec := serveCanvas(t, router, http.MethodPost, canvasPurgePath, "", "", body)
	wantCanvasRefusal(t, rec, http.StatusUnauthorized, "unauthorized")

	other := canvasServiceBearer(t, "service:other-plane", canvasTestOrg, []string{canvasPurgeScope})
	rec = serveCanvas(t, router, http.MethodPost, canvasPurgePath, other, "", body)
	wantCanvasRefusal(t, rec, http.StatusForbidden, "canvas_purge_not_authorized")
}

// TestCanvasPurgeRefusesAMalformedRequest: an unknown field (here an org, which the body may not name), a padded deletion request id and
// a missing idempotency key are each 400, and none reaches the store.
func TestCanvasPurgeRefusesAMalformedRequest(t *testing.T) {
	router := canvasPurgeRouter(t, canvas.NewRepo(nil))
	bearer := canvasServiceBearer(t, spaceDeletionCoordinator, canvasTestOrg, []string{canvasPurgeScope})
	for _, tc := range []struct {
		name string
		body string
	}{
		{"unknown field", canvasPurgeBody(t, canvasTestSpace, func(b map[string]any) { b["org_id"] = "org-other" })},
		{"padded deletion request id", canvasPurgeBody(t, canvasTestSpace, func(b map[string]any) {
			b["deletion_request_id"] = " space:" + canvasTestSpace + ":deletion:req-1 "
		})},
		{"missing idempotency key", canvasPurgeBody(t, canvasTestSpace, func(b map[string]any) { delete(b, "idempotency_key") })},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := serveCanvas(t, router, http.MethodPost, canvasPurgePath, bearer, "", tc.body)
			wantCanvasRefusal(t, rec, http.StatusBadRequest, canvas.CodeInvalidRequest)
		})
	}
}

// canvasPurgeReceipt is the top-level receipt the coordinator reads back.
type canvasPurgeReceipt struct {
	RequestID      string `json:"request_id"`
	OwnerPlane     string `json:"owner_plane"`
	OrgID          string `json:"org_id"`
	SpaceRef       string `json:"space_ref"`
	CanvasesPurged int64  `json:"canvases_purged"`
	VersionsPurged int64  `json:"versions_purged"`
	OwnerOutcome   string `json:"owner_outcome"`
}

// TestCanvasPurgeReceiptNamesTheBearerOrgAndTheSpace (gated on WIKI_TEST_DATABASE_URL): a valid purge deletes the Space's canvas and
// answers a top-level receipt with the verified org and the request's Space. A replay finds nothing and reports zero counts.
func TestCanvasPurgeReceiptNamesTheBearerOrgAndTheSpace(t *testing.T) {
	repo := canvasTestRepo(t)
	org, space := canvasUniqueScope(t, repo)
	if _, err := repo.Save(context.Background(), canvas.SaveInput{
		OrgID:          org,
		SpaceRef:       space,
		Content:        "# Room\n",
		IdempotencyKey: "seed-key-1",
		Author:         canvas.Author{Kind: canvas.AuthorUser, ID: canvasTestUser},
		Provenance:     canvas.Provenance{DecisionRef: "decision-seed-1", AuthorityRevision: 1, AudienceRevision: 1},
	}); err != nil {
		t.Fatalf("seed a canvas: %v", err)
	}
	router := canvasPurgeRouter(t, repo)
	bearer := canvasServiceBearer(t, spaceDeletionCoordinator, org, []string{canvasPurgeScope})
	body := canvasPurgeBody(t, space, nil)

	rec := serveCanvas(t, router, http.MethodPost, canvasPurgePath, bearer, "", body)
	if rec.Code != http.StatusOK {
		t.Fatalf("purge = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}
	var receipt canvasPurgeReceipt
	if err := json.Unmarshal(rec.Body.Bytes(), &receipt); err != nil {
		t.Fatalf("decode purge receipt %q: %v", rec.Body.String(), err)
	}
	if receipt.OrgID != org || receipt.SpaceRef != space || receipt.RequestID != "space:"+space+":deletion:req-1" ||
		receipt.OwnerPlane != "data" || receipt.OwnerOutcome != "succeeded" || receipt.CanvasesPurged != 1 || receipt.VersionsPurged != 1 {
		t.Fatalf("receipt = %+v, want org %q, space %q, one canvas and one version purged, outcome succeeded", receipt, org, space)
	}

	rec = serveCanvas(t, router, http.MethodPost, canvasPurgePath, bearer, "", body)
	if rec.Code != http.StatusOK {
		t.Fatalf("replayed purge = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}
	var replay canvasPurgeReceipt
	if err := json.Unmarshal(rec.Body.Bytes(), &replay); err != nil {
		t.Fatalf("decode replayed receipt %q: %v", rec.Body.String(), err)
	}
	if replay.CanvasesPurged != 0 || replay.VersionsPurged != 0 || replay.OrgID != org || replay.SpaceRef != space {
		t.Fatalf("replayed receipt = %+v, want zero counts for org %q and space %q", replay, org, space)
	}
}
