package nats

import (
	"errors"
	"fmt"
	"os"
	"strings"

	gonats "github.com/nats-io/nats.go"
)

type runtimeCredential struct {
	User     string
	Password string
	Token    string
}

// validateScopedUserPassword enforces the pair/length invariants shared by
// runtime and shared-publisher credential selection. pairName and
// passwordName label the credential in the returned error messages.
func validateScopedUserPassword(user, password, pairName, passwordName string) error {
	if (user == "") != (password == "") {
		return fmt.Errorf("%s must be configured together", pairName)
	}
	if user != "" && len(password) < 32 {
		return fmt.Errorf("%s must contain at least 32 characters", passwordName)
	}
	return nil
}

func selectRuntimeCredential(configToken string) (runtimeCredential, error) {
	user := strings.TrimSpace(os.Getenv("NATS_USER"))
	password := strings.TrimSpace(os.Getenv("NATS_PASSWORD"))
	if err := validateScopedUserPassword(user, password, "NATS_USER and NATS_PASSWORD", "NATS_PASSWORD"); err != nil {
		return runtimeCredential{}, err
	}
	if user != "" {
		return runtimeCredential{User: user, Password: password}, nil
	}
	token := strings.TrimSpace(configToken)
	if token != "" {
		if os.Getenv("NATS_ALLOW_TOKEN_FALLBACK") != "1" {
			return runtimeCredential{}, errors.New("NATS token authentication requires NATS_ALLOW_TOKEN_FALLBACK=1")
		}
		return runtimeCredential{Token: token}, nil
	}
	return runtimeCredential{}, nil
}

func runtimeAuthOptions(configToken string) ([]gonats.Option, error) {
	credential, err := selectRuntimeCredential(configToken)
	if err != nil {
		return nil, err
	}
	if credential.User != "" {
		return []gonats.Option{gonats.UserInfo(credential.User, credential.Password)}, nil
	}
	if credential.Token != "" {
		return []gonats.Option{gonats.Token(credential.Token)}, nil
	}
	return nil, nil
}
