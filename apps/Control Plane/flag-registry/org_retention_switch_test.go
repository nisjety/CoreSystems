package flagregistry

import (
	"os"
	"strings"
	"testing"
)

// EP-9 C1: org-core's retention policy and legal-hold routes sit behind
// ORG_RETENTION_POLICY_ENABLED. Like every switch of this phase it must be registered (a
// dark, default-off Control Plane switch read by the code that mounts the routes) and
// declared in org-core's compose block, or a deployment cannot turn it on and the
// registry ratchet never sees it.
func TestOrgRetentionPolicySwitchIsRegisteredAndDeclared(t *testing.T) {
	const name = "ORG_RETENTION_POLICY_ENABLED"
	const codePath = "apps/Control Plane/org-core/internal/retention/types.go"

	reg := loadShipped(t)
	f, ok := reg.Flag(name)
	if !ok {
		t.Errorf("%s gates org-core's retention and legal-hold routes (%s) but has no registry entry", name, codePath)
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
		if !strings.HasSuffix(f.Purpose, "Only the literal true turns it on.") {
			t.Errorf("%s: purpose must end with the strict-literal sentence: %q", name, f.Purpose)
		}
	}

	file, err := os.Open(repoFile(controlPlaneCompose))
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
			if d.Service != "org-core" {
				t.Errorf("%s is declared for service %q, want org-core", name, d.Service)
			}
			if d.Default != "false" {
				t.Errorf("%s is declared with default %q, want false", name, d.Default)
			}
			return
		}
	}
	t.Errorf("%s is not declared in the Control Plane compose (%s); a deployment cannot turn the routes on", name, controlPlaneCompose)
}
