package clients

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"strings"
)

const (
	authInternalAudience          = "auth-core-internal"
	maximumAuthCredentialFileSize = 64 * 1024
)

var authCredentialIdentifier = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{1,127}$`)

// AuthInternalClientCredential is User Core's deployment-owned identity for
// narrowly scoped Auth Core internal contracts.
type AuthInternalClientCredential struct {
	CredentialID string `json:"credentialId"`
	Principal    string `json:"principal"`
	Audience     string `json:"audience"`
	Token        string `json:"token"`
}

type authInternalCredentialEnvironment struct {
	Environment      string
	InlineCredential string
	CredentialFile   string
}

func configuredAuthInternalCredentialEnvironment() authInternalCredentialEnvironment {
	return authInternalCredentialEnvironment{
		Environment:      strings.TrimSpace(os.Getenv("ENVIRONMENT")),
		InlineCredential: os.Getenv("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL"),
		CredentialFile:   strings.TrimSpace(os.Getenv("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE")),
	}
}

func validAuthInternalToken(value string) bool {
	if value != strings.TrimSpace(value) || len(value) < 32 {
		return false
	}
	lower := strings.ToLower(value)
	for _, prefix := range []string{"test", "placeholder", "change-me", "replace-with", "your-"} {
		if strings.HasPrefix(lower, prefix) {
			return false
		}
	}
	return true
}

func parseAuthInternalClientCredential(raw string) (AuthInternalClientCredential, error) {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal([]byte(strings.TrimSpace(raw)), &fields); err != nil {
		return AuthInternalClientCredential{}, fmt.Errorf("decode USER_AUTH_INTERNAL_CLIENT_CREDENTIAL: %w", err)
	}
	if len(fields) != 4 {
		return AuthInternalClientCredential{}, fmt.Errorf("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL contains invalid fields")
	}
	for _, name := range []string{"credentialId", "principal", "audience", "token"} {
		if _, ok := fields[name]; !ok {
			return AuthInternalClientCredential{}, fmt.Errorf("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL contains invalid fields")
		}
	}

	var credential AuthInternalClientCredential
	if err := json.Unmarshal([]byte(raw), &credential); err != nil {
		return AuthInternalClientCredential{}, fmt.Errorf("decode USER_AUTH_INTERNAL_CLIENT_CREDENTIAL: %w", err)
	}
	if credential.CredentialID != strings.TrimSpace(credential.CredentialID) ||
		!authCredentialIdentifier.MatchString(credential.CredentialID) ||
		credential.Principal != "user-core" ||
		credential.Audience != authInternalAudience ||
		!validAuthInternalToken(credential.Token) {
		return AuthInternalClientCredential{}, fmt.Errorf("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL contains an invalid principal policy")
	}
	return credential, nil
}

func loadAuthInternalClientCredential(environment authInternalCredentialEnvironment) (AuthInternalClientCredential, error) {
	if environment.CredentialFile != "" {
		info, err := os.Stat(environment.CredentialFile)
		if err != nil {
			return AuthInternalClientCredential{}, fmt.Errorf("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE could not be read")
		}
		if info.Size() > maximumAuthCredentialFileSize {
			return AuthInternalClientCredential{}, fmt.Errorf("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE is too large")
		}
		raw, err := os.ReadFile(environment.CredentialFile)
		if err != nil {
			return AuthInternalClientCredential{}, fmt.Errorf("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE could not be read")
		}
		return parseAuthInternalClientCredential(string(raw))
	}
	if strings.EqualFold(strings.TrimSpace(environment.Environment), "production") {
		return AuthInternalClientCredential{}, fmt.Errorf("USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE is required in production")
	}
	return parseAuthInternalClientCredential(environment.InlineCredential)
}

// LoadAuthInternalClientCredential validates User Core's Auth identity before
// it opens listeners or connects to internal Auth contracts.
func LoadAuthInternalClientCredential() (AuthInternalClientCredential, error) {
	return loadAuthInternalClientCredential(configuredAuthInternalCredentialEnvironment())
}
