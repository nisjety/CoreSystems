package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestPageSpeedSecretIsOptionalFileOnlyAndValidated(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", "12345678901234567890123456789012")
	t.Setenv("ENVIRONMENT", "test")
	t.Setenv("PAGESPEED_API_KEY", "")
	t.Setenv("PAGESPEED_API_KEY_FILE", "")
	cfg, err := Load()
	if err != nil || cfg.PageSpeedAPIKey != "" {
		t.Fatalf("optional key: %v", err)
	}
	path := filepath.Join(t.TempDir(), "pagespeed-key")
	if err := os.WriteFile(path, []byte("synthetic-pagespeed-key\n"), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PAGESPEED_API_KEY_FILE", path)
	cfg, err = Load()
	if err != nil || cfg.PageSpeedAPIKey != "synthetic-pagespeed-key" {
		t.Fatalf("file key load: %v", err)
	}
	t.Setenv("PAGESPEED_API_KEY", "must-not-appear-in-errors")
	if _, err = Load(); err == nil || strings.Contains(err.Error(), "must-not-appear-in-errors") {
		t.Fatal("raw environment secret was allowed or leaked")
	}
	t.Setenv("PAGESPEED_API_KEY", "")
	t.Setenv("PAGESPEED_API_KEY_FILE", filepath.Join(t.TempDir(), "missing"))
	if _, err = Load(); err == nil {
		t.Fatal("missing key file accepted")
	}
}
