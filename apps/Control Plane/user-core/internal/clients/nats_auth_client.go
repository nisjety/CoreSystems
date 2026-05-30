package clients

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/nats-io/nats.go"
)

// NatsAuthClient handles NATS-based authentication with auth service
type NatsAuthClient struct {
	nc            *nats.Conn
	serviceID     string
	serviceSecret string
	sessionToken  string
	sessionExpiry time.Time
}

// ServiceAuthResponse represents the response from service.authenticate
type ServiceAuthResponse struct {
	Authenticated bool   `json:"authenticated"`
	ServiceSecret string `json:"serviceSecret,omitempty"`
	ServiceID     string `json:"serviceId,omitempty"`
	Error         string `json:"error,omitempty"`
}

// NewNatsAuthClient creates a new NATS auth client
func NewNatsAuthClient(natsURL, serviceID, serviceSecret string) (*NatsAuthClient, error) {
	// Get NATS token from environment (for production with aquatiq root container)
	natsToken := ""
	if t := os.Getenv("NATS_TOKEN"); t != "" {
		natsToken = t
	} else if t := os.Getenv("NATS_AUTH_TOKEN"); t != "" {
		natsToken = t
	}

	// Connect with or without token
	var nc *nats.Conn
	var err error
	if natsToken != "" {
		nc, err = nats.Connect(natsURL, nats.Token(natsToken))
	} else {
		nc, err = nats.Connect(natsURL)
	}

	if err != nil {
		return nil, fmt.Errorf("failed to connect to NATS: %w", err)
	}

	return &NatsAuthClient{
		nc:            nc,
		serviceID:     serviceID,
		serviceSecret: serviceSecret,
	}, nil
}

// Authenticate requests a service account session token from auth service
func (c *NatsAuthClient) Authenticate(ctx context.Context) error {
	// Check if we have a valid token
	if c.sessionToken != "" && time.Now().Before(c.sessionExpiry) {
		return nil // Token still valid
	}

	// Request new token via NATS
	payload, err := json.Marshal(map[string]string{
		"serviceId":     c.serviceID,
		"serviceSecret": c.serviceSecret,
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

	// Store service secret (doesn't expire)
	c.sessionToken = response.ServiceSecret
	c.sessionExpiry = time.Now().Add(365 * 24 * time.Hour) // Valid for 1 year

	return nil
}

// GetServiceSecret returns the internal service secret for HTTP requests
func (c *NatsAuthClient) GetServiceSecret() (string, error) {
	if c.sessionToken == "" {
		return "", fmt.Errorf("no service secret available, call Authenticate first")
	}

	return c.sessionToken, nil
}

// Close closes the NATS connection
func (c *NatsAuthClient) Close() {
	if c.nc != nil {
		c.nc.Close()
	}
}
