package controlplane

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"
)

var ErrUnauthenticated = errors.New("authentication required")

type Config struct {
	AuthServiceURL string
	UserServiceURL string
	InternalAPIKey string
	HTTPClient     *http.Client
}

type Client struct {
	authServiceURL string
	userServiceURL string
	internalAPIKey string
	httpClient     *http.Client
}

type ActorContext struct {
	UserID    string
	UserEmail string
	UserName  string
	OrgID     string
	Role      string
	SessionID string
}

type authSessionUser struct {
	ID    string `json:"id"`
	Email string `json:"email"`
	Name  string `json:"name"`
}

type authSession struct {
	ID string `json:"id"`
}

type authSessionPayload struct {
	User    *authSessionUser `json:"user"`
	Session *authSession     `json:"session"`
}

type authSessionResponse struct {
	Authenticated *bool               `json:"authenticated"`
	Data          *authSessionPayload `json:"data"`
	User          *authSessionUser    `json:"user"`
	Session       *authSession        `json:"session"`
}

type sessionContextResponse struct {
	UserID string `json:"userId"`
	OrgID  string `json:"orgId"`
	Role   string `json:"role"`
}

func NewClient(cfg Config) *Client {
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 10 * time.Second}
	}

	return &Client{
		authServiceURL: strings.TrimRight(cfg.AuthServiceURL, "/"),
		userServiceURL: strings.TrimRight(cfg.UserServiceURL, "/"),
		internalAPIKey: cfg.InternalAPIKey,
		httpClient:     httpClient,
	}
}

func (c *Client) ResolveActorContext(ctx context.Context, cookieHeader string) (*ActorContext, error) {
	if strings.TrimSpace(cookieHeader) == "" {
		return nil, ErrUnauthenticated
	}

	session, err := c.fetchSession(ctx, cookieHeader)
	if err != nil {
		return nil, err
	}

	sessionContext, err := c.fetchSessionContext(ctx, session)
	if err != nil {
		return nil, err
	}

	return &ActorContext{
		UserID:    session.User.ID,
		UserEmail: session.User.Email,
		UserName:  session.User.Name,
		OrgID:     sessionContext.OrgID,
		Role:      sessionContext.Role,
		SessionID: session.Session.ID,
	}, nil
}

func (c *Client) fetchSession(ctx context.Context, cookieHeader string) (*authSessionPayload, error) {
	requestBody, _ := json.Marshal(map[string]any{})
	request, err := http.NewRequestWithContext(
		ctx,
		http.MethodPost,
		c.authServiceURL+"/api/v2/auth/getSession",
		bytes.NewReader(requestBody),
	)
	if err != nil {
		return nil, err
	}

	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Cookie", cookieHeader)
	request.Header.Set("X-Internal-Api-Key", c.internalAPIKey)

	response, err := c.httpClient.Do(request)
	if err != nil {
		return nil, fmt.Errorf("auth session lookup failed: %w", err)
	}
	defer response.Body.Close()

	if response.StatusCode == http.StatusUnauthorized || response.StatusCode == http.StatusForbidden {
		return nil, ErrUnauthenticated
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return nil, fmt.Errorf("auth session lookup failed: status %d", response.StatusCode)
	}

	var payload authSessionResponse
	if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
		return nil, fmt.Errorf("decode auth session response: %w", err)
	}

	if payload.Authenticated != nil && !*payload.Authenticated {
		return nil, ErrUnauthenticated
	}

	session := payload.Data
	if session == nil && payload.User != nil {
		session = &authSessionPayload{
			User:    payload.User,
			Session: payload.Session,
		}
	}

	if session == nil || session.User == nil || strings.TrimSpace(session.User.ID) == "" {
		return nil, ErrUnauthenticated
	}

	if session.Session == nil {
		session.Session = &authSession{}
	}

	return session, nil
}

func (c *Client) fetchSessionContext(ctx context.Context, session *authSessionPayload) (*sessionContextResponse, error) {
	request, err := http.NewRequestWithContext(
		ctx,
		http.MethodGet,
		c.userServiceURL+"/api/v1/me/session-context",
		nil,
	)
	if err != nil {
		return nil, err
	}

	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Internal-Api-Key", c.internalAPIKey)
	request.Header.Set("X-User-Id", session.User.ID)
	request.Header.Set("X-User-Email", session.User.Email)
	request.Header.Set("X-User-Name", fallback(session.User.Name, session.User.Email, "User"))

	response, err := c.httpClient.Do(request)
	if err != nil {
		return nil, fmt.Errorf("session context lookup failed: %w", err)
	}
	defer response.Body.Close()

	if response.StatusCode == http.StatusUnauthorized || response.StatusCode == http.StatusForbidden {
		return nil, ErrUnauthenticated
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return nil, fmt.Errorf("session context lookup failed: status %d", response.StatusCode)
	}

	var payload sessionContextResponse
	if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
		return nil, fmt.Errorf("decode session context response: %w", err)
	}

	return &payload, nil
}

func fallback(values ...string) string {
	for _, value := range values {
		trimmed := strings.TrimSpace(value)
		if trimmed != "" {
			return trimmed
		}
	}
	return ""
}
