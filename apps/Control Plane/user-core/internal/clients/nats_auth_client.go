package clients

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
)

// NatsAuthClient handles NATS-based authentication with auth service
type NatsAuthClient struct {
	nc            *nats.Conn
	credential    AuthInternalClientCredential
	sessionToken  string
	sessionExpiry time.Time
}

type natsAuthCredential struct {
	User     string
	Password string
	Token    string
}

// validateScopedNatsUserPassword enforces the pair/length invariants on the
// NATS_USER/NATS_PASSWORD scoped credential.
func validateScopedNatsUserPassword(user, password string) error {
	if (user == "") != (password == "") {
		return fmt.Errorf("NATS_USER and NATS_PASSWORD must be configured together")
	}
	if user != "" && len(password) < 32 {
		return fmt.Errorf("NATS_PASSWORD must contain at least 32 characters")
	}
	return nil
}

func selectNatsAuthCredential() (natsAuthCredential, error) {
	user := strings.TrimSpace(os.Getenv("NATS_USER"))
	password := strings.TrimSpace(os.Getenv("NATS_PASSWORD"))
	if err := validateScopedNatsUserPassword(user, password); err != nil {
		return natsAuthCredential{}, err
	}
	if user != "" {
		return natsAuthCredential{User: user, Password: password}, nil
	}

	token := strings.TrimSpace(os.Getenv("NATS_TOKEN"))
	if token == "" {
		token = strings.TrimSpace(os.Getenv("NATS_AUTH_TOKEN"))
	}
	if token != "" {
		if os.Getenv("NATS_ALLOW_TOKEN_FALLBACK") != "1" {
			return natsAuthCredential{}, fmt.Errorf("NATS token authentication requires NATS_ALLOW_TOKEN_FALLBACK=1")
		}
		if len(token) < 32 {
			return natsAuthCredential{}, fmt.Errorf("NATS token must contain at least 32 characters")
		}
		return natsAuthCredential{Token: token}, nil
	}
	return natsAuthCredential{}, nil
}

// ServiceAuthResponse represents the response from service.authenticate
type ServiceAuthResponse struct {
	Authenticated bool   `json:"authenticated"`
	CredentialID  string `json:"credentialId,omitempty"`
	ServiceID     string `json:"serviceId,omitempty"`
	Error         string `json:"error,omitempty"`
}

// NewNatsAuthClient creates a new NATS auth client
func NewNatsAuthClient(natsURL string, serviceCredential AuthInternalClientCredential) (*NatsAuthClient, error) {
	credential, err := selectNatsAuthCredential()
	if err != nil {
		return nil, fmt.Errorf("configure NATS authentication: %w", err)
	}
	opts := []nats.Option{
		nats.Name("user-core-auth-client"),
		nats.CustomInboxPrefix("_INBOX.USER_CONTROL"),
	}
	if credential.User != "" {
		opts = append(opts, nats.UserInfo(credential.User, credential.Password))
	} else if credential.Token != "" {
		opts = append(opts, nats.Token(credential.Token))
	}
	nc, err := nats.Connect(natsURL, opts...)

	if err != nil {
		return nil, fmt.Errorf("failed to connect to NATS: %w", err)
	}

	return &NatsAuthClient{
		nc:         nc,
		credential: serviceCredential,
	}, nil
}

// Authenticate requests a service account session token from auth service
func (c *NatsAuthClient) Authenticate(_ context.Context) error {
	// Check if we have a valid token
	if c.sessionToken != "" && time.Now().Before(c.sessionExpiry) {
		return nil // Token still valid
	}

	// Request new token via NATS
	payload, err := json.Marshal(map[string]string{
		"credentialId":  c.credential.CredentialID,
		"serviceId":     c.credential.Principal,
		"serviceSecret": c.credential.Token,
	})
	if err != nil {
		return fmt.Errorf("failed to marshal auth request: %w", err)
	}

	msg, err := c.nc.Request("service.authenticate", payload, 5*time.Second)
	if err != nil {
		return fmt.Errorf("NATS service.authenticate failed: %w", err)
	}

	var response ServiceAuthResponse
	if err := json.Unmarshal(msg.Data, &response); err != nil {
		return fmt.Errorf("failed to unmarshal auth response: %w", err)
	}

	if !response.Authenticated {
		return fmt.Errorf("service authentication failed: %s", response.Error)
	}
	if response.CredentialID != c.credential.CredentialID || response.ServiceID != c.credential.Principal {
		return fmt.Errorf("service authentication returned a mismatched principal")
	}

	// Cache only the fact that Auth verified the exact tuple. The credential is
	// never returned or copied through the NATS response.
	c.sessionToken = c.credential.Token
	c.sessionExpiry = time.Now().Add(365 * 24 * time.Hour) // Valid for 1 year

	return nil
}

// GetServiceCredential returns the already deployment-owned credential only
// after Auth has verified the exact tuple over NATS.
func (c *NatsAuthClient) GetServiceCredential() (AuthInternalClientCredential, error) {
	if c.sessionToken == "" {
		return AuthInternalClientCredential{}, fmt.Errorf("no verified service credential available, call Authenticate first")
	}
	return c.credential, nil
}

// Close closes the NATS connection
func (c *NatsAuthClient) Close() {
	if c.nc != nil {
		c.nc.Close()
	}
}
