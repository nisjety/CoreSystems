package actions

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"sort"
	"strconv"
	"strings"
	"testing"
)

// The golden for the provider reads the agent may call (WS23-S2). It lists the
// Microsoft and Google read operations the executor serves, with the capability
// and sensitivity that requiredCapabilityForOperation gives them, and the writes
// deliberately kept out of the read catalogue. This test reads the executor's
// switch and the capability table from source, so a read arm added without a
// golden row fails here, and so does a golden row the code no longer serves.
//
// The Rust catalogue in execution-core and the mp-contracts table are checked
// against a byte-identical copy of this file (readOperationsVendored).
//
// Regenerate with `go test -update`, then re-pin readOperationsSHA256 with the
// digest the run prints and copy the golden over the vendored copy.
const (
	readOperationsGolden   = "testdata/provider-read-operations.json"
	readOperationsVendored = "../../../../Model Plane/rust/crates/mp-contracts/tests/fixtures/provider-read-operations.json"
	readOperationsSHA256   = "efc6009715aa27e1b0fb916ad6b6ddbaf84882abf845a4c22f5bb47b45499e4e"
	executorSource         = "service.go"
	capabilitySource       = "../api/server.go"
)

var updateReadOperations = flag.Bool("update", false, "regenerate provider-read-operations.json from the executor and the capability table")

type readGolden struct {
	Version        int       `json:"version"`
	Operations     []readRow `json:"operations"`
	ExcludedWrites []opRef   `json:"excludedWrites"`
}

type readRow struct {
	Provider   string   `json:"provider"`
	Operation  string   `json:"operation"`
	Params     []string `json:"params"`
	Capability string   `json:"capability"`
	Sensitive  bool     `json:"sensitive"`
}

type opRef struct {
	Provider  string `json:"provider"`
	Operation string `json:"operation"`
}

type capabilityEntry struct {
	capability string
	sensitive  bool
}

// goldenProviders are the providers whose executor functions the golden covers.
var goldenProviders = []string{"microsoft", "google"}

