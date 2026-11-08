package api

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"go/ast"
	"go/parser"
	"go/token"
	"maps"
	"os"
	"slices"
	"sort"
	"strconv"
	"testing"

	"github.com/triodelab/integration-corev2/internal/providers"
)

// The write-operations golden (EP-10-S2) names every write the provider executor serves.
// `operations` are the writes execution-core catalogues, so the agent may call them.
// `excludedWrites` are the other writes the approval allowlist gates; execution-core treats
// those as uncatalogued and pauses them for approval as well.
//
// This test reads each provider executor's operation switch from service.go and asks
// actionRequiresApproval and the provider catalogue about every write arm it finds. A write arm
// without a golden row fails here, a golden row the executor no longer serves fails here, and an
// arm that sends a POST, PUT, PATCH or DELETE without an allowlist name fails here, before the
// approval gate can be skipped (register D2-718).
//
// The Rust table in mp-contracts is checked against a byte-identical copy of the golden
// (writeOperationsVendored), and execution-core's catalogue against that table.
//
// Regenerate with `go test -update`, then re-pin writeOperationsSHA256 with the digest the run
// prints and copy the golden over the vendored copy.
const (
	writeOperationsGolden   = "../actions/testdata/provider-write-operations.json"
	writeOperationsVendored = "../../../../Model Plane/rust/crates/mp-contracts/tests/fixtures/provider-write-operations.json"
	writeOperationsSHA256   = "5e69423b54f6856fc8f70c6d2beafc5ed29653c431a80a1d215c2fcf994e7eaf"
	writeExecutorSource     = "../actions/service.go"
)

var updateWriteOperations = flag.Bool("update", false, "regenerate provider-write-operations.json from the executor and the allowlist")

type writeGolden struct {
	Version        int          `json:"version"`
	Operations     []writeRow   `json:"operations"`
	ExcludedWrites []writeEntry `json:"excludedWrites"`
}

// writeRow is a write that execution-core catalogues as a write.
type writeRow struct {
	Provider         string   `json:"provider"`
	Operation        string   `json:"operation"`
	Aliases          []string `json:"aliases"`
	ApprovalRequired bool     `json:"approval_required"`
	Direction        string   `json:"direction"`
	EditableKind     string   `json:"editable_kind"`
}

// writeEntry is a write the allowlist gates and the catalogue leaves out.
type writeEntry struct {
	Provider  string   `json:"provider"`
	Operation string   `json:"operation"`
	Aliases   []string `json:"aliases"`
}

// writeArm is one case clause of a provider executor's operation switch. names[0] is the
// operation and the rest are the other spellings the executor accepts for it.
type writeArm struct {
	provider string
	names    []string
	gated    []bool // gated[i] reports whether actionRequiresApproval names names[i]
	mutates  bool   // the clause calls a helper that sends a request body
}

// readsOverPost are executor arms that POST a search or a query and only read. They are not
// writes, so the allowlist must not gate them either.
var readsOverPost = map[string]bool{
	"google/gsc.search_analytics.query": true,
	"notion/databases":                  true,
	"notion/pages":                      true,
}

// writeExecutorFunctions maps each provider executor in service.go to its provider key.
var writeExecutorFunctions = map[string]string{
	"executeMicrosoft": "microsoft",
	"executeSlack":     "slack",
	"executeGoogle":    "google",
	"executeGitHub":    "github",
	"executeNotion":    "notion",
	"executeShopify":   "shopify",
	"executeStripe":    "stripe",
	"executeLinkedIn":  "linkedin",
	"executeMeta":      "meta",
	"executeSnapchat":  "snapchat",
	"executeOkta":      "okta",
}

// mutatingHelpers are the service.go helpers that send a request body to the provider.
var mutatingHelpers = map[string]bool{
	"post":           true,
	"postBearer":     true,
	"postBearerForm": true,
	"patchBearer":    true,
}

