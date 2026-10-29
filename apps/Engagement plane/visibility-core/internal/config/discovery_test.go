package config

import "testing"

func TestQuarryDiscoveryConfigIsOptionalSeparateAndFixed(t *testing.T) {
	for _, key := range []string{"PORT", "INTEGRATION_CORE_URL", "GOOGLE_CONNECTION_ID", "GOOGLE_GSC_PROPERTY", "INTEGRATION_BEARER_TOKEN_FILE", "MARKETING_INITIAL_SITE", "MARKETING_QUARRY_URL", "MARKETING_QUARRY_BEARER_TOKEN_FILE"} {
		t.Setenv(key, "")
	}
	t.Setenv("INTERNAL_API_KEY", "test-key")
	t.Setenv("MARKETING_ORGANIZATION_ID", "org-one")
	t.Setenv("MARKETING_WEBSITES_FILE", "/var/lib/marketing/websites.json")
	if c, err := Load(); err != nil || c.QuarryURL != "" || c.QuarryBearerTokenFile != "" {
		t.Fatal("optional config", err)
	}
	for _, tc := range []struct {
		endpoint, file string
		valid          bool
	}{{"http://quarry-edge:8082", "/run/secrets/quarry_token", true}, {"https://quarry.example", "/run/secrets/quarry_token", true}, {"http://evil.example", "/run/secrets/quarry_token", false}, {"https://quarry.example/path", "/run/secrets/quarry_token", false}, {"https://quarry.example", "", false}, {"", "/run/secrets/quarry_token", false}} {
		t.Setenv("MARKETING_QUARRY_URL", tc.endpoint)
		t.Setenv("MARKETING_QUARRY_BEARER_TOKEN_FILE", tc.file)
		cfg, err := Load()
		if (err == nil) != tc.valid {
			t.Fatalf("%s valid%v err%v", tc.endpoint, tc.valid, err)
		}
		if err == nil && cfg.QuarryURL != tc.endpoint {
			t.Fatal("destination lost")
		}
	}
	t.Setenv("MARKETING_QUARRY_URL", "https://quarry.example")
	t.Setenv("MARKETING_QUARRY_BEARER_TOKEN_FILE", "/run/secrets/token")
	t.Setenv("INTEGRATION_CORE_URL", "http://integration-api:3026")
	t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", "/run/secrets/token")
	if _, err := Load(); err == nil {
		t.Fatal("integration bearer reused for quarry")
	}
}
