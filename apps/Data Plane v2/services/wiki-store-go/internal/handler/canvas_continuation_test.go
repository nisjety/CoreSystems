package handler

// Tests of the approved-continuation lane of the canvas save (D22z, canvas_continuation.go). They run over fakeCanvasStore, so they
// need no database.

import (
	"context"
	"net/http"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/spacedecision"
)

// recordingCanvasStore is fakeCanvasStore that also keeps the last save it was handed.
type recordingCanvasStore struct {
	*fakeCanvasStore
	lastSave canvas.SaveInput
}

func (s *recordingCanvasStore) Save(ctx context.Context, in canvas.SaveInput) (canvas.WriteResult, error) {
	s.lastSave = in
	return s.fakeCanvasStore.Save(ctx, in)
}

// continuationRouter mounts the canvas routes, flag on, over store with the continuation lane as given.
func continuationRouter(t *testing.T, store canvasStore, continuation bool) http.Handler {
	t.Helper()
	_, verifier := canvasTestSigner(t)
	router := chi.NewRouter()
	MountCanvasRoutes(router, authctx.Middleware(verifier),
		newCanvasHandler(store, true, canvasDecisionKeys()).WithContinuation(continuation))
	return router
}

// continuationBearer is execution-core's data-plane service bearer for the tenant org with the given scopes.
func continuationBearer(t *testing.T, serviceID string, scopes ...string) string {
	t.Helper()
	return canvasServiceBearer(t, serviceID, canvasTestOrg, scopes)
}

var continuationScopes = []string{"wiki.write", canvasContinuationScope}

func canvasSaveRoute() canvasRoute { return canvasRoutes[3] }

// With the lane on, execution-core's service bearer and the run's agent decision save the canvas as the requester: the author is the
// run, on behalf of the human the Control decision names, and the provenance is that decision's.
func TestCanvasContinuationSavesAsTheRequesterNamedByTheDecision(t *testing.T) {
	store := &recordingCanvasStore{fakeCanvasStore: fakeStoreWithCanvas()}
	router := continuationRouter(t, store, true)
	save := canvasSaveRoute()
	decision := canvasRouteRunToken(t, save, nil)
	rec := serveCanvas(t, router, save.method, save.path, continuationBearer(t, canvasContinuationServiceID, continuationScopes...),
		decision, save.body)
	if rec.Code != http.StatusCreated {
		t.Fatalf("continuation save = %d, want 201; body=%s", rec.Code, rec.Body.String())
	}
	author := store.lastSave.Author
	if author.Kind != canvas.AuthorAgent || author.RunID != canvasTestRunID || author.OnBehalfOf != canvasTestUser {
		t.Fatalf("author = %+v, want the run on behalf of %s", author, canvasTestUser)
	}
	if store.lastSave.OrgID != canvasTestOrg || store.lastSave.Provenance.DecisionRef != "decision-run-save-key-1" {
		t.Fatalf("save input = %+v", store.lastSave)
	}
}

// Every way a service bearer can fall outside the lane is refused before the store, with the same mismatch a service gets today.
func TestCanvasContinuationRefusesEverythingOutsideTheLane(t *testing.T) {
	save := canvasSaveRoute()
	agentDecision := func(t *testing.T) string { return canvasRouteRunToken(t, save, nil) }
	humanDecision := func(t *testing.T) string { return canvasRouteHumanToken(t, save, "save-key-1", nil) }
	cases := []struct {
		name         string
		continuation bool
		bearer       func(t *testing.T) string
		decision     func(t *testing.T) string
		status       int
		code         string
	}{
		{"the lane is off", false,
			func(t *testing.T) string { return continuationBearer(t, canvasContinuationServiceID, continuationScopes...) },
			agentDecision, http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		{"another service", true,
			func(t *testing.T) string { return continuationBearer(t, "service:other-plane", continuationScopes...) },
			agentDecision, http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		{"no continuation scope", true,
			func(t *testing.T) string { return continuationBearer(t, canvasContinuationServiceID, "wiki.write") },
			agentDecision, http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		{"a human decision from a service", true,
			func(t *testing.T) string { return continuationBearer(t, canvasContinuationServiceID, continuationScopes...) },
			humanDecision, http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		{"a decision for another org", true,
			func(t *testing.T) string {
				return canvasServiceBearer(t, canvasContinuationServiceID, "org-other", continuationScopes)
			},
			agentDecision, http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		{"a decision for another Space", true,
			func(t *testing.T) string { return continuationBearer(t, canvasContinuationServiceID, continuationScopes...) },
			func(t *testing.T) string {
				return canvasRouteRunToken(t, save, func(run *spacedecision.RunClaims) { run.SpaceRef = "space-other" })
			}, http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		{"a read decision on the save", true,
			func(t *testing.T) string { return continuationBearer(t, canvasContinuationServiceID, continuationScopes...) },
			func(t *testing.T) string {
				return canvasRouteRunToken(t, save, func(run *spacedecision.RunClaims) {
					run.ActionID, run.ActionSchemaHash = spacedecision.ActionRunCanvasRead, spacedecision.SchemaRunCanvasRead
					run.Permissions = []string{spacedecision.PermissionCanvasRead}
				})
			}, http.StatusForbidden, spacedecision.CodeDecisionMismatch},
		{"a decision under a history floor", true,
			func(t *testing.T) string { return continuationBearer(t, canvasContinuationServiceID, continuationScopes...) },
			func(t *testing.T) string {
				return canvasRouteRunToken(t, save, func(run *spacedecision.RunClaims) {
					at := run.IssuedAt
					run.HistoryFloorAt = &at
				})
			}, http.StatusForbidden, spacedecision.CodeHistoryFloor},
		{"a decision under zero data retention", true,
			func(t *testing.T) string { return continuationBearer(t, canvasContinuationServiceID, continuationScopes...) },
			func(t *testing.T) string {
				return canvasRouteRunToken(t, save, func(run *spacedecision.RunClaims) { run.ZeroDataRetention = true })
			}, http.StatusPreconditionFailed, canvasZDRCode},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			store := fakeStoreWithCanvas()
			router := continuationRouter(t, store, tc.continuation)
			rec := serveCanvas(t, router, save.method, save.path, tc.bearer(t), tc.decision(t), save.body)
			wantCanvasRefusal(t, rec, tc.status, tc.code)
			store.wantStoreUntouched(t)
		})
	}
}

// The lane is the save's only: with it on, every other route still refuses the continuation bearer, the head read included.
func TestCanvasContinuationIsServedOnTheSaveOnly(t *testing.T) {
	bearer := continuationBearer(t, canvasContinuationServiceID, "wiki.read", "wiki.write", canvasContinuationScope)
	for _, route := range canvasRoutes {
		if route.name == "save" {
			continue
		}
		t.Run(route.name, func(t *testing.T) {
			store := fakeStoreWithCanvas()
			router := continuationRouter(t, store, true)
			decision := canvasRouteHumanToken(t, route, "read-key-1", nil)
			if route.agent {
				decision = canvasRouteRunToken(t, route, nil)
			}
			rec := serveCanvas(t, router, route.method, route.path, bearer, decision, route.body)
			wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeDecisionMismatch)
			store.wantStoreUntouched(t)
		})
	}
}
