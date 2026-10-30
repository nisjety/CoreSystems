package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCrUXKeyIsOptionalDedicatedProtectedFileOnly(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", "12345678901234567890123456789012")
	t.Setenv("ENVIRONMENT", "test")
	t.Setenv("CRUX_API_KEY", "")
	t.Setenv("CRUX_API_KEY_FILE", "")
	t.Setenv("PAGESPEED_API_KEY", "")
	t.Setenv("PAGESPEED_API_KEY_FILE", "")
	cfg, err := Load()
	if err != nil || cfg.CrUXAPIKey != "" {
		t.Fatal("optional CrUX key", err)
	}
	path := filepath.Join(t.TempDir(), "crux-key")
	if err := os.WriteFile(path, []byte("synthetic-crux-key\n"), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CRUX_API_KEY_FILE", path)
	cfg, err = Load()
	if err != nil || cfg.CrUXAPIKey != "synthetic-crux-key" || cfg.PageSpeedAPIKey != "" {
		t.Fatal("dedicated key load", err)
	}
	t.Setenv("CRUX_API_KEY", "inline-secret-must-not-leak")
	if _, err := Load(); err == nil || strings.Contains(err.Error(), "inline-secret") {
		t.Fatal("inline key accepted/leaked")
	}
	t.Setenv("CRUX_API_KEY", "")
	if err := os.Chmod(path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := Load(); err == nil {
		t.Fatal("public key file accepted")
	}
	if err := os.Chmod(path, 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(t.TempDir(), "link")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CRUX_API_KEY_FILE", link)
	if _, err := Load(); err == nil {
		t.Fatal("symlink accepted")
	}
	t.Setenv("CRUX_API_KEY_FILE", "")
	t.Setenv("PAGESPEED_API_KEY_FILE", path)
	cfg, err = Load()
	if err != nil || cfg.CrUXAPIKey != "" {
		t.Fatal("PageSpeed secret reused for CrUX", err)
	}
}
