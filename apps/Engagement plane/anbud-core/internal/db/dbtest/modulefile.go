package dbtest

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// ModuleFilePath is where a file of the module is (rel is a slash-separated path from the
// module root, such as docs/PERSONAL-DATA.md), found from this source file or, when the
// build has no source paths, by walking up from the working directory to the module root.
// It is empty when the file cannot be found.
func ModuleFilePath(rel string) string {
	if _, file, _, ok := runtime.Caller(0); ok {
		p := filepath.Join(filepath.Dir(file), "..", "..", "..", filepath.FromSlash(rel))
		if _, err := os.Stat(p); err == nil {
			return p
		}
	}
	dir, err := os.Getwd()
	if err != nil {
		return ""
	}
	for range 8 {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			p := filepath.Join(dir, filepath.FromSlash(rel))
			if _, err := os.Stat(p); err == nil {
				return p
			}
			return ""
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	return ""
}

// ReadModuleFile returns the text of a file of the module and fails t when it cannot be
// found or read. It is for the module's own documents and scripts, never for a file that
// holds a credential.
func ReadModuleFile(t testing.TB, rel string) string {
	t.Helper()
	path := ModuleFilePath(rel)
	if path == "" {
		t.Fatalf("dbtest: %s was not found from the module root", rel)
	}
	text, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("dbtest: %s could not be read", rel)
	}
	return string(text)
}