func loadReadGolden(t *testing.T) ([]byte, readGolden) {
	t.Helper()
	raw, err := os.ReadFile(readOperationsGolden)
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var golden readGolden
	if err := decoder.Decode(&golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	return raw, golden
}

func TestReadOperationsGoldenIsPinned(t *testing.T) {
	if *updateReadOperations {
		t.Skip("golden is being regenerated; re-pin readOperationsSHA256 after the update")
	}
	raw, _ := loadReadGolden(t)
	sum := sha256.Sum256(raw)
	if got := hex.EncodeToString(sum[:]); got != readOperationsSHA256 {
		t.Fatalf("golden sha256 is %s, pinned %s: a change to the golden is a reviewed change, so re-pin readOperationsSHA256 and copy the file to %s", got, readOperationsSHA256, readOperationsVendored)
	}
}

func TestReadOperationsVendoredCopyIsByteIdentical(t *testing.T) {
	if *updateReadOperations {
		t.Skip("golden is being regenerated; copy it over the vendored file after the update")
	}
	golden, err := os.ReadFile(readOperationsGolden)
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	vendored, err := os.ReadFile(readOperationsVendored)
	if err != nil {
		t.Fatalf("read the Rust copy %s: %v", readOperationsVendored, err)
	}
	if !bytes.Equal(golden, vendored) {
		t.Fatalf("%s differs from the golden; copy the golden over it", readOperationsVendored)
	}
}

func TestReadOperationsMatchExecutorAndCapabilityTable(t *testing.T) {
	_, golden := loadReadGolden(t)
	executor := executorOperations(t)
	capabilities := capabilityTable(t)

	if *updateReadOperations {
		regenerated := regenerateReadGolden(t, golden, executor, capabilities)
		raw, err := json.MarshalIndent(regenerated, "", "  ")
		if err != nil {
			t.Fatalf("encode golden: %v", err)
		}
		raw = append(raw, '\n')
		if err := os.WriteFile(readOperationsGolden, raw, 0o644); err != nil {
			t.Fatalf("write golden: %v", err)
		}
		sum := sha256.Sum256(raw)
		t.Logf("golden regenerated; new sha256 %s", hex.EncodeToString(sum[:]))
		return
	}

	excluded := map[string]map[string]bool{}
	for _, p := range goldenProviders {
		excluded[p] = map[string]bool{}
	}
	for _, w := range golden.ExcludedWrites {
		excluded[w.Provider][w.Operation] = true
	}

	expected := map[string]map[string]bool{}
	for _, p := range goldenProviders {
		expected[p] = map[string]bool{}
	}
	seen := map[string]bool{}
	for _, row := range golden.Operations {
		if _, ok := expected[row.Provider]; !ok {
			t.Errorf("golden row for unknown provider %q (%s)", row.Provider, row.Operation)
			continue
		}
		key := row.Provider + "/" + row.Operation
		if seen[key] {
			t.Errorf("golden lists %s twice", key)
		}
		seen[key] = true
		for _, name := range spellings(row.Provider, row.Operation) {
			expected[row.Provider][name] = true
			entry, ok := capabilities[row.Provider][name]
			if !ok {
				t.Errorf("%s %q has no capability entry in requiredCapabilityForOperation", row.Provider, name)
				continue
			}
			if entry.capability != row.Capability || entry.sensitive != row.Sensitive {
				t.Errorf("%s %q: golden says %s sensitive=%t, requiredCapabilityForOperation says %s sensitive=%t",
					row.Provider, name, row.Capability, row.Sensitive, entry.capability, entry.sensitive)
			}
		}
	}

	for _, p := range goldenProviders {
		for _, w := range golden.ExcludedWrites {
			if w.Provider == p {
				for _, name := range spellings(p, w.Operation) {
					expected[p][name] = true
				}
			}
		}
		for name := range executor[p] {
			if !expected[p][name] {
				t.Errorf("executor serves %s %q but the golden neither lists it as a read nor excludes it as a write", p, name)
			}
		}
		for name := range expected[p] {
			if !executor[p][name] {
				t.Errorf("golden names %s %q but the executor does not serve it", p, name)
			}
		}
	}
}

// spellings are the two names the executor accepts for one operation: the bare
// name and the provider-prefixed alias.
func spellings(provider, operation string) []string {
	return []string{operation, provider + "." + operation}
}

// executorOperations returns the operation names each provider executor function
// in service.go accepts, from its switch cases. Read and write names alike.
func executorOperations(t *testing.T) map[string]map[string]bool {
	t.Helper()
	file := parseSource(t, executorSource)
	functions := map[string]string{"executeMicrosoft": "microsoft", "executeGoogle": "google"}
	out := map[string]map[string]bool{}
	for _, p := range goldenProviders {
		out[p] = map[string]bool{}
	}
	for _, decl := range file.Decls {
		fn, ok := decl.(*ast.FuncDecl)
		if !ok || fn.Body == nil {
			continue
		}
		provider, ok := functions[fn.Name.Name]
		if !ok {
			continue
		}
		ast.Inspect(fn.Body, func(n ast.Node) bool {
			clause, ok := n.(*ast.CaseClause)
			if !ok {
				return true
			}
			for _, expr := range clause.List {
				if name, ok := stringLiteral(expr); ok {
					out[provider][name] = true
				}
			}
			return true
		})
	}
	for _, p := range goldenProviders {
		if len(out[p]) == 0 {
			t.Fatalf("found no %s cases in %s", p, executorSource)
		}
	}
	return out
}

// capabilityTable reads requiredCapabilityForOperation: for each provider case
// and operation case, the capability it returns and whether it is sensitive.
func capabilityTable(t *testing.T) map[string]map[string]capabilityEntry {
	t.Helper()
	file := parseSource(t, capabilitySource)
	var fn *ast.FuncDecl
	for _, decl := range file.Decls {
		if candidate, ok := decl.(*ast.FuncDecl); ok && candidate.Name.Name == "requiredCapabilityForOperation" {
			fn = candidate
		}
	}
	if fn == nil {
		t.Fatalf("requiredCapabilityForOperation not found in %s", capabilitySource)
	}
	out := map[string]map[string]capabilityEntry{}
	for _, stmt := range fn.Body.List {
		outer, ok := stmt.(*ast.SwitchStmt)
		if !ok {
			continue
		}
		for _, providerStmt := range outer.Body.List {
			providerClause := providerStmt.(*ast.CaseClause)
			provider, ok := stringLiteral(providerClause.List[0])
			if !ok {
				t.Fatalf("provider case in requiredCapabilityForOperation is not a string")
			}
			out[provider] = map[string]capabilityEntry{}
			for _, inner := range providerClause.Body {
				operationSwitch, ok := inner.(*ast.SwitchStmt)
				if !ok {
					continue
				}
				for _, opStmt := range operationSwitch.Body.List {
					opClause := opStmt.(*ast.CaseClause)
					ret, ok := opClause.Body[0].(*ast.ReturnStmt)
					if !ok || len(ret.Results) != 2 {
						t.Fatalf("operation case for %s does not return a capability pair", provider)
					}
					capability, ok := stringLiteral(ret.Results[0])
					if !ok {
						t.Fatalf("capability for %s is not a string literal", provider)
					}
					sensitive := ret.Results[1].(*ast.Ident).Name == "true"
					for _, expr := range opClause.List {
						if name, ok := stringLiteral(expr); ok {
							out[provider][name] = capabilityEntry{capability: capability, sensitive: sensitive}
						}
					}
				}
			}
		}
	}
	return out
}

// regenerateReadGolden rebuilds the operation rows from the executor and the
// capability table. Parameter lists are kept from the existing golden; a new
// operation starts with none, to be filled in by hand.
func regenerateReadGolden(t *testing.T, previous readGolden, executor map[string]map[string]bool, capabilities map[string]map[string]capabilityEntry) readGolden {
	t.Helper()
	params := map[string][]string{}
	for _, row := range previous.Operations {
		params[row.Provider+"/"+row.Operation] = row.Params
	}
	out := readGolden{Version: previous.Version, ExcludedWrites: previous.ExcludedWrites}
	for _, p := range goldenProviders {
		excluded := map[string]bool{}
		for _, w := range previous.ExcludedWrites {
			if w.Provider == p {
				excluded[w.Operation] = true
			}
		}
		operations := map[string]bool{}
		for name := range executor[p] {
			operation := strings.TrimPrefix(name, p+".")
			if !excluded[operation] {
				operations[operation] = true
			}
		}
		for operation := range operations {
			entry, ok := capabilities[p][operation]
			if !ok {
				t.Fatalf("%s %q has no capability entry", p, operation)
			}
			out.Operations = append(out.Operations, readRow{
				Provider:   p,
				Operation:  operation,
				Params:     nonNil(params[p+"/"+operation]),
				Capability: entry.capability,
				Sensitive:  entry.sensitive,
			})
		}
	}
	sort.SliceStable(out.Operations, func(i, j int) bool {
		if out.Operations[i].Provider != out.Operations[j].Provider {
			return out.Operations[i].Provider < out.Operations[j].Provider
		}
		return out.Operations[i].Operation < out.Operations[j].Operation
	})
	return out
}

func nonNil(values []string) []string {
	if values == nil {
		return []string{}
	}
	return values
}

func parseSource(t *testing.T, path string) *ast.File {
	t.Helper()
	file, err := parser.ParseFile(token.NewFileSet(), path, nil, 0)
	if err != nil {
		t.Fatalf("parse %s: %v", path, err)
	}
	return file
}

func stringLiteral(expr ast.Expr) (string, bool) {
	lit, ok := expr.(*ast.BasicLit)
	if !ok || lit.Kind != token.STRING {
		return "", false
	}
	value, err := strconv.Unquote(lit.Value)
	if err != nil {
		return "", false
	}
	return value, true
}
