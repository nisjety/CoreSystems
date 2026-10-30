package config

import "testing"

func TestRegistryConfigurationRequiresBoundOrganizationAndAbsoluteFile(t *testing.T) {
	for _, key := range []string{"PORT", "INTEGRATION_CORE_URL", "GOOGLE_CONNECTION_ID", "GOOGLE_GSC_PROPERTY", "INTEGRATION_BEARER_TOKEN_FILE", "MARKETING_ORGANIZATION_ID", "MARKETING_WEBSITES_FILE", "MARKETING_INITIAL_SITE"} {
		t.Setenv(key, "")
	}
	t.Setenv("INTERNAL_API_KEY", "test-key")
	for _, tc := range []struct {
		org, file, site string
		valid           bool
	}{
		{"org-one", "/var/lib/marketing/websites.json", "", true},
		{"org-one", "/var/lib/marketing/websites.json", "https://example.com/", true},
		{"", "/var/lib/marketing/websites.json", "", false},
		{"org-one", "relative.json", "", false},
		{"../other", "/var/lib/marketing/websites.json", "", false},
		{"org-one", "/var/lib/marketing/../websites.json", "", false},
		{"org-one", "/var/lib/marketing/websites.json", "https://localhost/", false},
		{"org-one", "/var/lib/marketing/websites.json", "https://example.com", false},
	} {
		t.Run(tc.org+tc.file+tc.site, func(t *testing.T) {
			t.Setenv("MARKETING_ORGANIZATION_ID", tc.org)
			t.Setenv("MARKETING_WEBSITES_FILE", tc.file)
			t.Setenv("MARKETING_INITIAL_SITE", tc.site)
			cfg, err := Load()
			if (err == nil) != tc.valid {
				t.Fatalf("valid=%v err=%v", tc.valid, err)
			}
			if err == nil && cfg.OrganizationID != tc.org {
				t.Fatal("organization binding lost")
			}
		})
	}
}
