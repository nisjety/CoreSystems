package grant

// The harness guard: newTestDB must apply every migrations/*.up.sql file.
//
// Production applies every *.up.sql on each container start
// (docker-entrypoint.sh), but the integration harness builds its own schema from
// the explicit list in newTestDB (postgres_integration_test.go). A migration that
// is missing from that list makes every store test fail with SQLSTATE 42703
// (undefined column). This test reads the list from the source, so it needs no
// database and runs in every `go test`, integration-tagged or not.

import (
	"go/ast"
	"go/parser"
	"go/token"
	"path/filepath"
	"strconv"
	"testing"
)

// harnessMigrations returns the migration file names that newTestDB applies, in
// source order.
func harnessMigrations(t *testing.T) []string {
	t.Helper()
	fset := token.NewFileSet()
	file, err := parser.ParseFile(fset, "postgres_integration_test.go", nil, 0)
	if err != nil {
		t.Fatalf("parse postgres_integration_test.go: %v", err)
	}
	var body *ast.BlockStmt
	for _, decl := range file.Decls {
		if fn, ok := decl.(*ast.FuncDecl); ok && fn.Name.Name == "newTestDB" {
			body = fn.Body
		}
	}
	if body == nil {
		t.Fatal("newTestDB not found in postgres_integration_test.go")
	}
	var names []string
	ast.Inspect(body, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		if fn, ok := call.Fun.(*ast.Ident); !ok || fn.Name != "applyMigration" || len(call.Args) != 3 {
			return true
		}
		lit, ok := call.Args[2].(*ast.BasicLit)
		if !ok || lit.Kind != token.STRING {
			t.Fatalf("newTestDB: applyMigration at %s must take a literal file name", fset.Position(call.Pos()))
		}
		name, err := strconv.Unquote(lit.Value)
		if err != nil {
			t.Fatalf("newTestDB: unquote %s: %v", lit.Value, err)
		}
		names = append(names, name)
		return true
	})
	return names
}

func TestNewTestDBAppliesEveryMigration(t *testing.T) {
	onDisk, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil {
		t.Fatal(err)
	}
	if len(onDisk) == 0 {
		t.Fatal("found no migrations/*.up.sql files: the guard reads the wrong directory")
	}
	listed := harnessMigrations(t)
	applied := make(map[string]bool, len(listed))
	for _, name := range listed {
		applied[name] = true
	}
	present := make(map[string]bool, len(onDisk))
	for _, path := range onDisk {
		name := filepath.Base(path)
		present[name] = true
		if !applied[name] {
			t.Errorf("%s is not applied by newTestDB: add applyMigration for it, in lexical order, or the store tests fail with an undefined column", name)
		}
	}
	for _, name := range listed {
		if !present[name] {
			t.Errorf("newTestDB applies %s, which is not in migrations/", name)
		}
	}
}
