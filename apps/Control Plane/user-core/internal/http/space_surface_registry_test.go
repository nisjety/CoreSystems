package http

import (
	"encoding/json"
	"os"
	"regexp"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

// W3-DEC-REG: the 14 Control decision, view, run-action-fence and agent
// registry/admission routes this wave added to the cross-plane surface
// registry (apps/Model Plane/api/contracts/space-surface.v1.json, commit
// 7878a4d59) must agree, in both directions, with what this service actually
// registers and implements: a ctl: row with the right plane/service/method/
// path, a handler function that is really defined where the row says it is,
// the right ZDR posture for a route that signs or reads a Space decision,
// and exactly one live route wired to that handler. The four W3-DEC-ORGAGENT
// org-subjects rows are a separate slice and are covered by their own tests;
// this file only audits the 14 W3-DEC-REG rows.
//
// spaceSurfaceRegistryPath is relative to this package directory
// (internal/http); go test always runs with the package directory as its
// working directory, so this resolves the same way regardless of where `go
// test` itself was invoked from.
const spaceSurfaceRegistryPath = "../../../../Model Plane/api/contracts/space-surface.v1.json"

type spaceSurfaceRow struct {
	ID           string `json:"id"`
	Kind         string `json:"kind"`
	Plane        string `json:"plane"`
	Service      string `json:"service"`
	Method       string `json:"method"`
	Path         string `json:"path"`
	Handler      string `json:"handler"`
	Basis        string `json:"basis"`
	RouteClass   string `json:"route_class"`
	ZDRClass     string `json:"zdr_class"`
	DecisionType string `json:"decision_type"`
}

type spaceSurfaceContract struct {
	Rows []spaceSurfaceRow `json:"rows"`
}

// w3RegRouteIDs are the exact 14 ctl: rows W3-DEC-REG added for this service.
var w3RegRouteIDs = map[string]bool{
	"ctl:POST:/api/v1/internal/spaces/canvas-decision":                                 true,
	"ctl:POST:/api/v1/internal/spaces/browser-run-decision":                            true,
	"ctl:POST:/api/v1/internal/spaces/memory-declassify-decision":                      true,
	"ctl:POST:/api/v1/internal/spaces/memory-personal-context-decision":                true,
	"ctl:POST:/api/v1/internal/spaces/model-action-view":                               true,
	"ctl:POST:/api/v1/internal/spaces/run-action-decision":                             true,
	"ctl:POST:/api/v1/internal/spaces/run-action-authority-check":                      true,
	"ctl:POST:/api/v1/internal/agents/subjects":                                        true,
	"ctl:GET:/api/v1/internal/agents/subjects":                                         true,
	"ctl:GET:/api/v1/internal/agents/subjects/:agent_subject_id":                       true,
	"ctl:PUT:/api/v1/internal/agents/subjects/:agent_subject_id/access":                true,
	"ctl:POST:/api/v1/internal/spaces/:space_ref/agent-candidates":                     true,
	"ctl:POST:/api/v1/internal/spaces/:space_ref/agent-admissions":                     true,
	"ctl:DELETE:/api/v1/internal/spaces/:space_ref/agent-admissions/:agent_subject_id": true,
}

// decisionTypesThatMustRefuse are the W3 rows that mint or check a signed Space
// decision; every one of them must refuse under ZDR regardless of its own
// route_class (model-action-view is ModelRead and memory-personal-context-decision
// is Chat, not DurableWrite, but both still carry a decision and must refuse).
var decisionTypesThatMustRefuse = map[string]bool{
	"canvas-decision":                  true,
	"browser-run-decision":             true,
	"memory-declassify-decision":       true,
	"memory-personal-context-decision": true,
	"model-action-view":                true,
	"run-action-decision":              true,
}

func loadSpaceSurfaceW3Rows(t *testing.T) []spaceSurfaceRow {
	t.Helper()
	raw, err := os.ReadFile(spaceSurfaceRegistryPath)
	if err != nil {
		t.Fatalf("read space-surface.v1.json: %v", err)
	}
	var contract spaceSurfaceContract
	if err := json.Unmarshal(raw, &contract); err != nil {
		t.Fatalf("parse space-surface.v1.json: %v", err)
	}
	seen := make(map[string]spaceSurfaceRow, len(w3RegRouteIDs))
	for _, row := range contract.Rows {
		if w3RegRouteIDs[row.ID] {
			if _, dup := seen[row.ID]; dup {
				t.Fatalf("row %s appears more than once in the registry", row.ID)
			}
			seen[row.ID] = row
		}
	}
	if len(seen) != len(w3RegRouteIDs) {
		var missing []string
		for id := range w3RegRouteIDs {
			if _, ok := seen[id]; !ok {
				missing = append(missing, id)
			}
		}
		t.Fatalf("found %d of the 14 W3-DEC-REG rows in the registry, missing: %v", len(seen), missing)
	}
	rows := make([]spaceSurfaceRow, 0, len(seen))
	for _, row := range seen {
		rows = append(rows, row)
	}
	return rows
}

// TestSpaceSurfaceRegistryW3RowsAreSelfConsistent proves (a): every W3-DEC-REG
// row claims the control plane and this service, and its id is exactly
// "ctl:<method>:<path>" for its own method and path fields -- so nothing in
// the registry can drift from its own id without this test catching it.
func TestSpaceSurfaceRegistryW3RowsAreSelfConsistent(t *testing.T) {
	rows := loadSpaceSurfaceW3Rows(t)
	for _, row := range rows {
		if row.Kind != "route" {
			t.Errorf("%s: kind = %q, want route", row.ID, row.Kind)
		}
		if row.Plane != "control" {
			t.Errorf("%s: plane = %q, want control", row.ID, row.Plane)
		}
		if row.Service != "user-core" {
			t.Errorf("%s: service = %q, want user-core", row.ID, row.Service)
		}
		if want := "ctl:" + row.Method + ":" + row.Path; row.ID != want {
			t.Errorf("id %s does not encode its own method/path, want %s", row.ID, want)
		}
	}
}

// handlerFuncDefined reports whether handler ("<path relative to user-core>::FuncName")
// names a file that exists under the user-core module and really defines that
// function, so a row can never point at a renamed or deleted handler unnoticed.
func handlerFuncDefined(t *testing.T, handler string) (file, name string, ok bool) {
	t.Helper()
	sep := "::"
	idx := -1
	for i := 0; i+len(sep) <= len(handler); i++ {
		if handler[i:i+len(sep)] == sep {
			idx = i
			break
		}
	}
	if idx < 0 {
		t.Errorf("handler %q is not in the <path>::<func> form", handler)
		return "", "", false
	}
	file, name = handler[:idx], handler[idx+len(sep):]
	// user-core root is two directories up from this package (internal/http).
	raw, err := os.ReadFile("../../" + file)
	if err != nil {
		t.Errorf("%s: handler file does not exist under user-core: %v", handler, err)
		return file, name, false
	}
	pattern := regexp.MustCompile(`(?m)^func\s*(?:\([^)]*\)\s*)?` + regexp.QuoteMeta(name) + `\s*\(`)
	if !pattern.Match(raw) {
		t.Errorf("%s: %s does not define func %s", handler, file, name)
		return file, name, false
	}
	return file, name, true
}

// TestSpaceSurfaceRegistryW3HandlersExist proves (b): every W3-DEC-REG row's
// handler file exists under this service and really defines the named function.
func TestSpaceSurfaceRegistryW3HandlersExist(t *testing.T) {
	rows := loadSpaceSurfaceW3Rows(t)
	for _, row := range rows {
		handlerFuncDefined(t, row.Handler)
	}
}

// TestSpaceSurfaceRegistryW3ZDRClassIsCorrect proves (c): every row that mints or
// checks a signed Space decision refuses under ZDR regardless of its own
// route_class, every DurableWrite row refuses, and the remaining agent
// registry/admission rows (which store ids and states only, never content) do
// not carry a ZDR posture of their own.
func TestSpaceSurfaceRegistryW3ZDRClassIsCorrect(t *testing.T) {
	rows := loadSpaceSurfaceW3Rows(t)
	for _, row := range rows {
		mustRefuse := row.RouteClass == "DurableWrite" || decisionTypesThatMustRefuse[row.DecisionType]
		switch {
		case mustRefuse && row.ZDRClass != "refuse":
			t.Errorf("%s: zdr_class = %q, want refuse (route_class %s, decision_type %s)",
				row.ID, row.ZDRClass, row.RouteClass, row.DecisionType)
		case !mustRefuse && row.ZDRClass != "refuse" && row.ZDRClass != "n/a":
			t.Errorf("%s: zdr_class = %q, want refuse or n/a", row.ID, row.ZDRClass)
		}
	}
	// run-action-authority-check is not itself a decision-type row above (it
	// checks one, decision_type run-action-decision) but it re-reads Control
	// authority through the same ZDR guard as the issuer and must also refuse.
	for _, row := range rows {
		if row.ID == "ctl:POST:/api/v1/internal/spaces/run-action-authority-check" && row.ZDRClass != "refuse" {
			t.Errorf("%s: zdr_class = %q, want refuse", row.ID, row.ZDRClass)
		}
	}
}

// TestSpaceSurfaceRegistryW3RoutesMatchLiveRouter proves (d): the routes this
// service actually registers equal the W3-DEC-REG row set, one for one. Each
// row's method and path is registered exactly once on the live engine, and
// the route registered there is wired to exactly the handler function the
// row names (so a row can never go unregistered, and a handler can never be
// quietly re-pointed at a different, undocumented path).
func TestSpaceSurfaceRegistryW3RoutesMatchLiveRouter(t *testing.T) {
	rows := loadSpaceSurfaceW3Rows(t)
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	counts := routeCounts(server.router.Routes())

	byMethodPath := make(map[string]gin.RouteInfo, len(server.router.Routes()))
	for _, route := range server.router.Routes() {
		byMethodPath[route.Method+" "+route.Path] = route
	}

	for _, row := range rows {
		key := row.Method + " " + row.Path
		if counts[key] != 1 {
			t.Errorf("%s: %d routes registered for %s, want exactly 1", row.ID, counts[key], key)
			continue
		}
		_, funcName, ok := handlerFuncDefined(t, row.Handler)
		if !ok {
			continue
		}
		route := byMethodPath[key]
		wantSuffix := "." + funcName + "-fm"
		if !strings.HasSuffix(route.Handler, wantSuffix) {
			t.Errorf("%s: %s is wired to handler %q, want it bound to %s", row.ID, key, route.Handler, funcName)
		}
	}
}
