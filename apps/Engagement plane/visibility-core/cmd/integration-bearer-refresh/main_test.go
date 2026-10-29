package main

import (
	"encoding/base64"
	"encoding/json"
	"flag"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestDeploymentConfigFromEnvironment(t *testing.T) {
	t.Setenv("AUTH_CORE_URL", "http://auth-core:3011")
	t.Setenv("MARKETING_INTEGRATION_ORGANIZATION_ID", "test-org")
	t.Setenv("MARKETING_INTEGRATION_SERVICE_ID", "marketing-core-triodelab")
	t.Setenv("MARKETING_INTEGRATION_SERVICE_KEY_FILE", "/run/secrets/service-key")
	t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", "/run/marketing-bearer/token")
	cfg := configFromEnvironment()
	if cfg.ServiceID != "marketing-core-triodelab" || cfg.OrganizationID != "test-org" || cfg.AuthCoreURL != "http://auth-core:3011" || cfg.ServiceKeyFile != "/run/secrets/service-key" || cfg.TokenFile != "/run/marketing-bearer/token" {
		t.Fatal("deployment environment was not projected into the bearer configuration")
	}
	t.Setenv("MARKETING_INTEGRATION_SERVICE_ID", "")
	if cfg := configFromEnvironment(); cfg.ServiceID != "" {
		t.Fatal("unset service ID must preserve the library's default behavior")
	}
}

func TestCommandChecksSyntheticBoundTokenWithoutIssuance(t *testing.T) {
	for _, service := range []string{"", "marketing-core-triodelab"} {
		t.Run("service="+service, func(t *testing.T) {
			directory := t.TempDir()
			if err := os.Chmod(directory, 0700); err != nil {
				t.Fatal(err)
			}
			keyFile, tokenFile := filepath.Join(directory, "key"), filepath.Join(directory, "token")
			if err := os.WriteFile(keyFile, []byte("synthetic-check-only-key"), 0600); err != nil {
				t.Fatal(err)
			}
			principal := service
			if principal == "" {
				principal = "marketing-core"
			}
			now := time.Now().UTC()
			claims, err := json.Marshal(map[string]any{
				"iss": "http://localhost:3011/api/convex-auth", "aud": "ingestion",
				"sub": "service:" + principal, "service_id": "service:" + principal,
				"principal_type": "service", "org_id": "test-org", "scopes": []string{"integration:read"},
				"iat": now.Unix(), "nbf": now.Add(-5 * time.Second).Unix(), "exp": now.Add(5 * time.Minute).Unix(),
			})
			if err != nil {
				t.Fatal(err)
			}
			header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"RS256","kid":"synthetic-test-only"}`))
			token := header + "." + base64.RawURLEncoding.EncodeToString(claims) + ".synthetic-signature"
			if err := os.WriteFile(tokenFile, []byte(token), 0600); err != nil {
				t.Fatal(err)
			}
			t.Setenv("AUTH_CORE_URL", "http://127.0.0.1:3011")
			t.Setenv("MARKETING_INTEGRATION_ORGANIZATION_ID", "test-org")
			t.Setenv("MARKETING_INTEGRATION_SERVICE_ID", service)
			t.Setenv("MARKETING_INTEGRATION_SERVICE_KEY_FILE", keyFile)
			t.Setenv("INTEGRATION_BEARER_TOKEN_FILE", tokenFile)
			originalFlags, originalArgs := flag.CommandLine, os.Args
			t.Cleanup(func() { flag.CommandLine, os.Args = originalFlags, originalArgs })
			flag.CommandLine = flag.NewFlagSet("integration-bearer-refresh", flag.ContinueOnError)
			os.Args = []string{"integration-bearer-refresh", "--check"}
			main()
		})
	}
}
