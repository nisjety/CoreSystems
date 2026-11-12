package flagregistry

import (
	"bytes"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

const (
	shippedRegistryFile = "flag-registry.json"
	modelPlaneCompose   = "apps/Model Plane/deploy/docker-compose.yml"
	controlPlaneCompose = "apps/Control Plane/docker-compose.yml"
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
	for _, rel := range []string{modelPlaneCompose, controlPlaneCompose} {
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
