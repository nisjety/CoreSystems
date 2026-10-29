package nats

import (
	"errors"
	"strings"
)

type runtimeCredential struct{ User, Password string }

func selectRuntimeCredential(user, password string) (runtimeCredential, error) {
	user = strings.TrimSpace(user)
	password = strings.TrimSpace(password)
	if user == "" || len(password) < 32 {
		return runtimeCredential{}, errors.New("scoped NATS user and password of at least 32 characters are required")
	}
	return runtimeCredential{User: user, Password: password}, nil
}
