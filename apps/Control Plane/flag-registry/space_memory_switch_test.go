package flagregistry

import (
	"os"
	"testing"
)

// EP-8 S7 (W3-EP-8-S7-a): user-core's Space memory decision route sits behind
// CONTROL_SPACE_MEMORY_DECISIONS, a switch added in this phase. Like every switch of
// this phase it must be registered (dark, default off, Control Plane owned, read in
// user-core's main) and declared in user-core's compose block. Its name does not end
// in _ENABLED, so the compose ratchet's naming convention never flags it on its own.
func TestSpaceMemoryDecisionsSwitchIsRegisteredAndDeclared(t *testing.T) {
	const name = "CONTROL_SPACE_MEMORY_DECISIONS"
	const codePath = "apps/Control Plane/user-core/cmd/server/main.go"

	reg := loadShipped(t)
	f, ok := reg.Flag(name)
	if !ok {
		t.Errorf("%s gates user-core's memory-decision route but has no registry entry", name)
	} else {
		if f.Owner != PlaneControl {
			t.Errorf("%s: owner %q, want %q", name, f.Owner, PlaneControl)
		}
		if f.Kind != KindSwitch || f.Default != "false" || f.EffectiveStage() != StageDark {
			t.Errorf("%s: kind %q default %q stage %q, want switch/false/dark", name, f.Kind, f.Default, f.EffectiveStage())
		}
		if f.OrgOverridable {
			t.Errorf("%s: must not be orgOverridable (a rollout control, never an authorization input)", name)
		}
		if f.CodePath != codePath {
			t.Errorf("%s: codePath %q, want %q", name, f.CodePath, codePath)
		}
	}

	data, err := os.Open(repoFile(controlPlaneCompose))
	if err != nil {
		t.Fatalf("open compose file: %v", err)
	}
	defer data.Close()
	decls, err := ScanCompose(data)
	if err != nil {
		t.Fatalf("scan compose file: %v", err)
	}
	for _, d := range decls {
		if d.Host == name || d.Env == name {
			if d.Service != "user-core" {
				t.Errorf("%s is declared for service %q, want user-core", name, d.Service)
			}
			return
		}
	}
	t.Errorf("%s is not declared in the Control Plane compose (%s)", name, controlPlaneCompose)
}
