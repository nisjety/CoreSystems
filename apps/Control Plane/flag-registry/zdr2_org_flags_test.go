package flagregistry

import (
	"os"
	"regexp"
	"slices"
	"strings"
	"testing"
)

// W5-079-b: org-core's closed list of org-overridable flags
// (org-core/internal/org/org_flags.go, OrgOverridableFlags) and this registry
// must agree. org-core is its own Go module, so the list is read with a small
// regex instead of an import.
const orgFlagsFile = "apps/Control Plane/org-core/internal/org/org_flags.go"

type orgFlagEntry struct {
	name, kind, def string
}

func readOrgOverridableFlags(t *testing.T) []orgFlagEntry {
	t.Helper()
	data, err := os.ReadFile(repoFile(orgFlagsFile))
	if err != nil {
		t.Fatalf("read %s: %v", orgFlagsFile, err)
	}
	src := strings.ReplaceAll(string(data), "\r\n", "\n")
	block := regexp.MustCompile(`(?s)var OrgOverridableFlags = \[\]OrgFlagDef\{(.*?)\n\}`).FindStringSubmatch(src)
	if block == nil {
		t.Fatalf("%s: OrgOverridableFlags block not found", orgFlagsFile)
	}
	entry := regexp.MustCompile(`\{Name:\s*"([^"]+)",\s*Kind:\s*OrgFlagKind(\w+)[^}]*?Default:\s*"([^"]*)"`)
	var out []orgFlagEntry
	for _, m := range entry.FindAllStringSubmatch(block[1], -1) {
		out = append(out, orgFlagEntry{name: m[1], kind: strings.ToLower(m[2]), def: m[3]})
	}
	// Every `Name:` in the block must have been parsed, so a reformatted entry
	// cannot drop out of the check silently.
	if got, want := len(out), strings.Count(block[1], "Name:"); got != want || got == 0 {
		t.Fatalf("%s: parsed %d of %d OrgOverridableFlags entries", orgFlagsFile, got, want)
	}
	return out
}

func TestOrgOverridableFlagsAreRegisteredOrgOverridable(t *testing.T) {
	reg := loadShipped(t)
	for _, entry := range readOrgOverridableFlags(t) {
		f, ok := reg.Flag(entry.name)
		if !ok {
			t.Errorf("%s is in org-core OrgOverridableFlags but not in the registry", entry.name)
			continue
		}
		if !f.OrgOverridable {
			t.Errorf("%s is in org-core OrgOverridableFlags but the registry does not mark it orgOverridable:true", entry.name)
		}
		if string(f.Kind) != entry.kind {
			t.Errorf("%s: org-core kind %q, registry kind %q", entry.name, entry.kind, f.Kind)
		}
		if f.Default != entry.def {
			t.Errorf("%s: org-core default %q, registry default %q", entry.name, entry.def, f.Default)
		}
	}
}

func TestEveryRegisteredOrgOverridableFlagIsInOrgCore(t *testing.T) {
	reg := loadShipped(t)
	var inOrgCore []string
	for _, entry := range readOrgOverridableFlags(t) {
		inOrgCore = append(inOrgCore, entry.name)
	}
	for _, f := range reg.Flags() {
		if f.OrgOverridable && !slices.Contains(inOrgCore, f.Name) {
			t.Errorf("%s is orgOverridable in the registry but org-core cannot store a value for it", f.Name)
		}
	}
}

// ZDR-Z-32: the zdr2 switches are registered as dark, default-off code
// switches owned by the plane that reads them, and the one kill switch is
// ZDR_WAIVER_ENABLED.
func TestZdr2SwitchesAreRegistered(t *testing.T) {
	reg := loadShipped(t)
	want := []struct {
		name       string
		owner      Plane
		codePath   string
		killSwitch bool
	}{
		{"AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED", PlaneControl, "apps/Control Plane/auth-core/src/auth/interactive-retention-policy.ts", false},
		{"ZDR_WAIVER_ENABLED", PlaneControl, "apps/Control Plane/auth-core/src/auth/retention-decision.service.ts", true},
		{"ZDR_ACKNOWLEDGEMENT_ENABLED", PlaneIngestion, "apps/Ingestion Plane/integration-corev2/internal/config/config.go", false},
		{"INTEGRATION_ZDR_EFFECTIVE_POSTURE_ENABLED", PlaneIngestion, "apps/Ingestion Plane/integration-corev2/internal/config/config.go", false},
		{"GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED", PlaneFrontend, "apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs", false},
		{"ZDR_WAIVER_UI_ENABLED", PlaneFrontend, "apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs", false},
		{"VITE_ZDR_WAIVER_UI", PlaneFrontend, "apps/Frontend Plane/verevonv3/src/shared/api/connection-zdr-client.ts", false},
		{"CONVERSATION_ZDR_EFFECTIVE_POSTURE_ENABLED", PlaneApplication, "apps/Application Plane/conversation-core/conversation-core-go/internal/config/config.go", false},
	}
	for _, w := range want {
		f, ok := reg.Flag(w.name)
		if !ok {
			t.Errorf("%s is not registered", w.name)
			continue
		}
		if f.Kind != KindSwitch || f.Default != "false" || f.Source != SourceCode || f.EffectiveStage() != StageDark {
			t.Errorf("%s: kind %q default %q source %q stage %q, want switch/false/code/dark",
				w.name, f.Kind, f.Default, f.Source, f.EffectiveStage())
		}
		if f.Owner != w.owner {
			t.Errorf("%s: owner %q, want %q", w.name, f.Owner, w.owner)
		}
		if f.CodePath != w.codePath {
			t.Errorf("%s: codePath %q, want %q", w.name, f.CodePath, w.codePath)
		}
		if f.KillSwitch != w.killSwitch {
			t.Errorf("%s: killSwitch %v, want %v", w.name, f.KillSwitch, w.killSwitch)
		}
		if f.OrgOverridable {
			t.Errorf("%s: a ZDR switch must not be orgOverridable", w.name)
		}
		if !strings.HasSuffix(f.Purpose, "Only the literal true turns it on.") {
			t.Errorf("%s: purpose must end with the strict-literal sentence: %q", w.name, f.Purpose)
		}
		if n := len([]rune(f.Purpose)); n > 240 {
			t.Errorf("%s: purpose is %d runes, at most 240", w.name, n)
		}
		data, err := os.ReadFile(repoFile(f.CodePath))
		if err != nil {
			t.Errorf("%s: codePath unreadable: %v", w.name, err)
			continue
		}
		if !strings.Contains(string(data), w.name) {
			t.Errorf("%s: codePath %s does not mention the variable", w.name, f.CodePath)
		}
	}
	for _, name := range []string{"AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED", "ZDR_WAIVER_ENABLED", "ZDR_ACKNOWLEDGEMENT_ENABLED"} {
		for _, other := range reg.Unregistered() {
			if other == name {
				t.Errorf("%s is registered and must not stay in the unregistered list", name)
			}
		}
	}
}
