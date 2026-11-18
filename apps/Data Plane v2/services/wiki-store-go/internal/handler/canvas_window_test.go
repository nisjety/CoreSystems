package handler

// Route tests for a history window on the room canvas (D18 and WS04-S11). A windowed read is decided by the canvas store, so the
// gated test below runs it end to end. A save or a restore under a window is refused before the store is read, which the tests
// without a database rely on. A hidden canvas is the typed 403, never a store failure, and a head-moved answer to a windowed
// creator names no head.

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/spacedecision"
)

// windowedHumanClaims is a human decision for one canvas operation under a window that starts at at. It carries both floors, a
// timestamp and a revision, as Control signs a since_join member's window.
func windowedHumanClaims(org, space, key string, write bool, at time.Time) spacedecision.Claims {
	claims := canvasHumanClaims(org, space, key, write)
	claims.HistoryFloorRevision = 1
	claims.HistoryFloorAt = &at
	return claims
}

// TestCanvasRestoreUnderAWindowIsRefusedBeforeTheStore: a restore makes an older version the head, so a windowed member may not restore.
// The refusal comes after the decision is verified and before the store is read.
func TestCanvasRestoreUnderAWindowIsRefusedBeforeTheStore(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.write"}, false)
	at := time.Now().UTC().Add(-time.Hour)
	claims := windowedHumanClaims(canvasTestOrg, canvasTestSpace, "restore-window-1", true, at)
	op := spacedecision.CanvasOp{Op: spacedecision.OpRestore, Schema: spacedecision.SchemaCanvasWrite,
		RestoreFromVersionID: "version-1", IdempotencyKey: "restore-window-1"}
	rec := serveCanvas(t, router, http.MethodPost, canvasSpacePath+"/versions/version-1/restore", bearer,
		canvasHumanToken(t, claims, op), canvasBody(t, map[string]any{"idempotency_key": "restore-window-1"}))
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeHistoryFloor)
}

// TestCanvasAgentWriteUnderAWindowIsRefusedBeforeTheStore: Control never issues an agent write under a window, so a run-action decision
// that carries one is refused before the store is read, whatever the operation.
func TestCanvasAgentWriteUnderAWindowIsRefusedBeforeTheStore(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.write"}, false)
	run := canvasRunClaims(canvasTestOrg, canvasTestSpace, "agent-window-1", true, nil)
	at := time.Now().UTC().Add(-time.Hour)
	run.HistoryFloorAt = &at
	decision := signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, run)
	rec := serveCanvas(t, router, http.MethodPost, canvasSpacePath+"/versions", bearer, decision,
		canvasBody(t, map[string]any{"content": "# Room\n", "idempotency_key": "agent-window-1"}))
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeHistoryFloor)
}

// TestCanvasHistoryFloorRefusalIsATypedForbidden: a hidden canvas is answered as the typed 403 with the floor's code, not as the 503
// a store failure gets.
func TestCanvasHistoryFloorRefusalIsATypedForbidden(t *testing.T) {
	rec := httptest.NewRecorder()
	writeCanvasStoreError(rec, &canvas.Error{Code: canvas.CodeHistoryFloor, Message: "the canvas is hidden from this member"})
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeHistoryFloor)
}

// TestCanvasHeadMovedNamesNoHeadToAWindowedCreator: the head-moved answer names the head to an unwindowed caller, as before, and names
// none to a windowed one, whose head's version number and time are pre-window state.
func TestCanvasHeadMovedNamesNoHeadToAWindowedCreator(t *testing.T) {
	moved := &canvas.Error{
		Code:    canvas.CodeHeadMoved,
		Message: "the canvas head moved since you opened it",
		Head:    &canvas.HeadRef{VersionID: "version-9", VersionNo: 9, AuthorKind: "user", CreatedAt: time.Now().UTC()},
	}

	plain := httptest.NewRecorder()
	writeCanvasStoreErrorAs(plain, moved, false)
	wantCanvasRefusal(t, plain, http.StatusConflict, canvas.CodeHeadMoved)
	head, ok := decodeCanvasErrorBody(t, plain).Error.Details["head"].(map[string]any)
	if !ok || head["version_id"] != "version-9" {
		t.Fatalf("an unwindowed caller was not told the head: %s", plain.Body.String())
	}

	windowed := httptest.NewRecorder()
	writeCanvasStoreErrorAs(windowed, moved, true)
	wantCanvasRefusal(t, windowed, http.StatusConflict, canvas.CodeHeadMoved)
	if got := decodeCanvasErrorBody(t, windowed).Error.Details["head"]; got != nil {
		t.Fatalf("a windowed caller was told the head: %v; body=%s", got, windowed.Body.String())
	}
}

// TestCanvasReadUnderAWindowIsShownOrHiddenByTheCanvas (gated on WIKI_TEST_DATABASE_URL): the same head read is answered by the store, so
// a member whose window starts after the canvas was created gets the typed 403, and one whose window starts before it reads the canvas.
func TestCanvasReadUnderAWindowIsShownOrHiddenByTheCanvas(t *testing.T) {
	repo := canvasTestRepo(t)
	org, space := canvasUniqueScope(t, repo)
	router := canvasRouter(t, repo, true)
	bearer := canvasBearer(t, org, []string{"wiki.read"}, false)
	seed, err := repo.Save(context.Background(), canvas.SaveInput{
		OrgID:          org,
		SpaceRef:       space,
		Content:        "# Room\n",
		IdempotencyKey: "seed-window-1",
		Author:         canvas.Author{Kind: canvas.AuthorUser, ID: canvasTestUser},
		Provenance:     canvas.Provenance{DecisionRef: "decision-seed-window-1", AuthorityRevision: 1, AudienceRevision: 1},
	})
	if err != nil || !seed.Inserted {
		t.Fatalf("seed the canvas = %+v, %v; want an inserted create", seed, err)
	}
	path := "/v1/space-canvas/spaces/" + space

	late := windowedHumanClaims(org, space, "read-window-late", false, seed.Version.CreatedAt.Add(time.Minute))
	rec := serveCanvas(t, router, http.MethodGet, path, bearer, canvasHumanToken(t, late, canvasReadOp("read-window-late")), "")
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeHistoryFloor)

	early := windowedHumanClaims(org, space, "read-window-early", false, seed.Version.CreatedAt.Add(-time.Minute))
	rec = serveCanvas(t, router, http.MethodGet, path, bearer, canvasHumanToken(t, early, canvasReadOp("read-window-early")), "")
	if rec.Code != http.StatusOK {
		t.Fatalf("a read inside the window = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}
}
