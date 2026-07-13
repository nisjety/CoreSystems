package users

import (
	"os"
	"strings"
	"testing"
)

func TestGetByEmailUsesCanonicalCaseInsensitiveLookup(t *testing.T) {
	contents, err := os.ReadFile("repository.go")
	if err != nil {
		t.Fatalf("read repository: %v", err)
	}
	source := string(contents)
	start := strings.Index(source, "func (r *Repository) GetByEmail")
	if start < 0 {
		t.Fatal("GetByEmail not found")
	}
	end := strings.Index(source[start:], "\nfunc ")
	if end < 0 {
		end = len(source) - start
	}
	function := source[start : start+end]
	if !strings.Contains(function, "LOWER(BTRIM(email)) = LOWER(BTRIM($1))") {
		t.Fatal("GetByEmail must normalize case and surrounding whitespace in SQL")
	}
}
