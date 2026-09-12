package snapshot

import (
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/redact"
)

// The pattern-level redaction tests that used to live here moved to
// internal/redact's own test file along with the patterns themselves (S4.2
// step 1). What stays is what this package still owns: which files are
// dropped outright, and that the surviving ones actually go through the
// redactor.

func TestExcludeCredentialsDropsScratchOnlyPathsEntirely(t *testing.T) {
	// Content here need not look secret-shaped at all: this test is about
	// whole-file path exclusion, not redaction.
	files := map[string][]byte{
		"scratch/notes.txt": []byte("scratch working notes"),
		"/tmp/cache.bin":    []byte("temporary cache contents"),
		"workspace/app.log": []byte("status=completed"),
	}
	out := ExcludeCredentials(files)
	if _, ok := out["scratch/notes.txt"]; ok {
		t.Fatal("scratch/ prefixed file should be dropped entirely")
	}
	if _, ok := out["/tmp/cache.bin"]; ok {
		t.Fatal("/tmp/ prefixed file should be dropped entirely")
	}
	if got, ok := out["workspace/app.log"]; !ok || string(got) != "status=completed" {
		t.Fatalf("non-scratch file should survive untouched, got %q, ok=%v", got, ok)
	}
}

// TestExcludeCredentialsRedactsSurvivingContent is the seam test for the
// move: it proves ExcludeCredentials still runs content through the shared
// redactor, without re-asserting any individual pattern.
func TestExcludeCredentialsRedactsSurvivingContent(t *testing.T) {
	fakePassword := strings.Repeat("h", 12)
	files := map[string][]byte{
		"workspace/.env": []byte("DB_PASSWORD=" + fakePassword),
	}
	out := ExcludeCredentials(files)
	got := string(out["workspace/.env"])
	if strings.Contains(got, fakePassword) {
		t.Fatalf("password leaked into the snapshot payload: %q", got)
	}
	if !strings.Contains(got, redact.Redacted) {
		t.Fatalf("expected the shared redaction marker, got %q", got)
	}
	if !strings.Contains(got, "DB_PASSWORD=") {
		t.Fatalf("key name should be preserved for diagnosability: %q", got)
	}
}
