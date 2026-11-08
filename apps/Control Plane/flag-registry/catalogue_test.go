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

func scanModelPlaneCompose(t *testing.T) map[string][]ComposeDecl {
	t.Helper()
	f, err := os.Open(repoFile(modelPlaneCompose))
	if err != nil {
		t.Fatalf("open compose file: %v", err)
	}
	defer f.Close()
	decls, err := ScanCompose(f)
	if err != nil {
		t.Fatalf("scan compose file: %v", err)
	}
	if len(decls) == 0 {
		t.Fatal("no declarations found; the scanner or the compose path is wrong")
	}
	byHost := make(map[string][]ComposeDecl)
	for _, d := range decls {
		byHost[d.Host] = append(byHost[d.Host], d)
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

func TestShippedRegistryMatchesModelPlaneCompose(t *testing.T) {
	reg := loadShipped(t)
	compose := scanModelPlaneCompose(t)
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
