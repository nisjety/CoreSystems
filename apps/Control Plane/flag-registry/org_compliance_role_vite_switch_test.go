package flagregistry

import (
	"bytes"
	"os"
	"strings"
	"testing"
)

// P2-cp-03-rel1 (O-21): the SPA's twin of the compliance role switch, VITE_ORG_COMPLIANCE_ROLE,
// offers the compliance role in the member list and the invitation pickers only while it is the
// literal true. The Control Plane switch decides whether the server accepts that role, so the two
// must not drift apart. The frontend twin needs its own registry row that names the one file that
// gates the picker, a declaration in the verevonv3 compose frontend service with the same default,
// and a reader that compares the exact literal. Without them a deployment can turn the role on in
// the server and leave the picker off, or the reverse, and the picker offers a choice the server
// refuses.
func TestOrgComplianceRoleViteSwitchIsRegisteredAndDeclaredByTheFrontend(t *testing.T) {
	const (
		name    = "VITE_ORG_COMPLIANCE_ROLE"
		service = "frontend"
		reader  = "apps/Frontend Plane/verevonv3/src/features/settings/lib/org-roles.ts"
	)
	gate := "import.meta.env." + name + " === 'true'"

	f, ok := loadShipped(t).Flag(name)
	if !ok {
		t.Fatalf("%s gates the compliance role in the SPA pickers but has no registry entry", name)
	}
	if f.Owner != PlaneFrontend {
		t.Errorf("%s: owner %q, want %q", name, f.Owner, PlaneFrontend)
	}
	if f.Kind != KindSwitch || f.Source != SourceCode || f.Default != "false" || f.EffectiveStage() != StageDark {
		t.Errorf("%s: kind %q source %q default %q stage %q, want switch/code/false/dark", name, f.Kind, f.Source, f.Default, f.EffectiveStage())
	}
	if f.OnLiteral != "true" || f.OnLiteralAnyCase != "" {
		t.Errorf("%s: onLiteral %q and onLiteralAnyCase %q, want onLiteral true", name, f.OnLiteral, f.OnLiteralAnyCase)
	}
	if f.OrgOverridable || f.KillSwitch || f.DevOnly {
		t.Errorf("%s: must be neither orgOverridable, a kill switch nor devOnly", name)
	}
	if f.CodePath != reader {
		t.Errorf("%s: codePath %q, want %q", name, f.CodePath, reader)
	}
	if !strings.HasSuffix(f.Purpose, "Only the literal true turns it on.") {
		t.Errorf("%s: purpose must end with the strict-literal sentence: %q", name, f.Purpose)
	}
	wantDecl := Declaration{Service: service, Env: name, Default: "false"}
	if len(f.Declarations) != 1 || f.Declarations[0] != wantDecl {
		t.Errorf("%s: registry declarations %+v, want exactly %+v", name, f.Declarations, wantDecl)
	}

	file, err := os.Open(repoFile(verevonv3Compose))
	if err != nil {
		t.Fatalf("open compose file: %v", err)
	}
	decls, err := ScanCompose(file)
	file.Close()
	if err != nil {
		t.Fatalf("scan compose file %s: %v", verevonv3Compose, err)
	}
	var declared []ComposeDecl
	for _, d := range decls {
		if d.Host == name || d.Env == name {
			declared = append(declared, d)
		}
	}
	if len(declared) != 1 || declared[0].Service != service || declared[0].Default != "false" {
		t.Errorf("%s: compose declares %+v in %s, want exactly one %s entry with default false", name, declared, verevonv3Compose, service)
	}

	data, err := os.ReadFile(repoFile(reader))
	if err != nil {
		t.Fatalf("%s: read reader: %v", name, err)
	}
	if !bytes.Contains(data, []byte(gate)) {
		t.Errorf("%s: %s does not gate on the literal true (want %q)", name, reader, gate)
	}
}
