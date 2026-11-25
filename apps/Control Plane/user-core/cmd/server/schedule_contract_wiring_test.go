package main

import (
	"go/ast"
	"go/parser"
	"go/token"
	"strings"
	"testing"
)

// parseMainBody returns the body of func main in main.go. The boot-check pins walk only this
// body, so a call inside a helper that main never runs cannot satisfy them.
func parseMainBody(t *testing.T) (*token.FileSet, *ast.BlockStmt) {
	t.Helper()
	fset := token.NewFileSet()
	file, err := parser.ParseFile(fset, "main.go", nil, parser.SkipObjectResolution)
	if err != nil {
		t.Fatalf("parse main.go: %v", err)
	}
	for _, decl := range file.Decls {
		fn, ok := decl.(*ast.FuncDecl)
		if ok && fn.Recv == nil && fn.Name.Name == "main" && fn.Body != nil {
			return fset, fn.Body
		}
	}
	t.Fatal("main.go has no func main")
	return nil, nil
}

// F-f10-27 rel1: main.go must run the stored-consent boot check exactly once. The schedule switch
// (SetScheduleContractsEnabled) and every Set*AuthorityResolver must be wired before the check.
// The check reads the switch as off until it is set, so a check placed above the switch sees it
// off and returns nil even when the switch is turned on afterwards. A resolver wired after the
// check is one the check never saw. Either way SCHEDULE_CONTRACT_ENABLED=true could start with a
// resolver missing, and every run-execution decision would answer 503.
func TestScheduleContractBootCheckFollowsEveryAuthorityWiring(t *testing.T) {
	fset, body := parseMainBody(t)
	var check token.Pos
	var checkCalls int
	var switchSet token.Pos
	var wiring []token.Pos
	ast.Inspect(body, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		sel, ok := call.Fun.(*ast.SelectorExpr)
		if !ok {
			return true
		}
		name := sel.Sel.Name
		switch {
		case name == "ValidateScheduleContractWiring":
			check = call.Pos()
			checkCalls++
		case name == "SetScheduleContractsEnabled":
			switchSet = call.Pos()
		case strings.HasPrefix(name, "Set") && strings.HasSuffix(name, "AuthorityResolver"):
			wiring = append(wiring, call.Pos())
		}
		return true
	})
	if !check.IsValid() {
		t.Fatal("func main never calls ValidateScheduleContractWiring")
	}
	if checkCalls > 1 {
		t.Fatalf("func main calls ValidateScheduleContractWiring %d times; the boot check must run once", checkCalls)
	}
	for _, pos := range wiring {
		if pos > check {
			t.Fatalf("an authority resolver is wired after the schedule contract boot check: %s", fset.Position(pos))
		}
	}
	if !switchSet.IsValid() {
		t.Fatal("func main never calls SetScheduleContractsEnabled")
	}
	if switchSet >= check {
		t.Fatalf("the schedule switch is wired after the schedule contract boot check, so the check would see it off: %s", fset.Position(switchSet))
	}
}

// F-f10-27 rel1: the boot check's error must end the start. The check must be the Init of an
// if-statement whose body calls log.Fatal, log.Fatalf, log.Fatalln or os.Exit. A logged-only
// error (log.Printf) or a discarded result (_ = ...) would let SCHEDULE_CONTRACT_ENABLED=true
// start without a prepared-run resolver, and every run-execution decision would answer 503.
func TestScheduleContractBootCheckRefusesTheStartWhenItFails(t *testing.T) {
	_, body := parseMainBody(t)
	refused := false
	ast.Inspect(body, func(n ast.Node) bool {
		ifStmt, ok := n.(*ast.IfStmt)
		if !ok {
			return true
		}
		assign, ok := ifStmt.Init.(*ast.AssignStmt)
		if !ok || len(assign.Rhs) != 1 {
			return true
		}
		call, ok := assign.Rhs[0].(*ast.CallExpr)
		if !ok {
			return true
		}
		sel, ok := call.Fun.(*ast.SelectorExpr)
		if !ok || sel.Sel.Name != "ValidateScheduleContractWiring" {
			return true
		}
		ast.Inspect(ifStmt.Body, func(m ast.Node) bool {
			exit, ok := m.(*ast.CallExpr)
			if !ok {
				return true
			}
			esel, ok := exit.Fun.(*ast.SelectorExpr)
			if !ok {
				return true
			}
			pkg, ok := esel.X.(*ast.Ident)
			if !ok {
				return true
			}
			switch {
			case pkg.Name == "log" && (esel.Sel.Name == "Fatal" || esel.Sel.Name == "Fatalf" || esel.Sel.Name == "Fatalln"):
				refused = true
			case pkg.Name == "os" && esel.Sel.Name == "Exit":
				refused = true
			}
			return true
		})
		return true
	})
	if !refused {
		t.Fatal("func main calls ValidateScheduleContractWiring but its error does not end the start (the if-block must log.Fatal or os.Exit)")
	}
}
