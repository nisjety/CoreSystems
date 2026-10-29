package main

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestComposeHealthcheckUsesLivenessNotSemanticReadiness(t *testing.T) {
	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}
	composePath := filepath.Join(filepath.Dir(sourceFile), "..", "..", "..", "..", "deploy", "docker-compose.yml")
	compose, err := os.ReadFile(composePath)
	if err != nil {
		t.Fatalf("read Model Plane Compose file: %v", err)
	}
	serviceStart := strings.Index(string(compose), "\n  letta-bridge:")
	serviceEnd := strings.Index(string(compose), "\n  agent-memory-redis:")
	if serviceStart < 0 || serviceEnd < 0 || serviceEnd <= serviceStart {
		t.Fatal("locate letta-bridge Compose service block")
	}
	service := string(compose)[serviceStart:serviceEnd]
	if !strings.Contains(service, "http://localhost:8088/healthz") {
		t.Fatal("letta-bridge Docker healthcheck must use liveness /healthz")
	}
	if strings.Contains(service, "http://localhost:8088/readyz") {
		t.Fatal("letta-bridge Docker healthcheck must not require an observed semantic search")
	}
}
