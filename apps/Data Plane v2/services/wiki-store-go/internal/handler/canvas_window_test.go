package handler

// Route tests for a history window on the room canvas (D18, WS04-S11, D21m). A windowed member reads and saves the current canvas
// like any member. What the window hides is history: a version written before it is refused, typed, on the get-version and restore
// routes, and the version list is filtered and says so in meta.window. The store decides the window; these tests run over the fake
// store, which answers the probe's InWindow, and the gated test runs the head read end to end.

import (
	"context"
	"encoding/json"
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

// windowedRouteToken signs a windowed human decision that is correct for route over the fake store's canvas.
func windowedRouteToken(t *testing.T, route canvasRoute, key string, at time.Time) string {
	t.Helper()
	return canvasRouteHumanToken(t, route, key, func(c *spacedecision.Claims) {
		c.HistoryFloorRevision = 1
		c.HistoryFloorAt = &at
	})
}

// TestCanvasWindowOnTheRoutes (D21m): what a windowed member gets on each route, for a version inside the window (or the head) and
// for one written before it. The head read and the save are never refused for the window. Get-version and restore of a pre-window
// version are the typed 403 space_history_floor_unsupported and use no store past the probe; inside the window they pass.
func TestCanvasWindowOnTheRoutes(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	at := time.Now().UTC().Add(-time.Hour)
	cases := []struct {
		route        string
		insideWindow bool
		wantStatus   int
		wantCode     string
	}{
		{"head", false, http.StatusOK, ""},
		{"save", false, http.StatusCreated, ""},
		{"list versions", false, http.StatusOK, ""},
		{"get version", true, http.StatusOK, ""},
		{"get version", false, http.StatusForbidden, spacedecision.CodeHistoryFloor},
		{"restore", true, http.StatusCreated, ""},
		{"restore", false, http.StatusForbidden, spacedecision.CodeHistoryFloor},
	}
	for _, tc := range cases {
		var route canvasRoute
		for _, r := range canvasRoutes {
			if r.name == tc.route {
				route = r
			}
		}
		name := tc.route + "/outside the window"
		if tc.insideWindow {
			name = tc.route + "/inside the window"
		}
		t.Run(name, func(t *testing.T) {
			store := fakeStoreWithCanvas()
			store.probe.InWindow = tc.insideWindow
			rec := serveCanvas(t, fakeCanvasRouter(t, store), route.method, route.path, bearer,
				windowedRouteToken(t, route, "window-"+route.name, at), route.body)
			if tc.wantCode != "" {
				wantCanvasRefusal(t, rec, tc.wantStatus, tc.wantCode)
				store.wantStoreUntouched(t)
				return
			}
			if rec.Code != tc.wantStatus {
				t.Fatalf("status = %d, want %d; body=%s", rec.Code, tc.wantStatus, rec.Body.String())
			}
			if store.probeFloor.At == nil || !store.probeFloor.At.Equal(at) || store.probeFloor.Revision != 1 {
				t.Fatalf("the probe was handed floor %+v, want the decision's window", store.probeFloor)
			}
			if route.name == "restore" && (store.restoreFloor.At == nil || store.restoreFloor.Revision != 1) {
				t.Fatalf("the restore was handed floor %+v, want the decision's window for the locked re-check", store.restoreFloor)
			}
		})
	}
}

// TestCanvasAgentWriteUnderAWindowIsRefusedBeforeTheStore: Control never issues an agent write under a window, so a run-action decision
// that carries one is refused after the not-found probe and before the store is written, whatever the operation.
func TestCanvasAgentWriteUnderAWindowIsRefusedBeforeTheStore(t *testing.T) {
	store := fakeStoreWithCanvas()
	defer store.wantStoreUntouched(t)
	router := fakeCanvasRouter(t, store)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.write"}, false)
	run := canvasRunClaims(canvasTestOrg, canvasTestSpace, "agent-window-1", true,
		runWriteArgs(saveBody{Content: "# Room\n", IdempotencyKey: "agent-window-1"}))
	at := time.Now().UTC().Add(-time.Hour)
	run.HistoryFloorAt = &at
	decision := signCanvasEnvelope(t, spacedecision.RunEnvelopeVersion, run)
	rec := serveCanvas(t, router, http.MethodPost, canvasSpacePath+"/versions", bearer, decision,
		canvasBody(t, map[string]any{"content": "# Room\n", "idempotency_key": "agent-window-1"}))
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeHistoryFloor)
}

