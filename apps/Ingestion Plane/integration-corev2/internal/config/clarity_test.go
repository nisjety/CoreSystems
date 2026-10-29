package config

import (
	"encoding/base64"
	"testing"
)

func TestClarityConfigurationUsesOnlyProtectedBindingPath(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("CLARITY_BINDINGS_FILE", "/run/secrets/clarity_bindings")
	got, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if got.ClarityBindingsFile != "/run/secrets/clarity_bindings" {
		t.Fatal("missingClaritybindingpath")
	}
	if got.GoogleAnalyticsDataAPIBaseURL != "https://analyticsdata.googleapis.com" {
		t.Fatal("incorrectGA4defaultendpoint")
	}
	for _, name := range []string{"CLARITY_TOKEN", "CLARITY_API_TOKEN"} {
		t.Setenv(name, "synthetic-inline-secret")
		if _, err := Load(); err == nil {
			t.Fatal("inlineClaritysecretenvironmentaccepted")
		}
		t.Setenv(name, "")
	}
}
