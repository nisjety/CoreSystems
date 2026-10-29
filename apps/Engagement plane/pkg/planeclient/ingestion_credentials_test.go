package planeclient

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func ingestionClaims(org string, expires int64, nonce string) map[string]any {
	return map[string]any{"aud": "ingestion", "org_id": org, "sub": "service:social-core", "service_id": "service:social-core", "principal_type": "service", "exp": expires, "scopes": []string{"integration:read"}, "jti": nonce}
}

func TestIngestionBearerReloadsProtectedAudienceScopedFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "bearer")
	writeClaims(t, path, ingestionClaims("org-test", time.Now().Add(time.Hour).Unix(), "first"))
	first, err := IngestionBearer("social-core", "org-test", path)
	if err != nil || first == "" {
		t.Fatalf("first credential: %v", err)
	}
	writeClaims(t, path, ingestionClaims("org-test", time.Now().Add(time.Hour).Unix(), "second"))
	second, err := IngestionBearer("social-core", "org-test", path)
	if err != nil || second == first {
		t.Fatalf("credential did not rotate: %v", err)
	}
	writeClaims(t, path, ingestionClaims("org-test", time.Now().Add(-time.Minute).Unix(), "expired"))
	if _, err := IngestionBearer("social-core", "org-test", path); err == nil {
		t.Fatal("expired rotation accepted")
	}
}

func TestIngestionBearerRejectsWrongBindingAndUnsafeFiles(t *testing.T) {
	path := filepath.Join(t.TempDir(), "bearer")
	writeClaims(t, path, ingestionClaims("org-other", time.Now().Add(time.Hour).Unix(), "foreign"))
	if _, err := IngestionBearer("social-core", "org-test", path); err == nil {
		t.Fatal("wrong organization accepted")
	}
	writeClaims(t, path, ingestionClaims("org-test", time.Now().Add(time.Hour).Unix(), "safe"))
	for _, scope := range []struct{ core, org, file string }{
		{"leads-core", "org-test", path}, {"Social-core", "org-test", path},
		{"social-core", " org-test", path}, {"social-core", "org/test", path},
		{"social-core", "org-test", "relative"}, {"social-core", "org-test", path + "/../bearer"},
	} {
		if _, err := IngestionBearer(scope.core, scope.org, scope.file); err == nil {
			t.Fatalf("unsafe binding accepted: %+v", scope)
		}
	}
	if err := os.Chmod(path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := IngestionBearer("social-core", "org-test", path); err == nil {
		t.Fatal("publicly readable file accepted")
	}
	if err := os.Chmod(path, 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(t.TempDir(), "link")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	if _, err := IngestionBearer("social-core", "org-test", link); err == nil {
		t.Fatal("symlink accepted")
	}
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if _, err := IngestionBearer("social-core", "org-test", path); err == nil || strings.Contains(err.Error(), path) {
		t.Fatal("missing credential accepted or path exposed")
	}
}
