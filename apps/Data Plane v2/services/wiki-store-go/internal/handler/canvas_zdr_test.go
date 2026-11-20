package handler

// W3-DEC-WIKIZDR: a token or a decision that carries zero data retention is refused on every room canvas route, reads included,
// with the Model Plane catalogue refusal zdr_durable_mutation_forbidden (412). The catalogue entry is read from the contract, so the
// retryable flag, the action and the message the test expects are the contract's own values.

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/spacedecision"
)

// zdrCatalogueEntry is one row of the Model Plane ZDR refusal catalogue (apps/Model Plane/api/contracts/zdr-refusals.json).
type zdrCatalogueEntry struct {
	Action    string `json:"action"`
	Code      string `json:"code"`
	MessageEN string `json:"message_en"`
	Retryable bool   `json:"retryable"`
}

// catalogueDurableMutationRefusal returns the catalogue entry for zdr_durable_mutation_forbidden. The path is relative to this
// package and reaches the Model Plane contract read-only.
func catalogueDurableMutationRefusal(t *testing.T) zdrCatalogueEntry {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("..", "..", "..", "..", "..", "Model Plane", "api", "contracts", "zdr-refusals.json"))
	if err != nil {
		t.Fatalf("read the Model Plane ZDR refusal catalogue: %v", err)
	}
	var catalogue struct {
		Codes []zdrCatalogueEntry `json:"codes"`
	}
	if err := json.Unmarshal(raw, &catalogue); err != nil {
		t.Fatalf("decode the Model Plane ZDR refusal catalogue: %v", err)
	}
	for _, entry := range catalogue.Codes {
		if entry.Code == canvasZDRCode {
			return entry
		}
	}
	t.Fatalf("the Model Plane catalogue has no %s entry", canvasZDRCode)
	return zdrCatalogueEntry{}
}

// wantCanvasZDRRefusal asserts the 412 body: the catalogue code, its message, retryable false, action none, and the room canvas surface.
func wantCanvasZDRRefusal(t *testing.T, rec *httptest.ResponseRecorder) {
	t.Helper()
	entry := catalogueDurableMutationRefusal(t)
	if rec.Code != http.StatusPreconditionFailed {
		t.Fatalf("status = %d, want %d; body=%s", rec.Code, http.StatusPreconditionFailed, rec.Body.String())
	}
	var body struct {
		Error struct {
			Code      string            `json:"code"`
			Message   string            `json:"message"`
			Retryable *bool             `json:"retryable"`
			Action    string            `json:"action"`
			Details   map[string]string `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode refusal %q: %v", rec.Body.String(), err)
	}
	got := body.Error
	retryable := got.Retryable != nil && *got.Retryable
	if got.Code != entry.Code || got.Message != entry.MessageEN || got.Retryable == nil || retryable != entry.Retryable ||
		got.Action != entry.Action || got.Details["surface"] != canvasSurface {
		t.Fatalf("refusal code %q message %q retryable %v action %q surface %q; want code %q message %q retryable %v action %q surface %q; body=%s",
			got.Code, got.Message, retryable, got.Action, got.Details["surface"],
			entry.Code, entry.MessageEN, entry.Retryable, entry.Action, canvasSurface, rec.Body.String())
	}
}

// TestCanvasRoutesRefuseAZDRBearerWithTheCatalogueRefusalBeforeAnyHandler: a restrictive-ZDR bearer that holds the route's scope is
// refused on all five routes, reads included, before any decision is read or any handler runs.
func TestCanvasRoutesRefuseAZDRBearerWithTheCatalogueRefusalBeforeAnyHandler(t *testing.T) {
	router := canvasRouter(t, canvas.NewRepo(nil), true)
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, true)
	for _, route := range canvasRoutes {
		t.Run(route.name, func(t *testing.T) {
			// No decision is sent: the refusal comes before the decision is read.
			rec := serveCanvas(t, router, route.method, route.path, bearer, "", route.body)
			wantCanvasZDRRefusal(t, rec)
		})
	}
}

// TestCanvasRoutesRefuseAZDRHumanDecisionWithTheSameCatalogueRefusal: a human decision issued under zero data retention, presented by a
// bearer that is not restrictive, gets the same 412 on every route once the canvas exists. The decision is otherwise correct for its
// route (the action, the Space, the subject and the digest all match), so only the zero-retention check refuses it, and the store is
// not used past the not-found probe (D21n: not-found, then authority, then ZDR).
func TestCanvasRoutesRefuseAZDRHumanDecisionWithTheSameCatalogueRefusal(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	for _, route := range canvasRoutes {
		t.Run(route.name, func(t *testing.T) {
			store := fakeStoreWithCanvas()
			decision := canvasRouteHumanToken(t, route, "zdr-decision-key-1", func(c *spacedecision.Claims) { c.ZeroDataRetention = true })
			rec := serveCanvas(t, fakeCanvasRouter(t, store), route.method, route.path, bearer, decision, route.body)
			wantCanvasZDRRefusal(t, rec)
			store.wantStoreUntouched(t)
		})
	}
}

// TestCanvasAgentDecisionWithZDRGetsTheCatalogueRefusalOnTheRoutesThatAcceptOne: the head read and the save accept an agent decision,
// and an agent decision issued under zero data retention gets the same 412 there rather than a decision mismatch.
func TestCanvasAgentDecisionWithZDRGetsTheCatalogueRefusalOnTheRoutesThatAcceptOne(t *testing.T) {
	bearer := canvasBearer(t, canvasTestOrg, []string{"wiki.read", "wiki.write"}, false)
	for _, route := range canvasRoutes {
		if !route.agent {
			continue
		}
		t.Run(route.name, func(t *testing.T) {
			store := fakeStoreWithCanvas()
			decision := canvasRouteRunToken(t, route, func(r *spacedecision.RunClaims) { r.ZeroDataRetention = true })
			rec := serveCanvas(t, fakeCanvasRouter(t, store), route.method, route.path, bearer, decision, route.body)
			wantCanvasZDRRefusal(t, rec)
			store.wantStoreUntouched(t)
		})
	}
}
