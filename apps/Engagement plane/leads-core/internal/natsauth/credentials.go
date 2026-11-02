package natsauth

import (
	"errors"
	"strings"
)

type Credential struct{ User, Password string }

func Select(user, password string) (Credential, error) {
	user = strings.TrimSpace(user)
	password = strings.TrimSpace(password)
	if user == "" || len(password) < 32 {
		return Credential{}, errors.New("scoped NATS user and password of at least 32 characters are required")
	}
	return Credential{User: user, Password: password}, nil
}
