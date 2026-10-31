package config

import (
	"encoding/base64"
	"slices"
	"testing"
)

func TestSiteSourcesUseProtectedMappingAndNeverClaimGlobalTenantReadiness(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("MARKETING_PROVIDERS_BINDINGS_FILE", "/run/secrets/marketing_providers_bindings")
	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.MarketingProvidersBindingsFile != "/run/secrets/marketing_providers_bindings" {
		t.Fatal("missing protected path")
	}
	readiness := cfg.ProviderReadiness()
	if !slices.Contains(readiness["cookiebot"], "SITE_SCOPED_API_KEY_BINDING") || !slices.Contains(readiness["algolia"], "ALGOLIA_MCP_OAUTH_CLIENT_FILE") {
		t.Fatal("global deployment file was treated as tenant connection")
	}
}
