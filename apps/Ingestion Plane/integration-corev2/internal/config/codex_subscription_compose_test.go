package config

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// integrationAPIComposeBlock returns the integration-api service block of the
// Ingestion Plane compose file.
func integrationAPIComposeBlock(t *testing.T) string {
	t.Helper()
	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}
	compose, err := os.ReadFile(filepath.Join(filepath.Dir(sourceFile), "..", "..", "..", "docker-compose.yml"))
	if err != nil {
		t.Fatalf("read ingestion compose file: %v", err)
	}
	text := strings.ReplaceAll(string(compose), "\r\n", "\n")
	start := strings.Index(text, "\n  integration-api:\n")
	if start < 0 {
		t.Fatal("integration-api service not found")
	}
	block := text[start+1:]
	lines := strings.Split(block, "\n")
	for index := 1; index < len(lines); index++ {
		line := lines[index]
		if strings.HasPrefix(line, "  ") && !strings.HasPrefix(line, "   ") && strings.TrimSpace(line) != "" && !strings.HasPrefix(strings.TrimSpace(line), "#") {
			return strings.Join(lines[:index], "\n")
		}
	}
	return block
}

func TestIntegrationAPIComposeOwnsCodexBrokerKnobs(t *testing.T) {
	block := integrationAPIComposeBlock(t)
	if !strings.Contains(block, "\n    init: true\n") {
		t.Fatal("integration-api must run with init: true so Codex process trees are reaped")
	}
	// Each compose default must equal the default Load applies in code.
	for _, line := range []string{
		"CODEX_SUBSCRIPTION_TRANSPORT: ${CODEX_SUBSCRIPTION_TRANSPORT:-app-server}",
		"CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR: ${CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR:-verevon}",
		"CODEX_SUBSCRIPTION_DIRECT_REQUEST_SHAPE: ${CODEX_SUBSCRIPTION_DIRECT_REQUEST_SHAPE:-classic}",
		"CODEX_SUBSCRIPTION_DIRECT_PROMPT_MODE: ${CODEX_SUBSCRIPTION_DIRECT_PROMPT_MODE:-roles}",
		"CODEX_SUBSCRIPTION_DIRECT_MAX_RETRIES: ${CODEX_SUBSCRIPTION_DIRECT_MAX_RETRIES:-2}",
		"CODEX_SUBSCRIPTION_REFRESH_SKEW: ${CODEX_SUBSCRIPTION_REFRESH_SKEW:-10m}",
		"CODEX_SUBSCRIPTION_MAX_CONCURRENCY: ${CODEX_SUBSCRIPTION_MAX_CONCURRENCY:-16}",
		"CODEX_SUBSCRIPTION_MAX_CONCURRENCY_PER_CONNECTION: ${CODEX_SUBSCRIPTION_MAX_CONCURRENCY_PER_CONNECTION:-4}",
		"CODEX_SUBSCRIPTION_QUEUE_TIMEOUT: ${CODEX_SUBSCRIPTION_QUEUE_TIMEOUT:-30s}",
		"CODEX_APP_SERVER_HARDENED: ${CODEX_APP_SERVER_HARDENED:-true}",
		"CODEX_APP_SERVER_THREAD_EFFORT: ${CODEX_APP_SERVER_THREAD_EFFORT:-true}",
	} {
		if !strings.Contains(block, "\n      "+line+"\n") {
			t.Fatalf("integration-api environment lacks %q", line)
		}
	}
	if strings.Contains(block, "CODEX_SUBSCRIPTION_MODEL_PLANE_API_KEY:") {
		t.Fatal("the dedicated Model Plane key must stay in integration-corev2/.env, not compose interpolation")
	}
	// The image-specific command default intentionally differs from the code
	// default ("codex", the npm launcher): it is the stable native path the
	// API Dockerfile links, and compose environment overrides the env_file.
	if !strings.Contains(block, "\n      CODEX_APP_SERVER_COMMAND: ${CODEX_APP_SERVER_COMMAND:-"+codexNativeImagePath+"}\n") {
		t.Fatalf("integration-api must default CODEX_APP_SERVER_COMMAND to %s", codexNativeImagePath)
	}
	// Sized for per-call app-server spawns: 2 CPUs throttled 9.3% of periods
	// and 2 GiB was reached.
	for _, line := range []string{
		"memory: ${INTEGRATION_API_MEMORY_LIMIT:-4G}",
		`cpus: "${INTEGRATION_API_CPU_LIMIT:-6.0}"`,
	} {
		if !strings.Contains(block, "\n          "+line+"\n") {
			t.Fatalf("integration-api resource limits lack %q", line)
		}
	}
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.CodexSubscriptionTransport != "app-server" || cfg.CodexSubscriptionDirectMaxRetries != 2 || cfg.CodexSubscriptionMaxConcurrency != 16 || !cfg.CodexAppServerHardened || !cfg.CodexAppServerThreadEffort {
		t.Fatal("code defaults drifted from the compose defaults")
	}
}

// TestComposeThreadEffortDefaultMatchesCode reads the compose default of
// CODEX_APP_SERVER_THREAD_EFFORT, loads it the way the container does, and
// requires the same value as an unset variable, so the two defaults cannot
// drift apart.
func TestComposeThreadEffortDefaultMatchesCode(t *testing.T) {
	const prefix = "\n      CODEX_APP_SERVER_THREAD_EFFORT: ${CODEX_APP_SERVER_THREAD_EFFORT:-"
	block := integrationAPIComposeBlock(t)
	if strings.Count(block, "\n      CODEX_APP_SERVER_THREAD_EFFORT:") != 1 {
		t.Fatal("integration-api must set CODEX_APP_SERVER_THREAD_EFFORT exactly once")
	}
	start := strings.Index(block, prefix)
	if start < 0 {
		t.Fatal("integration-api environment lacks a CODEX_APP_SERVER_THREAD_EFFORT default")
	}
	composeDefault, _, found := strings.Cut(block[start+len(prefix):], "}\n")
	if !found {
		t.Fatal("unterminated CODEX_APP_SERVER_THREAD_EFFORT default")
	}
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
	t.Setenv("CODEX_APP_SERVER_THREAD_EFFORT", "")
	unset, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv("CODEX_APP_SERVER_THREAD_EFFORT", composeDefault)
	fromCompose, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if len(fromCompose.codexSubscriptionUnparsed) != 0 {
		t.Fatalf("compose default %q does not parse: %v", composeDefault, fromCompose.codexSubscriptionUnparsed)
	}
	if fromCompose.CodexAppServerThreadEffort != unset.CodexAppServerThreadEffort || !unset.CodexAppServerThreadEffort {
		t.Fatalf("compose default %q loads as %v, code default is %v; both must be on", composeDefault, fromCompose.CodexAppServerThreadEffort, unset.CodexAppServerThreadEffort)
	}
}
