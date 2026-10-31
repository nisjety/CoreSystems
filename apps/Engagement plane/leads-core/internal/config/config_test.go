package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestReadSecretFileRejectsOversizedAndNonregularFiles(t *testing.T) {
	file := filepath.Join(t.TempDir(), "oversized")
	if err := os.WriteFile(file, []byte(strings.Repeat("x", 8193)), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("TEST_SECRET_FILE", file)
	if _, err := readSecretFile("TEST_SECRET_FILE"); err == nil {
		t.Fatal("oversized bearer must fail")
	}
	t.Setenv("TEST_SECRET_FILE", t.TempDir())
	if _, err := readSecretFile("TEST_SECRET_FILE"); err == nil {
		t.Fatal("directory must fail")
	}
}

func TestLoadIntegrationBearerFromFile(t *testing.T) {
	file := filepath.Join(t.TempDir(), "tenant-bearer")
	if err := os.WriteFile(file, []byte("tenant-test-token\n"), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("DATABASE_URL", "postgres://localhost/studio")
	t.Setenv("INTERNAL_API_KEY", "inbound-only")
	t.Setenv("BILLING_CORE_URL", "")
	t.Setenv("LEADS_BILLING_SERVICE_ID", "")
	t.Setenv("LEADS_BILLING_SERVICE_TOKEN", "")
	t.Setenv("INTEGRATION_CORE_TENANT_ORGANIZATION_ID", "org-1")
	t.Setenv("INTEGRATION_CORE_TENANT_BEARER_TOKEN_FILE", file)
	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.IntegrationCoreTenantBearerToken != "tenant-test-token" || cfg.IntegrationCoreTenantOrganizationID != "org-1" {
		t.Fatal("integration bearer not loaded")
	}
}

func TestIntegrationReadyRequiresURLAndTenantCredentials(t *testing.T) {
	for name, cfg := range map[string]Config{
		"ready":        {IntegrationCoreURL: "http://integration-api:3026", IntegrationCoreTenantBearerToken: "tenant-test-token", IntegrationCoreTenantOrganizationID: "org-1"},
		"URL only":     {IntegrationCoreURL: "http://integration-api:3026"},
		"missing URL":  {IntegrationCoreTenantBearerToken: "tenant-test-token", IntegrationCoreTenantOrganizationID: "org-1"},
		"missing org":  {IntegrationCoreURL: "http://integration-api:3026", IntegrationCoreTenantBearerToken: "tenant-test-token"},
		"blank values": {IntegrationCoreURL: " ", IntegrationCoreTenantBearerToken: " ", IntegrationCoreTenantOrganizationID: " "},
	} {
		t.Run(name, func(t *testing.T) {
			if got := cfg.IntegrationReady(); got != (name == "ready") {
				t.Fatalf("IntegrationReady() = %t", got)
			}
		})
	}
}

func TestLoadRejectsMissingIntegrationBearerFile(t *testing.T) {
	t.Setenv("DATABASE_URL", "postgres://localhost/studio")
	t.Setenv("INTERNAL_API_KEY", "inbound-only")
	t.Setenv("BILLING_CORE_URL", "")
	t.Setenv("LEADS_BILLING_SERVICE_ID", "")
	t.Setenv("LEADS_BILLING_SERVICE_TOKEN", "")
	t.Setenv("INTEGRATION_CORE_TENANT_BEARER_TOKEN_FILE", filepath.Join(t.TempDir(), "missing"))
	if _, err := Load(); err == nil {
		t.Fatal("missing bearer file must fail")
	}
}

func TestLoadRejectsReusingInboundKeyForIntegration(t *testing.T) {
	file := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(file, []byte("same-key"), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("DATABASE_URL", "postgres://localhost/studio")
	t.Setenv("INTERNAL_API_KEY", "same-key")
	t.Setenv("BILLING_CORE_URL", "")
	t.Setenv("LEADS_BILLING_SERVICE_ID", "")
	t.Setenv("LEADS_BILLING_SERVICE_TOKEN", "")
	t.Setenv("INTEGRATION_CORE_TENANT_ORGANIZATION_ID", "org-1")
	t.Setenv("INTEGRATION_CORE_TENANT_BEARER_TOKEN_FILE", file)
	if _, err := Load(); err == nil {
		t.Fatal("inbound key must not be reused")
	}
}

func TestLoadUsesServiceLocalNATSPassword(t *testing.T) {
	t.Setenv("DATABASE_URL", "postgres://studio:test@localhost/studio")
	t.Setenv("INTERNAL_API_KEY", "test-leads-key")
	t.Setenv("NATS_PASSWORD", "")
	t.Setenv("APPLICATION_LEADS_NATS_PASSWORD", "test-leads-nats-password")

	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.NATSPassword != "test-leads-nats-password" {
		t.Fatal("service-local NATS password was not loaded")
	}
}

func TestLoadPrefersExplicitNATSPassword(t *testing.T) {
	t.Setenv("DATABASE_URL", "postgres://studio:test@localhost/studio")
	t.Setenv("INTERNAL_API_KEY", "test-leads-key")
	t.Setenv("NATS_PASSWORD", "test-explicit-password")
	t.Setenv("APPLICATION_LEADS_NATS_PASSWORD", "test-local-password")

	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.NATSPassword != "test-explicit-password" {
		t.Fatal("explicit NATS_PASSWORD should take precedence")
	}
}

func TestLoadUsesStudioNATSPassword(t *testing.T) {
	t.Setenv("DATABASE_URL", "postgres://studio:test@localhost/studio")
	t.Setenv("INTERNAL_API_KEY", "test-leads-key")
	t.Setenv("BILLING_CORE_URL", "")
	t.Setenv("LEADS_BILLING_SERVICE_ID", "")
	t.Setenv("LEADS_BILLING_SERVICE_TOKEN", "")
	t.Setenv("NATS_PASSWORD", "")
	t.Setenv("APPLICATION_LEADS_NATS_PASSWORD", "test-legacy-password")
	t.Setenv("STUDIO_LEADS_NATS_PASSWORD", "test-studio-password")

	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.NATSPassword != "test-studio-password" {
		t.Fatal("Studio-local NATS password should take precedence over the legacy key")
	}
}

func TestLoadRequiredValuesAndBillingConfiguration(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "")
	t.Setenv("DATABASE_URL", "postgres://localhost/studio")
	if _, err := Load(); err == nil {
		t.Fatal("missing API key must fail")
	}
	t.Setenv("INTERNAL_API_KEY", "test-key")
	t.Setenv("DATABASE_URL", "")
	if _, err := Load(); err == nil {
		t.Fatal("missing database URL must fail")
	}
	t.Setenv("DATABASE_URL", "postgres://localhost/studio")
	t.Setenv("BILLING_CORE_URL", "http://billing")
	t.Setenv("LEADS_BILLING_SERVICE_ID", "")
	t.Setenv("LEADS_BILLING_SERVICE_TOKEN", "")
	if _, err := Load(); err == nil {
		t.Fatal("partial billing configuration must fail")
	}
	t.Setenv("LEADS_BILLING_SERVICE_ID", "leads")
	t.Setenv("LEADS_BILLING_SERVICE_TOKEN", "test-token")
	if _, err := Load(); err != nil {
		t.Fatal(err)
	}
}

func TestEnvironmentParsing(t *testing.T) {
	t.Setenv("CONFIG_VALUE", "  custom  ")
	if got := getEnv("CONFIG_VALUE", "fallback"); got != "custom" {
		t.Fatalf("getEnv = %q", got)
	}
	t.Setenv("CONFIG_VALUE", " ")
	if got := getEnv("CONFIG_VALUE", "fallback"); got != "fallback" {
		t.Fatalf("getEnv fallback = %q", got)
	}

	for _, tc := range []struct {
		value string
		want  int
	}{
		{"", 7}, {"42", 42}, {"nope", 7}, {"0", 7}, {"65536", 7},
	} {
		t.Setenv("CONFIG_INT", tc.value)
		if got := getEnvInt("CONFIG_INT", 7); got != tc.want {
			t.Errorf("getEnvInt(%q) = %d, want %d", tc.value, got, tc.want)
		}
	}
	for _, tc := range []struct {
		value string
		want  bool
	}{
		{"yes", true}, {"on", true}, {"1", true}, {"true", true},
		{"no", false}, {"off", false}, {"0", false}, {"false", false},
		{"", true}, {"invalid", true},
	} {
		t.Setenv("CONFIG_BOOL", tc.value)
		if got := getEnvBool("CONFIG_BOOL", true); got != tc.want {
			t.Errorf("getEnvBool(%q) = %t, want %t", tc.value, got, tc.want)
		}
	}
	for _, tc := range []struct {
		value string
		want  time.Duration
	}{
		{"", time.Second}, {"2m", 2 * time.Minute}, {"bad", time.Second}, {"0s", time.Second},
	} {
		t.Setenv("CONFIG_DURATION", tc.value)
		if got := getEnvDuration("CONFIG_DURATION", time.Second); got != tc.want {
			t.Errorf("getEnvDuration(%q) = %s, want %s", tc.value, got, tc.want)
		}
	}
}