var mutatingMethods = map[string]bool{
	"MethodPost":   true,
	"MethodPut":    true,
	"MethodPatch":  true,
	"MethodDelete": true,
}

func (arm writeArm) key() string {
	return arm.provider + "/" + arm.names[0]
}

func (arm writeArm) gatedAll() bool {
	for _, gated := range arm.gated {
		if !gated {
			return false
		}
	}
	return true
}

func (arm writeArm) gatedAny() bool {
	for _, gated := range arm.gated {
		if gated {
			return true
		}
	}
	return false
}

// isWrite reports whether the arm is a write: the allowlist gates one of its spellings, or it
// sends a request body and is not a read posted as JSON.
func (arm writeArm) isWrite() bool {
	return arm.gatedAny() || (arm.mutates && !readsOverPost[arm.key()])
}

func loadWriteGolden(t *testing.T) ([]byte, writeGolden) {
	t.Helper()
	raw, err := os.ReadFile(writeOperationsGolden)
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var golden writeGolden
	if err := decoder.Decode(&golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	return raw, golden
}

func TestWriteOperationsGoldenIsPinned(t *testing.T) {
	if *updateWriteOperations {
		t.Skip("golden is being regenerated; re-pin writeOperationsSHA256 after the update")
	}
	raw, _ := loadWriteGolden(t)
	sum := sha256.Sum256(raw)
	if got := hex.EncodeToString(sum[:]); got != writeOperationsSHA256 {
		t.Fatalf("golden sha256 is %s, pinned %s: a change to the golden is a reviewed change, so re-pin writeOperationsSHA256 and copy the file to %s", got, writeOperationsSHA256, writeOperationsVendored)
	}
}

func TestWriteOperationsVendoredCopyIsByteIdentical(t *testing.T) {
	if *updateWriteOperations {
		t.Skip("golden is being regenerated; copy it over the vendored file after the update")
	}
	golden, err := os.ReadFile(writeOperationsGolden)
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	vendored, err := os.ReadFile(writeOperationsVendored)
	if err != nil {
		t.Fatalf("read the Rust copy %s: %v", writeOperationsVendored, err)
	}
	if !bytes.Equal(golden, vendored) {
		t.Fatalf("%s differs from the golden; copy the golden over it", writeOperationsVendored)
	}
}

func TestWriteOperationsMatchExecutorAllowlistAndCatalogue(t *testing.T) {
	_, golden := loadWriteGolden(t)
	arms := writeArms(t)

	if *updateWriteOperations {
		regenerated := regenerateWriteGolden(golden, arms)
		raw, err := json.MarshalIndent(regenerated, "", "  ")
		if err != nil {
			t.Fatalf("encode golden: %v", err)
		}
		raw = append(raw, '\n')
		if err := os.WriteFile(writeOperationsGolden, raw, 0o644); err != nil {
			t.Fatalf("write golden: %v", err)
		}
		sum := sha256.Sum256(raw)
		t.Logf("golden regenerated; new sha256 %s", hex.EncodeToString(sum[:]))
		return
	}

	writes := map[string]writeArm{}
	for _, arm := range arms {
		if arm.gatedAny() && !arm.gatedAll() {
			t.Errorf("%s %q: actionRequiresApproval names some spellings of the operation but not all of %v", arm.provider, arm.names[0], arm.names)
		}
		if arm.isWrite() && !arm.gatedAll() {
			t.Errorf("%s %q sends a request body but actionRequiresApproval does not name it: an ungated write skips the approval gate (register D2-718)", arm.provider, arm.names[0])
		}
		if readsOverPost[arm.key()] && arm.gatedAny() {
			t.Errorf("%s %q is a read posted as JSON and must not be named by actionRequiresApproval", arm.provider, arm.names[0])
		}
		if arm.isWrite() {
			writes[arm.key()] = arm
		}
	}

	listed := map[string]string{}
	checkRow := func(list, provider, operation string, aliases []string) {
		key := provider + "/" + operation
		if previous, ok := listed[key]; ok {
			t.Errorf("%s %q is listed twice, also under %s", provider, operation, previous)
			return
		}
		listed[key] = list
		arm, ok := writes[key]
		if !ok {
			t.Errorf("%s lists %s %q, but the executor serves no write arm for it", list, provider, operation)
			return
		}
		if !arm.gatedAll() {
			t.Errorf("%s lists %s %q, but actionRequiresApproval does not name every spelling of it", list, provider, operation)
			return
		}
		if !slices.Equal(sortedNames(aliases), sortedNames(arm.names[1:])) {
			t.Errorf("%s %q: the golden lists aliases %v, the executor accepts %v", provider, operation, aliases, arm.names[1:])
		}
	}
	for _, row := range golden.Operations {
		checkRow("operations", row.Provider, row.Operation, row.Aliases)
		if !row.ApprovalRequired {
			t.Errorf("operations %q: approval_required must be true", row.Operation)
		}
		if row.Direction != "write" {
			t.Errorf("operations %q: direction is %q, want write", row.Operation, row.Direction)
		}
		if row.EditableKind != "mail_send" && row.EditableKind != "none" {
			t.Errorf("operations %q: editable_kind is %q, want mail_send or none", row.Operation, row.EditableKind)
		}
	}
	for _, entry := range golden.ExcludedWrites {
		checkRow("excludedWrites", entry.Provider, entry.Operation, entry.Aliases)
	}
	for _, key := range slices.Sorted(maps.Keys(writes)) {
		arm := writes[key]
		if _, ok := listed[key]; !ok {
			t.Errorf("the executor serves write %s %q %v, but the golden neither lists it as an operation nor excludes it: add a row", arm.provider, arm.names[0], arm.names)
		}
		for _, name := range arm.names {
			capability, ok := requiredCapabilityForOperation(arm.provider, name)
			if !ok {
				t.Errorf("%s %q has no capability in requiredCapabilityForOperation", arm.provider, name)
				continue
			}
			switch direction := catalogueDirection(arm.provider, capability); direction {
			case "write":
			case "":
				t.Errorf("%s %q needs capability %s, which the provider catalogue does not list", arm.provider, name, capability)
			default:
				t.Errorf("%s %q needs capability %s, which the provider catalogue marks %s, not write", arm.provider, name, capability, direction)
			}
		}
	}
}

// writeArms reads every provider executor's operation switch from service.go and classifies
// each case clause.
func writeArms(t *testing.T) []writeArm {
	t.Helper()
	file, err := parser.ParseFile(token.NewFileSet(), writeExecutorSource, nil, 0)
	if err != nil {
		t.Fatalf("parse %s: %v", writeExecutorSource, err)
	}
	var arms []writeArm
	for _, decl := range file.Decls {
		fn, ok := decl.(*ast.FuncDecl)
		if !ok || fn.Body == nil {
			continue
		}
		provider, ok := writeExecutorFunctions[fn.Name.Name]
		if !ok {
			continue
		}
		for _, clause := range operationClauses(t, fn) {
			var names []string
			for _, expr := range clause.List {
				name, ok := stringLiteral(expr)
				if !ok {
					t.Fatalf("%s: a case of %s is not a string literal", writeExecutorSource, fn.Name.Name)
				}
				names = append(names, name)
			}
			if len(names) == 0 {
				continue
			}
			arm := writeArm{provider: provider, names: names, mutates: callsMutatingHelper(clause.Body)}
			for _, name := range names {
				arm.gated = append(arm.gated, actionRequiresApproval(provider, name))
			}
			arms = append(arms, arm)
		}
	}
	if len(arms) == 0 {
		t.Fatalf("found no provider operation arms in %s", writeExecutorSource)
	}
	return arms
}

// operationClauses returns the case clauses of the function's first top-level switch, which is
// the operation switch every provider executor starts with.
func operationClauses(t *testing.T, fn *ast.FuncDecl) []*ast.CaseClause {
	t.Helper()
	for _, stmt := range fn.Body.List {
		sw, ok := stmt.(*ast.SwitchStmt)
		if !ok {
			continue
		}
		var clauses []*ast.CaseClause
		for _, item := range sw.Body.List {
			clause, ok := item.(*ast.CaseClause)
			if !ok {
				t.Fatalf("%s: the operation switch of %s has a non-case clause", writeExecutorSource, fn.Name.Name)
			}
			clauses = append(clauses, clause)
		}
		return clauses
	}
	t.Fatalf("%s has no top-level operation switch", fn.Name.Name)
	return nil
}

// callsMutatingHelper reports whether the clause body calls a helper that sends a request body,
// or names an HTTP method that does.
func callsMutatingHelper(body []ast.Stmt) bool {
	found := false
	for _, stmt := range body {
		ast.Inspect(stmt, func(n ast.Node) bool {
			switch node := n.(type) {
			case *ast.CallExpr:
				if sel, ok := node.Fun.(*ast.SelectorExpr); ok && mutatingHelpers[sel.Sel.Name] {
					found = true
				}
			case *ast.SelectorExpr:
				if mutatingMethods[node.Sel.Name] {
					found = true
				}
			}
			return !found
		})
	}
	return found
}

// catalogueDirection returns the direction the provider catalogue gives a capability, or ""
// when the catalogue does not list it.
func catalogueDirection(provider, capability string) string {
	found, ok := providers.Find(provider)
	if !ok {
		return ""
	}
	for _, candidate := range found.Capabilities {
		if candidate.Key == capability {
			return candidate.Direction
		}
	}
	return ""
}

// regenerateWriteGolden rebuilds both lists from the executor. A write already listed as an
// operation keeps its editable kind; every other write becomes an excluded write, for a person to
// promote once execution-core catalogues it.
func regenerateWriteGolden(previous writeGolden, arms []writeArm) writeGolden {
	operations := map[string]writeRow{}
	for _, row := range previous.Operations {
		operations[row.Provider+"/"+row.Operation] = row
	}
	out := writeGolden{Version: previous.Version, Operations: []writeRow{}, ExcludedWrites: []writeEntry{}}
	for _, arm := range arms {
		if !arm.isWrite() {
			continue
		}
		aliases := slices.Clone(arm.names[1:])
		if aliases == nil {
			aliases = []string{}
		}
		if row, ok := operations[arm.key()]; ok {
			out.Operations = append(out.Operations, writeRow{
				Provider:         arm.provider,
				Operation:        arm.names[0],
				Aliases:          aliases,
				ApprovalRequired: true,
				Direction:        "write",
				EditableKind:     row.EditableKind,
			})
			continue
		}
		out.ExcludedWrites = append(out.ExcludedWrites, writeEntry{
			Provider:  arm.provider,
			Operation: arm.names[0],
			Aliases:   aliases,
		})
	}
	sort.SliceStable(out.Operations, func(i, j int) bool {
		if out.Operations[i].Provider != out.Operations[j].Provider {
			return out.Operations[i].Provider < out.Operations[j].Provider
		}
		return out.Operations[i].Operation < out.Operations[j].Operation
	})
	sort.SliceStable(out.ExcludedWrites, func(i, j int) bool {
		if out.ExcludedWrites[i].Provider != out.ExcludedWrites[j].Provider {
			return out.ExcludedWrites[i].Provider < out.ExcludedWrites[j].Provider
		}
		return out.ExcludedWrites[i].Operation < out.ExcludedWrites[j].Operation
	})
	return out
}

func sortedNames(names []string) []string {
	out := slices.Clone(names)
	slices.Sort(out)
	return out
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
