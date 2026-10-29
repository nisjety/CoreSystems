package nats

import (
	"errors"
	"strings"

	gonats "github.com/nats-io/nats.go"
)

type runtimeCredential struct {
	User     string
	Password string
	Token    string
}

type Credentials struct {
	User               string
	Password           string
	Token              string
	AllowTokenFallback bool
	InboxPrefix        string
}

func selectRuntimeCredential(credentials Credentials) (runtimeCredential, error) {
	user := strings.TrimSpace(credentials.User)
	password := strings.TrimSpace(credentials.Password)
	if (user == "") != (password == "") {
		return runtimeCredential{}, errors.New("NATS_USER and NATS_PASSWORD must be configured together")
	}
	if user != "" {
		if len(password) < 32 {
			return runtimeCredential{}, errors.New("NATS_PASSWORD must contain at least 32 characters")
		}
		return runtimeCredential{User: user, Password: password}, nil
	}
	token := strings.TrimSpace(credentials.Token)
	if token != "" {
		if !credentials.AllowTokenFallback {
			return runtimeCredential{}, errors.New("NATS token authentication requires NATS_ALLOW_TOKEN_FALLBACK=1")
		}
		return runtimeCredential{Token: token}, nil
	}
	return runtimeCredential{}, nil
}

func runtimeAuthOptions(credentials Credentials) ([]gonats.Option, error) {
	credential, err := selectRuntimeCredential(credentials)
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
