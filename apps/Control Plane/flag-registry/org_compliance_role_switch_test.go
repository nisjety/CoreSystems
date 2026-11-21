package flagregistry

import (
	"os"
	"strings"
	"testing"
)

// O-21: the compliance organisation role is accepted by org-core, user-core and
// auth-core, read for audit by audit-core and placed against holds through the
// verevonv3 gateway only while ORG_COMPLIANCE_ROLE_ENABLED is the literal true. The
// switch must be registered (a dark, default-off Control Plane switch) and declared
// in the compose block of every service that reads it, or a deployment turns it on
// in one service and not the others and the role is refused (or, worse, half
// granted) somewhere along the path. The five readers: org-core (role validation
// and hold authority), user-core (membership resolve), auth-core (canonical roles,
// the agent-run refusal), audit-core (audit and compliance reads) and the
// verevonv3 gateway (the BFF's own hold and audit role gates).
func TestOrgComplianceRoleSwitchIsRegisteredAndDeclaredInEveryReadingService(t *testing.T) {
	const name = "ORG_COMPLIANCE_ROLE_ENABLED"
	const codePath = "apps/Control Plane/org-core/internal/org/compliance_role.go"

	reg := loadShipped(t)
	f, ok := reg.Flag(name)
	if !ok {
		t.Errorf("%s gates the compliance role in org-core, user-core and auth-core but has no registry entry", name)
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

	controlServices := []string{"org-core", "user-core", "auth-core", "audit-core"}
	const gatewayService = "gateway"

	if ok {
		got := map[string]bool{}
		for _, d := range f.Declarations {
			got[d.Service] = true
			if d.Env != name || d.Default != "false" {
				t.Errorf("%s: registry declaration %+v, want env %s default false", name, d, name)
			}
		}
		for _, service := range append(append([]string{}, controlServices...), gatewayService) {
			if !got[service] {
				t.Errorf("%s: the registry row has no declaration for %s", name, service)
			}
		}
		if len(f.Declarations) != len(controlServices)+1 {
			t.Errorf("%s: %d registry declarations, want %d", name, len(f.Declarations), len(controlServices)+1)
		}
	}

	for _, check := range []struct {
		compose  string
		services []string
	}{
		{controlPlaneCompose, controlServices},
		{verevonv3Compose, []string{gatewayService}},
	} {
		file, err := os.Open(repoFile(check.compose))
		if err != nil {
			t.Fatalf("open compose file: %v", err)
		}
		decls, err := ScanCompose(file)
		file.Close()
		if err != nil {
			t.Fatalf("scan compose file %s: %v", check.compose, err)
		}
		declared := map[string]string{}
		for _, d := range decls {
			if d.Host == name || d.Env == name {
				declared[d.Service] = d.Default
			}
		}
		for _, service := range check.services {
			def, found := declared[service]
			if !found {
				t.Errorf("%s is not declared in the compose block of %s (%s)", name, service, check.compose)
				continue
			}
			if def != "false" {
				t.Errorf("%s is declared for %s with default %q, want false", name, service, def)
			}
		}
		for service := range declared {
			listed := false
			for _, want := range check.services {
				if service == want {
					listed = true
				}
			}
			if !listed {
				t.Errorf("%s is declared for %s in %s, a service this test does not know reads it", name, service, check.compose)
			}
		}
	}
}
