package dbtest

import (
	"path/filepath"
	"strings"
	"testing"
)

// ModuleFilePath finds the module's own files from the package directory the tests run in.
func TestModuleFilePathFindsTheModuleRoot(t *testing.T) {
	t.Parallel()

	mod := ModuleFilePath("go.mod")
	if mod == "" {
		t.Fatal("ModuleFilePath did not find go.mod")
	}
	if filepath.Base(mod) != "go.mod" {
		t.Errorf("ModuleFilePath(go.mod) = %q", mod)
	}
	if got := ModuleFilePath("scripts/roles.sql"); got == "" || filepath.Dir(got) != filepath.Join(filepath.Dir(mod), "scripts") {
		t.Errorf("ModuleFilePath(scripts/roles.sql) = %q, want it beside go.mod under scripts", got)
	}
	if got := ModuleFilePath("no/such/file.txt"); got != "" {
		t.Errorf("ModuleFilePath of a missing file = %q, want empty", got)
	}
	if text := ReadModuleFile(t, "scripts/roles.sql"); !strings.Contains(text, "anbud_dba") {
		t.Error("ReadModuleFile(scripts/roles.sql) did not return the script")
	}
}
