package nats

import (
	"context"
	"encoding/json"

	natsclient "github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

// AuthValidator validates sessions via the existing session.validate NATS subject
// on the local controlplane-nats. This reuses the contract auth-core already serves.
type AuthValidator struct {
	client  *Client
	subject string
}

type ValidateRequest struct {
	Cookies string `json:"cookies"`
}

type ValidateResponse struct {
	Valid  bool   `json:"valid"`
	UserID string `json:"userId"`
	Email  string `json:"email"`
	Name   string `json:"name"`
	OrgID  string `json:"orgId,omitempty"`
}

func NewAuthValidator(client *Client, subject string) *AuthValidator {
	return &AuthValidator{
		client:  client,
		subject: subject,
	}
}

// ValidateSession sends a session.validate request to auth-core and returns the user info.
func (v *AuthValidator) ValidateSession(ctx context.Context, cookies string) (*ValidateResponse, error) {
	data, err := json.Marshal(ValidateRequest{Cookies: cookies})
	if err != nil {
		return nil, err
	}
	msg, err := v.client.Request(v.subject, data, 5e9) // 5s timeout
	if err != nil {
		return nil, err
	}
	return parseValidateResponse(msg)
}

func parseValidateResponse(msg *natsclient.Msg) (*ValidateResponse, error) {
	var resp ValidateResponse
	if err := json.Unmarshal(msg.Data, &resp); err != nil {
		log.Error().Err(err).Msg("Failed to parse session.validate response")
		return nil, err
	}
	return &resp, nil
}
