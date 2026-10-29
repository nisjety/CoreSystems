package config

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestIntegrationAPIVerifierUsesCanonicalAuthCoreIssuer(t *testing.T) {
	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}

	composePath := filepath.Join(filepath.Dir(sourceFile), "..", "..", "..", "docker-compose.yml")
	compose, err := os.ReadFile(composePath)
	if err != nil {
		t.Fatalf("read ingestion compose file: %v", err)
	}

	const expected = "PLANE_TOKEN_ISSUER: ${AUTH_CORE_ISSUER:-http://localhost:3011/api/convex-auth}"
	if !strings.Contains(string(compose), expected) {
		t.Fatalf("integration-api verifier issuer must derive from AUTH_CORE_ISSUER; expected %q", expected)
	}
}
