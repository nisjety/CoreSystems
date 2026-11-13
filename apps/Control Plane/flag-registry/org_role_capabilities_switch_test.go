package flagregistry

import (
	"os"
	"testing"
)

// The member capability route sits behind ORG_ROLE_CAPABILITIES_ENABLED. Like every switch of this
// phase it must be registered (a dark, default-off Control Plane switch read by the handler that
// gates the route) and declared in org-core's compose block, or a deployment cannot turn the route
// on and the registry ratchet never sees it.
func TestOrgRoleCapabilitiesSwitchIsRegisteredAndDeclared(t *testing.T) {
	const name = "ORG_ROLE_CAPABILITIES_ENABLED"
	const codePath = "apps/Control Plane/org-core/internal/http/member_effective_capabilities_handler.go"

	reg := loadShipped(t)
	f, ok := reg.Flag(name)
	if !ok {
		t.Errorf("%s gates org-core's member capability route (%s) but has no registry entry", name, codePath)
	} else {
		if f.Owner != PlaneControl {
			t.Errorf("%s: owner %q, want %q", name, f.Owner, PlaneControl)
		}
		if f.Kind != KindSwitch || f.Default != "false" || f.EffectiveStage() != StageDark {
			t.Errorf("%s: kind %q default %q stage %q, want switch/false/dark", name, f.Kind, f.Default, f.EffectiveStage())
		}
		if f.OrgOverridable || f.KillSwitch {
			t.Errorf("%s: must be neither orgOverridable nor a kill switch", name)
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
			if d.Service != "org-core" {
				t.Errorf("%s is declared for service %q, want org-core", name, d.Service)
			}
			return
		}
	}
	t.Errorf("%s is not declared in the Control Plane compose (%s); a deployment cannot turn the route on", name, controlPlaneCompose)
}
