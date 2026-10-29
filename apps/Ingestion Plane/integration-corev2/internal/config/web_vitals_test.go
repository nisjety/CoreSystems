package config

import (
	"encoding/base64"
	"testing"
)

func TestWebVitalsConfigurationNamesOnlyProtectedBindings(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("WEB_VITALS_BINDINGS_FILE", "/run/secrets/web_vitals_bindings")
	got, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if got.WebVitalsBindingsFile != "/run/secrets/web_vitals_bindings" {
		t.Fatal("missing protected bindings path")
	}
}
