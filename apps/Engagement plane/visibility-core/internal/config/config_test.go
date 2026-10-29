package config

import "testing"

func TestLoadGoogleConsentDestinations(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "internal-test-key")
	t.Setenv("MARKETING_GOOGLE_CONSENT_RETURN_URL", "")
	t.Setenv("MARKETING_GOOGLE_OAUTH_CALLBACK_URL", "")
	cfg, err := Load()
	if err != nil || cfg.GoogleConsentReturnURL != "http://localhost:5173/studio/pulse" || cfg.GoogleOAuthCallbackURL != "http://localhost:3026/oauth/callback/google" {
		t.Fatalf("unexpected consent configuration: %v", err)
	}
	t.Setenv("MARKETING_GOOGLE_CONSENT_RETURN_URL", "https://app.example.com/studio/pulse")
	t.Setenv("MARKETING_GOOGLE_OAUTH_CALLBACK_URL", "https://integrations.example.com/oauth/callback/google")
	cfg, err = Load()
	if err != nil || cfg.GoogleConsentReturnURL != "https://app.example.com/studio/pulse" || cfg.GoogleOAuthCallbackURL != "https://integrations.example.com/oauth/callback/google" {
		t.Fatalf("explicit consent configuration rejected: %v", err)
	}
	for _, key := range []string{"MARKETING_GOOGLE_CONSENT_RETURN_URL", "MARKETING_GOOGLE_OAUTH_CALLBACK_URL"} {
		t.Run(key, func(t *testing.T) {
			t.Setenv(key, "http://evil.example/callback")
			if _, err := Load(); err == nil {
				t.Fatal("unsafe consent destination accepted")
			}
		})
	}
}

func TestLoadUsesDefaultsWithInternalAPIKey(t *testing.T) {
	t.Setenv("PORT", "")
	t.Setenv("INTERNAL_API_KEY", "marketing-internal-key")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load error: %v", err)
	}
	if cfg.Port != 3165 || cfg.InternalAPIKey != "marketing-internal-key" {
		t.Fatalf("config = %#v, want default port and configured key", cfg)
	}
}

func TestLoadRejectsInvalidPortAndMissingKey(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "marketing-internal-key")
	t.Setenv("PORT", "not-a-port")
	if _, err := Load(); err == nil {
		t.Fatal("Load error = nil, want invalid port rejection")
	}

	t.Setenv("PORT", "3165")
	t.Setenv("INTERNAL_API_KEY", "")
	if _, err := Load(); err == nil {
		t.Fatal("Load error = nil, want missing key rejection")
	}
}

func TestLoadAcceptsFixedPulseSourceConfiguration(t *testing.T) {
	t.Setenv("PORT", "")
	t.Setenv("INTERNAL_API_KEY", "internal-test-key")
	t.Setenv("INTEGRATION_CORE_URL", "http://integration-core:3026")
	t.Setenv("GOOGLE_CONNECTION_ID", "google-connection")
	t.Setenv("GOOGLE_GSC_PROPERTY", "sc-domain:aquatiq.com")
	t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", "/run/secrets/integration-bearer-token")
	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.GoogleGSCProperty != "sc-domain:aquatiq.com" || cfg.IntegrationBearerTokenFile != "/run/secrets/integration-bearer-token" {
		t.Fatalf("pulse config=%#v", cfg)
	}
}

func TestLoadRejectsInvalidPulseDestinationOrProperty(t *testing.T) {
	t.Setenv("PORT", "")
	t.Setenv("INTERNAL_API_KEY", "internal-test-key")
	t.Setenv("GOOGLE_CONNECTION_ID", "google-connection")
	t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", "/run/secrets/integration-bearer-token")
	t.Setenv("GOOGLE_GSC_PROPERTY", "sc-domain:aquatiq.com")
	t.Setenv("INTEGRATION_CORE_URL", "http://evil.example")
	if _, err := Load(); err == nil {
		t.Fatal("insecure outbound destination accepted")
	}
	t.Setenv("INTEGRATION_CORE_URL", "https://integration.example")
	t.Setenv("GOOGLE_GSC_PROPERTY", "sc-domain:localhost")
	if _, err := Load(); err == nil {
		t.Fatal("unsafe preferred property accepted")
	}
}

