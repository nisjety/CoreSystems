package flagregistry

import (
	"os"
	"strings"
	"testing"
)

// Wave 3 EP-2 S9: execution-core enforces the wide-research child token budget only when
// WIDE_RESEARCH_CHILD_BUDGET_ENFORCED is the literal true. Its name does not end in _ENABLED,
// so the compose ratchet's naming convention never flags it on its own. Nothing else requires
// its registry entry or its execution-core compose declaration, so this test does.
func TestWideResearchChildBudgetSwitchIsRegisteredAndDeclared(t *testing.T) {
	const name = "WIDE_RESEARCH_CHILD_BUDGET_ENFORCED"

	reg := loadShipped(t)
	f, ok := reg.Flag(name)
	if !ok {
		t.Errorf("%s gates the wide-research child token budget but has no registry entry", name)
	} else {
		if f.Owner != PlaneModel {
			t.Errorf("%s: owner %q, want %q", name, f.Owner, PlaneModel)
		}
		if f.Kind != KindSwitch || f.Default != "false" || f.EffectiveStage() != StageDark {
			t.Errorf("%s: kind %q default %q stage %q, want switch/false/dark", name, f.Kind, f.Default, f.EffectiveStage())
		}
		if !strings.Contains(f.Purpose, "Only the literal true enforces it") {
			t.Errorf("%s: purpose must say that only the literal true enforces it: %q", name, f.Purpose)
		}
	}

	file, err := os.Open(repoFile(modelPlaneCompose))
	if err != nil {
		t.Fatalf("open compose file: %v", err)
	}
	defer file.Close()
	decls, err := ScanCompose(file)
	if err != nil {
		t.Fatalf("scan compose file: %v", err)
	}
	for _, d := range decls {
		if d.Host == name || d.Env == name {
			if d.Service != "execution-core" {
				t.Errorf("%s is declared for service %q, want execution-core", name, d.Service)
			}
			if d.Default != "false" {
				t.Errorf("%s is declared with default %q, want false", name, d.Default)
			}
			return
		}
	}
	t.Errorf("%s is not declared in the Model Plane compose (%s); a deployment cannot turn it on", name, modelPlaneCompose)
}
