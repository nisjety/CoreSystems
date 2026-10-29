package rawstore

import (
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

// T11: constraint 2 holds in the code. Every string literal of the non-test
// sources that names an evidence table must be free of UPDATE, DELETE, TRUNCATE,
// ALTER and DROP. INSERT ... ON CONFLICT DO NOTHING is allowed. The scan reads
// the parsed literals, not the text, so comments do not count.
func TestRawstoreNeverMutatesEvidence(t *testing.T) {
	t.Parallel()

	evidence := []string{"raw_documents", "raw_search_pages", "download_attempts"}
	// Check mutation targets, allowing a rebuildable projection to join evidence.
	mutating := regexp.MustCompile(`(?i)\b(?:update\s+|delete\s+from\s+|truncate\s+(?:table\s+)?|alter\s+table\s+|drop\s+table\s+)(?:public\.)?(?:raw_documents|raw_search_pages|download_attempts)\b`)

	entries, err := os.ReadDir(".")
	if err != nil {
		t.Fatalf("read package directory: %v", err)
	}
	fset := token.NewFileSet()
	evidenceLiterals := 0
	for _, e := range entries {
		name := e.Name()
		if e.IsDir() || !strings.HasSuffix(name, ".go") || strings.HasSuffix(name, "_test.go") {
			continue
		}
		file, err := parser.ParseFile(fset, name, nil, parser.SkipObjectResolution)
		if err != nil {
			t.Fatalf("parse %s: %v", name, err)
		}
		ast.Inspect(file, func(n ast.Node) bool {
			lit, ok := n.(*ast.BasicLit)
			if !ok || lit.Kind != token.STRING {
				return true
			}
			value, err := strconv.Unquote(lit.Value)
			if err != nil {
				t.Fatalf("%s: unquote literal: %v", name, err)
			}
			if !namesAny(value, evidence) {
				return true
			}
			evidenceLiterals++
			if m := mutating.FindString(value); m != "" {
				t.Errorf("%s: %q on an evidence table in SQL: %q", name, m, value)
			}
			return true
		})
	}
	if evidenceLiterals == 0 {
		t.Fatal("the scan found no SQL that names an evidence table, so it checks nothing")
	}
}

func namesAny(s string, names []string) bool {
	for _, n := range names {
		if strings.Contains(s, n) {
			return true
		}
	}
	return false
}