func TestLoadAllowsNoPreferredPropertyForSiteSelection(t *testing.T) {
	t.Setenv("PORT", "")
	t.Setenv("INTERNAL_API_KEY", "internal-test-key")
	t.Setenv("INTEGRATION_CORE_URL", "http://integration-core:3026")
	t.Setenv("GOOGLE_CONNECTION_ID", "google-connection")
	t.Setenv("GOOGLE_GSC_PROPERTY", "")
	t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", "/run/secrets/integration-bearer-token")
	if _, err := Load(); err != nil {
		t.Fatalf("optional preferred property rejected: %v", err)
	}
}

func TestLoadAcceptsBrokerBeforeSourceAuthorization(t *testing.T) {
	t.Setenv("PORT", "")
	t.Setenv("INTERNAL_API_KEY", "internal-test-key")
	t.Setenv("GOOGLE_CONNECTION_ID", "")
	t.Setenv("GOOGLE_GSC_PROPERTY", "")
	t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", "")
	for _, destination := range []string{"http://integration-api:3026", "http://localhost:33026", "https://integration.example"} {
		t.Run(destination, func(t *testing.T) {
			t.Setenv("INTEGRATION_CORE_URL", destination)
			cfg, err := Load()
			if err != nil {
				t.Fatalf("broker-only config rejected: %v", err)
			}
			if cfg.IntegrationCoreURL != destination || cfg.GoogleConnectionID != "" || cfg.IntegrationBearerTokenFile != "" {
				t.Fatal("broker config unexpectedly added source authorization")
			}
		})
	}
}

func TestLoadRejectsUnsafeBrokerBeforeSourceAuthorization(t *testing.T) {
	t.Setenv("PORT", "")
	t.Setenv("INTERNAL_API_KEY", "internal-test-key")
	t.Setenv("GOOGLE_CONNECTION_ID", "")
	t.Setenv("GOOGLE_GSC_PROPERTY", "")
	t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", "")
	for _, destination := range []string{"http://evil.example", "https://user@integration.example", "https://integration.example/path", "https://integration.example/?token=value"} {
		t.Run(destination, func(t *testing.T) {
			t.Setenv("INTEGRATION_CORE_URL", destination)
			if _, err := Load(); err == nil {
				t.Fatal("unsafe broker-only destination accepted")
			}
		})
	}
}

func TestLoadRejectsPartialSourceAuthorization(t *testing.T) {
	t.Setenv("PORT", "")
	t.Setenv("INTERNAL_API_KEY", "internal-test-key")
	for _, tc := range []struct{ name, destination, connection, property, bearer string }{
		{"connection only", "", "google-connection", "", ""},
		{"bearer only", "", "", "", "/run/secrets/token"},
		{"property only", "", "", "sc-domain:aquatiq.com", ""},
		{"broker and connection", "http://integration-api:3026", "google-connection", "", ""},
		{"broker and property", "http://integration-api:3026", "", "sc-domain:aquatiq.com", ""},
		{"connection and bearer without broker", "", "google-connection", "", "/run/secrets/token"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			t.Setenv("INTEGRATION_CORE_URL", tc.destination)
			t.Setenv("GOOGLE_CONNECTION_ID", tc.connection)
			t.Setenv("GOOGLE_GSC_PROPERTY", tc.property)
			t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", tc.bearer)
			if _, err := Load(); err == nil {
				t.Fatal("partial source authorization accepted")
			}
		})
	}
}

func TestLoadAcceptsPublicDiagnosticsWithoutSearchConsoleConnection(t *testing.T) {
	t.Setenv("PORT", "")
	t.Setenv("INTERNAL_API_KEY", "internal-test-key")
	t.Setenv("INTEGRATION_CORE_URL", "http://integration-api:3026")
	t.Setenv("GOOGLE_CONNECTION_ID", "")
	t.Setenv("GOOGLE_GSC_PROPERTY", "")
	t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", "/run/secrets/token")
	if _, err := Load(); err != nil {
		t.Fatalf("public diagnostics require broker bearer, not private Search Console grant: %v", err)
	}
}
