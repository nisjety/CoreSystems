package flagregistry

import (
	"bytes"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

const (
	shippedRegistryFile = "flag-registry.json"
	modelPlaneCompose   = "apps/Model Plane/deploy/docker-compose.yml"
	controlPlaneCompose = "apps/Control Plane/docker-compose.yml"
	verevonv3Compose    = "apps/Frontend Plane/verevonv3/docker-compose.yml"
)

// repoFile resolves a repository-relative path from this module, which lives at
// apps/Control Plane/flag-registry.
func repoFile(rel string) string {
	return filepath.Join("..", "..", "..", filepath.FromSlash(rel))
}

func loadShipped(t *testing.T) *Registry {
	t.Helper()
	f, err := os.Open(shippedRegistryFile)
	if err != nil {
		t.Fatalf("open %s: %v", shippedRegistryFile, err)
	}
	defer f.Close()
	reg, err := Load(f)
	if err != nil {
		t.Fatalf("%s does not validate: %v", shippedRegistryFile, err)
	}
	return reg
}

// scanRatchetedCompose scans every compose file the ratchet covers and returns the
// declarations keyed by host variable, so a flag set in either file is checked
// against the declarations of both.
func scanRatchetedCompose(t *testing.T) map[string][]ComposeDecl {
	t.Helper()
	byHost := make(map[string][]ComposeDecl)
	for _, rel := range []string{modelPlaneCompose, controlPlaneCompose, verevonv3Compose} {
		f, err := os.Open(repoFile(rel))
		if err != nil {
			t.Fatalf("open %s: %v", rel, err)
		}
		decls, err := ScanCompose(f)
		f.Close()
		if err != nil {
			t.Fatalf("scan %s: %v", rel, err)
		}
		if len(decls) == 0 {
			t.Fatalf("%s: no declarations found; the scanner or the compose path is wrong", rel)
		}
		for _, d := range decls {
			byHost[d.Host] = append(byHost[d.Host], d)
		}
	}
	return byHost
}

func declarationKeys(decls []Declaration) []string {
	keys := make([]string, 0, len(decls))
	for _, d := range decls {
		keys = append(keys, d.Service+" "+d.Env+" "+d.Default)
	}
	slices.Sort(keys)
	return keys
}

func composeKeys(decls []ComposeDecl) []string {
	keys := make([]string, 0, len(decls))
	for _, d := range decls {
		keys = append(keys, d.Service+" "+d.Env+" "+d.Default)
	}
	slices.Sort(keys)
	return keys
}

func TestShippedRegistryLoads(t *testing.T) {
	if n := len(loadShipped(t).Flags()); n == 0 {
		t.Fatal("the shipped registry has no flags")
	}
}

func TestShippedRegistryMatchesComposeFiles(t *testing.T) {
	reg := loadShipped(t)
	compose := scanRatchetedCompose(t)
	known := make(map[string]bool)
	for _, f := range reg.Flags() {
		known[f.Name] = true
		if want, got := declarationKeys(f.Declarations), composeKeys(compose[f.Name]); !slices.Equal(want, got) {
			t.Errorf("%s: registry declares %v, compose declares %v", f.Name, want, got)
		}
	}
	for _, name := range reg.Unregistered() {
		known[name] = true
		if len(compose[name]) == 0 {
			t.Errorf("%s is listed as unregistered, but compose no longer sets it; remove it", name)
		}
	}
	for _, host := range slices.Sorted(maps.Keys(compose)) {
		if IsConventionName(host) && !known[host] {
			t.Errorf("%s follows the flag naming convention and compose sets it, but has no registry entry; add it to flags or to unregistered", host)
		}
	}
}

// modelComposeFiles returns the Model Plane compose files the second ratchet reads: the base file, and every overlay in
// the same deploy directory, such as the override, production, release, replica and bridge files. A switch that only an
// overlay sets is ratcheted like one in the base file.
func modelComposeFiles(t *testing.T) []string {
	t.Helper()
	const deploy = "apps/Model Plane/deploy"
	matches, err := filepath.Glob(filepath.Join(repoFile(deploy), "docker-compose*.yml"))
	if err != nil {
		t.Fatalf("list the Model Plane compose files: %v", err)
	}
	files := []string{modelPlaneCompose}
	for _, p := range matches {
		if rel := deploy + "/" + filepath.Base(p); rel != modelPlaneCompose {
			files = append(files, rel)
		}
	}
	slices.Sort(files)
	return files
}

// modelSwitchShapedOutsideConvention returns the Model Plane compose variables outside the
// naming convention whose compose default is a boolean-looking value, keyed by variable name. It reads every file
// modelComposeFiles returns.
func modelSwitchShapedOutsideConvention(t *testing.T) map[string]bool {
	t.Helper()
	found := make(map[string]bool)
	for _, rel := range modelComposeFiles(t) {
		f, err := os.Open(repoFile(rel))
		if err != nil {
			t.Fatalf("open %s: %v", rel, err)
		}
		decls, err := ScanCompose(f)
		f.Close()
		if err != nil {
			t.Fatalf("scan %s: %v", rel, err)
		}
		switchShapedOutsideConvention(decls, found)
	}
	return found
}

// switchShapedOutsideConvention records in found each declared variable outside the naming convention whose compose
// default is a boolean-looking value.
func switchShapedOutsideConvention(decls []ComposeDecl, found map[string]bool) {
	for _, d := range decls {
		if !IsConventionName(d.Host) && isBooleanLooking(d.Default) {
			found[d.Host] = true
		}
	}
}

// TestModelComposeOverlaysAreReadByTheSecondRatchet pins the overlays the second ratchet reads, and the rule it applies to a
// switch that only an overlay sets: such a switch is found, and a blank default is not a switch.
func TestModelComposeOverlaysAreReadByTheSecondRatchet(t *testing.T) {
	files := modelComposeFiles(t)
	for _, want := range []string{
		modelPlaneCompose,
		"apps/Model Plane/deploy/docker-compose.override.yml",
		"apps/Model Plane/deploy/docker-compose.production.yml",
		"apps/Model Plane/deploy/docker-compose.release.yml",
		"apps/Model Plane/deploy/docker-compose.replicas.yml",
	} {
		if !slices.Contains(files, want) {
			t.Errorf("%s is not read by the second ratchet; it reads %v", want, files)
		}
	}
	const overlay = `services:
  model-gateway:
    environment:
      OVERLAY_ONLY_SWITCH: ${OVERLAY_ONLY_SWITCH:-false}
      OVERLAY_BLANK: ${OVERLAY_BLANK:-}
`
	decls, err := ScanCompose(strings.NewReader(overlay))
	if err != nil {
		t.Fatalf("scan the fixture: %v", err)
	}
	found := make(map[string]bool)
	switchShapedOutsideConvention(decls, found)
	if !found["OVERLAY_ONLY_SWITCH"] || found["OVERLAY_BLANK"] {
		t.Errorf("the fixture's switch-shaped names are %v, want OVERLAY_ONLY_SWITCH and not OVERLAY_BLANK", found)
	}
}

// TestModelComposeSwitchesOutsideTheConventionAreRegisteredOrListed is the second compose ratchet.
// The convention ratchet reads only names that follow the naming convention, so a switch such as
// ORG_EXTENSION_ALLOWLIST could be set in the Model Plane compose with no entry and nothing would
// notice. This one reads every Model Plane compose variable outside the convention whose default is
// a boolean-looking value, and requires each to be registered or listed in unregisteredSwitches.
func TestModelComposeSwitchesOutsideTheConventionAreRegisteredOrListed(t *testing.T) {
	reg := loadShipped(t)
	switches := modelSwitchShapedOutsideConvention(t)
	listed := reg.UnregisteredSwitches()
	for _, name := range slices.Sorted(maps.Keys(switches)) {
		if _, ok := reg.Flag(name); ok {
			continue
		}
		if !slices.Contains(listed, name) {
			t.Errorf("%s is set by the Model Plane compose outside the naming convention with a switch-shaped default, but has no registry entry; add it to flags or to unregisteredSwitches", name)
		}
	}
	for _, name := range listed {
		if !switches[name] {
			t.Errorf("%s is listed in unregisteredSwitches, but the Model Plane compose no longer sets it to a switch value; remove it", name)
		}
	}
}

// TestModelComposeSwitchRatchetReadsTheNamesThatEscapedIt pins the scan on names the convention
// ratchet could not see: a switch outside the convention is read whether it is registered or
// listed, so removing its entry would make the ratchet fail rather than pass silently.
func TestModelComposeSwitchRatchetReadsTheNamesThatEscapedIt(t *testing.T) {
	switches := modelSwitchShapedOutsideConvention(t)
	for _, name := range []string{"HOOKS_DURABLE", "MCP_DO_TOOLS", "ORG_AGENT_POLICY_API", "ORG_EXTENSION_ALLOWLIST", "SPACE_WAKE_API", "WORKSPACE_TOOLS"} {
		if !switches[name] {
			t.Errorf("%s is set by the Model Plane compose with a switch-shaped default, but the ratchet does not read it", name)
		}
	}
}

func TestShippedCodeFlagsAreStillRead(t *testing.T) {
	for _, f := range loadShipped(t).Flags() {
		if f.CodePath == "" {
			continue
		}
		data, err := os.ReadFile(repoFile(f.CodePath))
		if err != nil {
			t.Errorf("%s: codePath: %v", f.Name, err)
			continue
		}
		if !bytes.Contains(data, []byte(f.Name)) {
			t.Errorf("%s: %s no longer mentions the variable", f.Name, f.CodePath)
		}
	}
}

// TestApprovalScopeGrantsCodePathNamesItsReader pins the codePath of
// VITE_APPROVAL_SCOPE_GRANTS to the file that reads the switch. ChatPanels.tsx
// gates the grants card on the literal "true"; ThreadApprovalGrants.tsx only
// draws the card and never names the variable, so a codePath there leaves
// TestShippedCodeFlagsAreStillRead red.
func TestApprovalScopeGrantsCodePathNamesItsReader(t *testing.T) {
	const (
		name   = "VITE_APPROVAL_SCOPE_GRANTS"
		reader = "apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx"
		gate   = "import.meta.env." + name + " === 'true'"
	)
	f, ok := loadShipped(t).Flag(name)
	if !ok {
		t.Fatalf("%s is not registered", name)
	}
	if f.CodePath != reader {
		t.Errorf("%s: codePath is %s, want %s", name, f.CodePath, reader)
	}
	data, err := os.ReadFile(repoFile(reader))
	if err != nil {
		t.Fatalf("%s: read reader: %v", name, err)
	}
	if !bytes.Contains(data, []byte(gate)) {
		t.Errorf("%s: %s does not gate on the literal true (want %q)", name, reader, gate)
	}
}
