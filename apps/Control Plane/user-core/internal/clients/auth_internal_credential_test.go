package clients

import (
	"os"
	"path/filepath"
	"testing"
)

const userAuthInternalToken = "user-auth-internal-0123456789abcdef0123456789abcdef"

func userAuthInternalCredentialJSON() string {
	return `{"credentialId":"user-core-2026-07","principal":"user-core","audience":"auth-core-internal","token":"` + userAuthInternalToken + `"}`
}

func TestParseAuthInternalClientCredential(t *testing.T) {
	credential, err := parseAuthInternalClientCredential(userAuthInternalCredentialJSON())
	if err != nil {
		t.Fatalf("parseAuthInternalClientCredential() error = %v", err)
	}
	if credential.CredentialID != "user-core-2026-07" || credential.Principal != "user-core" || credential.Token != userAuthInternalToken {
		t.Fatalf("unexpected credential: %+v", credential)
	}
}

func TestParseAuthInternalClientCredentialFailsClosed(t *testing.T) {
	tests := []string{
		"",
		"[",
		`{"credentialId":"user-core-a","principal":"attacker","audience":"auth-core-internal","token":"` + userAuthInternalToken + `"}`,
		`{"credentialId":"user-core-a","principal":"user-core","audience":"wrong","token":"` + userAuthInternalToken + `"}`,
		`{"credentialId":"user-core-a","principal":"user-core","audience":"auth-core-internal","token":"short"}`,
	}
	for _, raw := range tests {
		if _, err := parseAuthInternalClientCredential(raw); err == nil {
			t.Fatalf("parseAuthInternalClientCredential(%q) error = nil, want failure", raw)
		}
	}
}

func TestLoadAuthInternalClientCredentialRequiresFileInProduction(t *testing.T) {
	if _, err := loadAuthInternalClientCredential(authInternalCredentialEnvironment{
		Environment:      "production",
		InlineCredential: userAuthInternalCredentialJSON(),
	}); err == nil {
		t.Fatal("loadAuthInternalClientCredential() error = nil, want production file requirement")
	}

	path := filepath.Join(t.TempDir(), "credential.json")
	if err := os.WriteFile(path, []byte(userAuthInternalCredentialJSON()), 0600); err != nil {
		t.Fatalf("write credential: %v", err)
	}
	credential, err := loadAuthInternalClientCredential(authInternalCredentialEnvironment{
		Environment:    "production",
		CredentialFile: path,
	})
	if err != nil {
		t.Fatalf("loadAuthInternalClientCredential() error = %v", err)
	}
	if credential.Token != userAuthInternalToken {
		t.Fatal("production file credential did not load")
	}
}

func TestLoadConfiguredAuthInternalClientCredentialInDevelopment(t *testing.T) {
	t.Setenv("ENVIRONMENT", "development")
	t.Setenv("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE", "")
	t.Setenv("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL", userAuthInternalCredentialJSON())

	credential, err := LoadAuthInternalClientCredential()
	if err != nil {
		t.Fatalf("LoadAuthInternalClientCredential() error = %v", err)
	}
	if credential.CredentialID != "user-core-2026-07" {
		t.Fatalf("credential id = %q", credential.CredentialID)
	}
}

func TestLoadAuthInternalClientCredentialRejectsUnreadableFile(t *testing.T) {
	if _, err := loadAuthInternalClientCredential(authInternalCredentialEnvironment{
		Environment:    "production",
		CredentialFile: filepath.Join(t.TempDir(), "missing.json"),
	}); err == nil {
		t.Fatal("missing credential file was accepted")
	}
}
