package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestAlgoliaRegistrationProtectedAndPinned(t *testing.T) {
	path := filepath.Join(t.TempDir(), "client.json")
	good := `{"version":1,"clientId":"owned-public-client","redirectUri":"http://localhost:3026/api/v1/providers/algolia/callback","resource":"https://mcp.algolia.com/mcp"}`
	cfg := Config{PublicBaseURL: "http://localhost:3026", AlgoliaMCPOAuthClientFile: path}
	for _, tc := range []struct {
		body  string
		mode  os.FileMode
		valid bool
	}{{good, 0600, true}, {good, 0644, false}, {`{"version":1}`, 0600, false}, {good[:len(good)-1] + `,"clientId":"other"}`, 0600, false}} {
		if err := os.WriteFile(path, []byte(tc.body), 0600); err != nil {
			t.Fatal(err)
		}
		_ = os.Chmod(path, tc.mode)
		_, err := cfg.AlgoliaRegistration()
		if (err == nil) != tc.valid {
			t.Fatalf("valid=%v err=%v", tc.valid, err)
		}
	}
	_ = os.WriteFile(path, []byte(good), 0600)
	_ = os.Chmod(path, 0600)
	cfg.PublicBaseURL = "http://localhost:9999"
	if _, err := cfg.AlgoliaRegistration(); err == nil {
		t.Fatal("unregistered callback accepted")
	}
}
