package handler

// W3-DEC-REG (wiki part): the routes MountCanvasRoutes and MountCanvasPurgeRoute install each have a data: row in the Model Plane's
// space surface registry, with the ZDR class, guard, flag and handler that row names, and the registry names no data route the module
// does not mount. The registry is read in place, not copied. The routes are the ones chi reports for a router that mounts the
// production handlers, and the handler files are parsed (not compiled) to check that each row's function exists.

import (
	"encoding/json"
	"go/ast"
	"go/parser"
	"go/token"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
)

// surfaceRegistryPath is the space surface registry, relative to this package (internal/handler).
var surfaceRegistryPath = filepath.Join("..", "..", "..", "..", "..", "Model Plane", "api", "contracts", "space-surface.v1.json")

// moduleRoot is the wiki-store-go module root, the base of every data: row's handler path.
var moduleRoot = filepath.Join("..", "..")

type surfaceRow struct {
	ID       string  `json:"id"`
	Plane    string  `json:"plane"`
	Method   string  `json:"method"`
	Path     string  `json:"path"`
	Handler  string  `json:"handler"`
	ZDRClass string  `json:"zdr_class"`
	Basis    string  `json:"basis"`
	Flag     *string `json:"flag"`
}

// mountedRoute is one route a router serves, as chi reports it. purge marks the route MountCanvasPurgeRoute installs.
type mountedRoute struct {
	method, path string
	purge        bool
}

// mountedCanvasRoutes returns the routes MountCanvasRoutes and MountCanvasPurgeRoute install over the production handlers.
func mountedCanvasRoutes(t *testing.T) []mountedRoute {
	t.Helper()
	passthrough := func(next http.Handler) http.Handler { return next }
	canvasMux := chi.NewRouter()
	MountCanvasRoutes(canvasMux, passthrough, NewCanvasHandler(canvas.NewRepo(nil), true, nil))
	purgeMux := chi.NewRouter()
	MountCanvasPurgeRoute(purgeMux, passthrough, NewCanvasPurgeHandler(canvas.NewRepo(nil)))

	var routes []mountedRoute
	for _, group := range []struct {
		router chi.Router
		purge  bool
	}{{canvasMux, false}, {purgeMux, true}} {
		err := chi.Walk(group.router, func(method, route string, _ http.Handler, _ ...func(http.Handler) http.Handler) error {
			routes = append(routes, mountedRoute{method: method, path: route, purge: group.purge})
			return nil
		})
		if err != nil {
			t.Fatalf("walk the mounted canvas routes: %v", err)
		}
	}
	return routes
}

// declaresFunction reports whether the Go file at path declares a function or method named name. The file is parsed, not compiled.
func declaresFunction(t *testing.T, path, name string) bool {
	t.Helper()
	file, err := parser.ParseFile(token.NewFileSet(), path, nil, parser.SkipObjectResolution)
	if err != nil {
		t.Fatalf("parse %s: %v", path, err)
	}
	for _, decl := range file.Decls {
		if fn, ok := decl.(*ast.FuncDecl); ok && fn.Name.Name == name {
			return true
		}
	}
	return false
}

// wantHandlerDefined checks that the row's handler, "file::function" relative to the module root, exists and declares the function.
func wantHandlerDefined(t *testing.T, row surfaceRow) {
	t.Helper()
	file, fn, ok := strings.Cut(row.Handler, "::")
	if !ok || file == "" || fn == "" {
		t.Fatalf("%s: handler %q is not file::function", row.ID, row.Handler)
	}
	path := filepath.Join(moduleRoot, filepath.FromSlash(file))
	if !declaresFunction(t, path, fn) {
		t.Fatalf("%s: %s does not declare %s", row.ID, path, fn)
	}
}

// wantCanvasRow checks a row of MountCanvasRoutes: refused on zdr=true by the canvas guard, behind the canvas flag. The guard the basis
// names must be a function of the handler package.
func wantCanvasRow(t *testing.T, row surfaceRow) {
	t.Helper()
	if row.ZDRClass != "refuse" || row.Basis != "guard:refuseCanvasZDR" || row.Flag == nil || *row.Flag != "WIKISTORE_SPACE_CANVAS" {
		t.Fatalf("%s: zdr_class %q, basis %q, flag %v; want refuse, guard:refuseCanvasZDR, WIKISTORE_SPACE_CANVAS",
			row.ID, row.ZDRClass, row.Basis, row.Flag)
	}
	guard := strings.TrimPrefix(row.Basis, "guard:")
	if !declaresFunction(t, filepath.Join(moduleRoot, "internal", "handler", "canvas.go"), guard) {
		t.Fatalf("%s: basis names guard %s, which canvas.go does not declare", row.ID, guard)
	}
}

// wantPurgeRow checks the purge row: deletion is allowed under every posture and the route is not behind the canvas flag.
func wantPurgeRow(t *testing.T, row surfaceRow) {
	t.Helper()
	if row.ZDRClass != "n/a" || row.Flag != nil {
		t.Fatalf("%s: zdr_class %q, flag %v; want n/a and no flag", row.ID, row.ZDRClass, row.Flag)
	}
}

func findSurfaceRow(rows []surfaceRow, method, path string) (surfaceRow, bool) {
	for _, row := range rows {
		if row.Method == method && row.Path == path {
			return row, true
		}
	}
	return surfaceRow{}, false
}

func isMounted(routes []mountedRoute, method, path string) bool {
	for _, route := range routes {
		if route.method == method && route.path == path {
			return true
		}
	}
	return false
}

func TestCanvasRoutesHaveTheirSurfaceRegistryRows(t *testing.T) {
	raw, err := os.ReadFile(surfaceRegistryPath)
	if err != nil {
		t.Fatalf("read the space surface registry: %v", err)
	}
	var registry struct {
		Rows []surfaceRow `json:"rows"`
	}
	if err := json.Unmarshal(raw, &registry); err != nil {
		t.Fatalf("decode the space surface registry: %v", err)
	}
	var dataRows []surfaceRow
	for _, row := range registry.Rows {
		if row.Plane == "data" {
			dataRows = append(dataRows, row)
		}
	}

	mounted := mountedCanvasRoutes(t)
	for _, route := range mounted {
		t.Run(route.method+" "+route.path, func(t *testing.T) {
			row, ok := findSurfaceRow(dataRows, route.method, route.path)
			if !ok {
				t.Fatalf("%s %s is mounted but the registry has no data: row for it", route.method, route.path)
			}
			wantHandlerDefined(t, row)
			if route.purge {
				wantPurgeRow(t, row)
			} else {
				wantCanvasRow(t, row)
			}
		})
	}

	t.Run("no data row names a route the module does not mount", func(t *testing.T) {
		for _, row := range dataRows {
			if !isMounted(mounted, row.Method, row.Path) {
				t.Errorf("registry row %s is not mounted by the module", row.ID)
			}
		}
	})
}
