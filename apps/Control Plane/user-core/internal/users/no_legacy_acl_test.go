package users_test

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestNoCodeReadsLegacyDocumentAcl asserts that resource_grants is the single
// authority: no Go source in user-core reads or writes the dropped legacy
// `document_acl` table. Migrations (which backfill from then DROP the legacy
// table) and generated protobuf stubs are exempt; this test file is exempt
// because it necessarily names the table.
func TestNoCodeReadsLegacyDocumentAcl(t *testing.T) {
	root := moduleRoot(t)

	const legacy = "document_acl"
	var offenders []string

	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			switch d.Name() {
			case "migrations", "tmp", "vendor", ".git", "proto":
				return filepath.SkipDir
			}
			return nil
		}
		name := d.Name()
		if !strings.HasSuffix(name, ".go") {
			return nil
		}
		// Exempt: this guard test (names the table) and generated stubs.
		if name == "no_legacy_acl_test.go" || strings.HasSuffix(name, ".pb.go") {
			return nil
		}
		content, readErr := os.ReadFile(path)
		if readErr != nil {
			return readErr
		}
		if strings.Contains(string(content), legacy) {
			rel, _ := filepath.Rel(root, path)
			offenders = append(offenders, rel)
		}
		return nil
	})
	if err != nil {
		t.Fatalf("walk failed: %v", err)
	}

	if len(offenders) > 0 {
		t.Fatalf("legacy %q table still referenced by Go source (must use resource_grants): %s",
			legacy, strings.Join(offenders, ", "))
	}
}

// moduleRoot walks up from the test's working directory to the directory holding go.mod.
func moduleRoot(t *testing.T) string {
	t.Helper()
	dir, err := os.Getwd()
	if err != nil {
		t.Fatalf("getwd: %v", err)
	}
	for {
		if _, statErr := os.Stat(filepath.Join(dir, "go.mod")); statErr == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			t.Fatal("could not locate go.mod (module root)")
		}
		dir = parent
	}
}
