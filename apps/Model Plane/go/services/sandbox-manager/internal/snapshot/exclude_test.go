package snapshot

import (
	"strings"
	"testing"
)

func TestExcludeCredentialsDropsScratchOnlyPathsEntirely(t *testing.T) {
	// Content here need not look secret-shaped at all: this test is about
	// whole-file path exclusion, not redaction.
	files := map[string][]byte{
		"scratch/notes.txt": []byte("scratch working notes"),
		"/tmp/cache.bin":     []byte("temporary cache contents"),
		"workspace/app.log":  []byte("status=completed"),
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

func TestExcludeCredentialsRedactsBearerAndJWT(t *testing.T) {
	out := scrubString("Authorization: Bearer eyJhbGciOi.eyJzdWIi.sig and sk-abcdef1234567890XYZ")
	if !strings.Contains(out, redacted) {
		t.Fatalf("expected redaction marker, got %q", out)
	}
	if strings.Contains(out, "eyJhbGciOi") {
		t.Fatalf("JWT leaked: %q", out)
	}
	if strings.Contains(out, "sk-abcdef") {
		t.Fatalf("sk- token leaked: %q", out)
	}
}

func TestExcludeCredentialsRedactsConnectionStringPasswordKeepsHost(t *testing.T) {
	// Valid DSNs URL-encode reserved characters, so the password has no raw '@'.
	out := scrubString("postgres://svc_user:s3cr3tPass@db.internal:5432/app")
	if !strings.Contains(out, redacted) {
		t.Fatalf("expected redaction marker, got %q", out)
	}
	if strings.Contains(out, "s3cr3tPass") {
		t.Fatalf("password leaked: %q", out)
	}
	if !strings.Contains(out, "postgres://svc_user:") || !strings.Contains(out, "@db.internal:5432/app") {
		t.Fatalf("scheme/user/host should be preserved for diagnosability: %q", out)
	}
}

func TestExcludeCredentialsRedactsInlineAssignmentsKeepsKey(t *testing.T) {
	// Placeholder values (repeated characters, not a plausible real secret)
	// still exercise the capture regex's shape match without looking like a
	// leaked credential to a secret scanner.
	fakePassword := strings.Repeat("h", 12)
	fakeAPIKey := strings.Repeat("k", 12)
	out := scrubString(`export DB_PASSWORD=` + fakePassword + ` && api_key: "` + fakeAPIKey + `"`)
	if strings.Contains(out, fakePassword) {
		t.Fatalf("value leaked: %q", out)
	}
	if strings.Contains(out, fakeAPIKey) {
		t.Fatalf("value leaked: %q", out)
	}
	if !strings.Contains(out, "DB_PASSWORD=") || !strings.Contains(out, "api_key") {
		t.Fatalf("key names should be preserved: %q", out)
	}
}

func TestExcludeCredentialsRedactsSlackAndGoogleKeys(t *testing.T) {
	// Placeholder values shaped to match each pattern's character-class and
	// length requirements, built from a repeated character rather than a
	// plausible real token.
	fakeSlackToken := "xoxb-" + strings.Repeat("1", 15)
	fakeGoogleKey := "AIza" + strings.Repeat("z", 35)
	out := scrubString("tok=" + fakeSlackToken + " key=" + fakeGoogleKey)
	if strings.Contains(out, fakeSlackToken) {
		t.Fatalf("slack token leaked: %q", out)
	}
	if strings.Contains(out, fakeGoogleKey) {
		t.Fatalf("google api key leaked: %q", out)
	}
}

func TestExcludeCredentialsPreservesOrdinaryAssignments(t *testing.T) {
	out := scrubString("status=completed path=/usr/bin count=42")
	if out != "status=completed path=/usr/bin count=42" {
		t.Fatalf("non-secret assignments must survive untouched, got %q", out)
	}
}