// TestCanvasAgentReadUnderAWindowReadsTheHead (D21m): an agent read under a timestamp window reads the current head, as the member
// it acts for does.
func TestCanvasAgentReadUnderAWindowReadsTheHead(t *testing.T) {
	store := fakeStoreWithCanvas()
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read"}, false)
	at := time.Now().UTC().Add(-time.Hour)
	decision := canvasRouteRunToken(t, canvasRoutes[0], func(r *spacedecision.RunClaims) { r.HistoryFloorAt = &at })
	rec := serveCanvas(t, fakeCanvasRouter(t, store), http.MethodGet, canvasSpacePath, bearer, decision, "")
	if rec.Code != http.StatusOK {
		t.Fatalf("an agent head read under a window = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}
}

// TestCanvasVersionListSaysItIsWindowed (D21m): a windowed member's list carries meta.window with the window's start and whether
// older versions were withheld, and each listed version says whether it is the head. An unwindowed list carries no meta.window.
func TestCanvasVersionListSaysItIsWindowed(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read"}, false)
	list := canvasRoutes[1]
	at := time.Date(2026, time.November, 1, 10, 0, 0, 0, time.UTC)

	type listBody struct {
		Data []struct {
			VersionID string `json:"version_id"`
			Current   *bool  `json:"current"`
		} `json:"data"`
		Meta map[string]any `json:"meta"`
	}
	decode := func(rec *httptest.ResponseRecorder) listBody {
		t.Helper()
		if rec.Code != http.StatusOK {
			t.Fatalf("list = %d, want 200; body=%s", rec.Code, rec.Body.String())
		}
		var body listBody
		if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
			t.Fatalf("decode list %q: %v", rec.Body.String(), err)
		}
		return body
	}

	for _, hidden := range []bool{true, false} {
		store := fakeStoreWithCanvas()
		store.hiddenBefore = hidden
		body := decode(serveCanvas(t, fakeCanvasRouter(t, store), list.method, list.path, bearer,
			windowedRouteToken(t, list, "window-list", at), ""))
		window, ok := body.Meta["window"].(map[string]any)
		if !ok {
			t.Fatalf("a windowed list has no meta.window: %+v", body.Meta)
		}
		if window["starts_at"] != at.Format(time.RFC3339) || window["hidden_before"] != hidden {
			t.Fatalf("meta.window = %v, want starts_at %s and hidden_before %v", window, at.Format(time.RFC3339), hidden)
		}
		if len(body.Data) != 1 || body.Data[0].Current == nil || !*body.Data[0].Current {
			t.Fatalf("the listed head is not marked current: %+v", body.Data)
		}
		if store.listFloor.At == nil || !store.listFloor.At.Equal(at) {
			t.Fatalf("the list was handed floor %+v, want the decision's window", store.listFloor)
		}
	}

	plain := decode(serveCanvas(t, fakeCanvasRouter(t, fakeStoreWithCanvas()), list.method, list.path, bearer,
		canvasRouteHumanToken(t, list, "plain-list", nil), ""))
	if _, ok := plain.Meta["window"]; ok {
		t.Fatalf("an unwindowed list carries meta.window: %+v", plain.Meta)
	}
}

// TestCanvasHistoryFloorRefusalIsATypedForbidden: a version hidden by the window is answered as the typed 403 with the floor's code,
// not as the 503 a store failure gets.
func TestCanvasHistoryFloorRefusalIsATypedForbidden(t *testing.T) {
	rec := httptest.NewRecorder()
	writeCanvasStoreError(rec, &canvas.Error{Code: canvas.CodeHistoryFloor, Message: "the version is hidden from this member"})
	wantCanvasRefusal(t, rec, http.StatusForbidden, spacedecision.CodeHistoryFloor)
}

// TestCanvasHeadMovedNamesTheHead (D21m): a windowed member reads the current head, so a head-moved answer names it to every caller.
func TestCanvasHeadMovedNamesTheHead(t *testing.T) {
	moved := &canvas.Error{
		Code:    canvas.CodeHeadMoved,
		Message: "the canvas head moved since you opened it",
		Head:    &canvas.HeadRef{VersionID: "version-9", VersionNo: 9, AuthorKind: "user", CreatedAt: time.Now().UTC()},
	}
	rec := httptest.NewRecorder()
	writeCanvasStoreError(rec, moved)
	wantCanvasRefusal(t, rec, http.StatusConflict, canvas.CodeHeadMoved)
	head, ok := decodeCanvasErrorBody(t, rec).Error.Details["head"].(map[string]any)
	if !ok || head["version_id"] != "version-9" {
		t.Fatalf("the head-moved answer does not name the head: %s", rec.Body.String())
	}
}

// TestCanvasReadUnderAWindowShowsTheCurrentCanvas (gated on WIKI_TEST_DATABASE_URL, D21m): a member whose window starts after the
// canvas was created reads its head end to end, as one whose window starts before it does.
func TestCanvasReadUnderAWindowShowsTheCurrentCanvas(t *testing.T) {
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

	for name, at := range map[string]time.Time{
		"late":  seed.Version.CreatedAt.Add(time.Minute),
		"early": seed.Version.CreatedAt.Add(-time.Minute),
	} {
		claims := windowedHumanClaims(org, space, "read-window-"+name, false, at)
		rec := serveCanvas(t, router, http.MethodGet, path, bearer, canvasHumanToken(t, claims, canvasReadOp("read-window-"+name)), "")
		if rec.Code != http.StatusOK {
			t.Fatalf("a %s-window head read = %d, want 200; body=%s", name, rec.Code, rec.Body.String())
		}
	}
}
