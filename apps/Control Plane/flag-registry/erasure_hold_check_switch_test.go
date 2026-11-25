package flagregistry

import (
	"os"
	"slices"
	"strings"
	"testing"
)

// F-f10-06 (P2-cp-08 rel1): ERASURE_HOLD_CHECK gates two erasure paths, and both read only the literal true.
// org-core reads it in its 30-day organisation purge and in its erasure routes (internal/org and
// internal/http). session-core reads it before a thread or organisation erasure (session-core src/gdpr.rs, in
// apps/Model Plane/rust/services/session-core). Both services read it, but neither compose file declared it and
// the registry had no row, so an operator could not turn the check on through the declared surfaces. This pins
// the registry row and one compose declaration per reader, each defaulting to false, so a deployment that sets
// nothing keeps its behaviour.
func TestErasureHoldCheckIsRegisteredAndDeclaredByBothReaders(t *testing.T) {
	const name = "ERASURE_HOLD_CHECK"
	const codePath = "apps/Control Plane/org-core/internal/http/gdpr_handlers.go"

	reg := loadShipped(t)
	f, ok := reg.Flag(name)
	if !ok {
		t.Fatalf("%s gates org-core's purge and erasure refusals and session-core's erasure gate but has no registry entry", name)
	}
	if f.Owner != PlaneControl {
		t.Errorf("%s: owner %q, want %q", name, f.Owner, PlaneControl)
	}
	if f.Kind != KindSwitch || f.Default != "false" || f.EffectiveStage() != StageDark {
		t.Errorf("%s: kind %q default %q stage %q, want switch/false/dark", name, f.Kind, f.Default, f.EffectiveStage())
	}
	if f.OnLiteral != "true" || f.OnLiteralAnyCase != "" {
		t.Errorf("%s: onLiteral %q and onLiteralAnyCase %q, want the strict literal true only", name, f.OnLiteral, f.OnLiteralAnyCase)
	}
	if f.OrgOverridable || f.KillSwitch {
		t.Errorf("%s: must be neither orgOverridable nor a kill switch", name)
	}
	if f.CodePath != codePath {
		t.Errorf("%s: codePath %q, want %q", name, f.CodePath, codePath)
	}
	if !strings.HasSuffix(f.Purpose, "Only the literal true turns it on.") {
		t.Errorf("%s: purpose must end with the strict-literal sentence: %q", name, f.Purpose)
	}
	for _, want := range []Declaration{
		{Service: "org-core", Env: name, Default: "false"},
		{Service: "session-core", Env: name, Default: "false"},
	} {
		if !slices.Contains(f.Declarations, want) {
			t.Errorf("%s: registry declarations %+v do not include %+v", name, f.Declarations, want)
		}
	}

	// Each reader's service sets the variable in the compose file that runs it, defaulting to false.
	for _, c := range []struct{ rel, service string }{
		{controlPlaneCompose, "org-core"},
		{modelPlaneCompose, "session-core"},
	} {
		found := false
		for _, d := range scanComposeDecls(t, c.rel) {
			if d.Host != name {
				continue
			}
			found = true
			if d.Service != c.service || d.Default != "false" {
				t.Errorf("%s is declared for %q with default %q in %s, want %q with default false", name, d.Service, d.Default, c.rel, c.service)
			}
		}
		if !found {
			t.Errorf("%s is not declared in %s for %s; a deployment cannot turn the check on", name, c.rel, c.service)
		}
	}
}

// scanComposeDecls reads one compose file with the scanner the ratchet uses.
func scanComposeDecls(t *testing.T, rel string) []ComposeDecl {
	t.Helper()
	file, err := os.Open(repoFile(rel))
	if err != nil {
		t.Fatalf("open %s: %v", rel, err)
	}
	defer file.Close()
	decls, err := ScanCompose(file)
	if err != nil {
		t.Fatalf("scan %s: %v", rel, err)
	}
	return decls
}
