package http

import (
	"encoding/json"
	"os"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

// D18: the two per-member history-window routes (migration 050, commit 903cce5ea) joined the Space surface
// registry's ctl: rows after the W3-DEC-REG set, so space_surface_registry_test.go does not list them. They are held
// to the same two-way agreement here: each row names this service and its own method and path, its handler is really
// defined where the row says, and the live router registers the route exactly once, bound to that handler. The row's
// ZDR class and flag say what Control does: both routes store a mode and an instant and no content, so both are n/a,
// and only the write sits behind the SPACE_MEMBER_HISTORY_WINDOWS rollout switch (the read always answers, because a
// stored window is always enforced).

// d18SurfaceRow reads the two fields the W3-DEC-REG rows do not need.
type d18SurfaceRow struct {
	spaceSurfaceRow
	MinRole string  `json:"min_role"`
	Flag    *string `json:"flag"`
}

// d18RouteWants is the registry's ctl: rows for the D18 history-window routes, with what each must say.
var d18RouteWants = []struct {
	id         string
	routeClass string
	flag       string // "" when the row names no flag
}{
	{"ctl:PUT:/api/v1/internal/spaces/:space_ref/members/:subject_id/history-window", "Config", "SPACE_MEMBER_HISTORY_WINDOWS"},
	{"ctl:GET:/api/v1/internal/spaces/:space_ref/members/:subject_id/history-window", "ReadMetadata", ""},
}

func TestSpaceSurfaceRegistryD18RowsMatchTheLiveRouter(t *testing.T) {
	raw, err := os.ReadFile(spaceSurfaceRegistryPath)
	if err != nil {
		t.Fatalf("read space-surface.v1.json: %v", err)
	}
	var contract struct {
		Rows []d18SurfaceRow `json:"rows"`
	}
	if err := json.Unmarshal(raw, &contract); err != nil {
		t.Fatalf("parse space-surface.v1.json: %v", err)
	}
	byID := make(map[string]d18SurfaceRow, len(d18RouteWants))
	for _, row := range contract.Rows {
		for _, want := range d18RouteWants {
			if row.ID != want.id {
				continue
			}
			if _, dup := byID[row.ID]; dup {
				t.Fatalf("row %s appears more than once in the registry", row.ID)
			}
			byID[row.ID] = row
		}
	}

	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	counts := routeCounts(server.router.Routes())
	byMethodPath := make(map[string]gin.RouteInfo, len(server.router.Routes()))
	for _, route := range server.router.Routes() {
		byMethodPath[route.Method+" "+route.Path] = route
	}

	for _, want := range d18RouteWants {
		row, ok := byID[want.id]
		if !ok {
			t.Errorf("%s: no row in the registry", want.id)
			continue
		}
		if row.Kind != "route" || row.Plane != "control" || row.Service != "user-core" {
			t.Errorf("%s: kind %q, plane %q, service %q; want route, control, user-core", row.ID, row.Kind, row.Plane, row.Service)
		}
		if id := "ctl:" + row.Method + ":" + row.Path; row.ID != id {
			t.Errorf("id %s does not encode its own method/path, want %s", row.ID, id)
		}
		if row.RouteClass != want.routeClass || row.DecisionType != "membership" || row.MinRole != "manager" {
			t.Errorf("%s: route_class %q, decision_type %q, min_role %q; want %s, membership, manager",
				row.ID, row.RouteClass, row.DecisionType, row.MinRole, want.routeClass)
		}
		// A window is a mode and an instant: no content, so it is allowed under every ZDR posture.
		if row.ZDRClass != "n/a" {
			t.Errorf("%s: zdr_class = %q, want n/a", row.ID, row.ZDRClass)
		}
		gotFlag := ""
		if row.Flag != nil {
			gotFlag = *row.Flag
		}
		if gotFlag != want.flag {
			t.Errorf("%s: flag = %q, want %q (empty means Control does not gate the route)", row.ID, gotFlag, want.flag)
		}

		key := row.Method + " " + row.Path
		if counts[key] != 1 {
			t.Errorf("%s: %d routes registered for %s, want exactly 1", row.ID, counts[key], key)
			continue
		}
		_, funcName, defined := handlerFuncDefined(t, row.Handler)
		if !defined {
			continue
		}
		if wantSuffix := "." + funcName + "-fm"; !strings.HasSuffix(byMethodPath[key].Handler, wantSuffix) {
			t.Errorf("%s: %s is wired to handler %q, want it bound to %s", row.ID, key, byMethodPath[key].Handler, funcName)
		}
	}
}
