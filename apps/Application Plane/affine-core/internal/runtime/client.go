package runtime

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

type Config struct {
	BaseURL       string
	AdminEmail    string
	AdminPassword string
}

type Client struct {
	baseURL       string
	adminEmail    string
	adminPassword string
	httpClient    *http.Client
}

type Session struct {
	UserID         string
	UserEmail      string
	RuntimeCookies []string
}

func NewClient(cfg Config) *Client {
	return &Client{
		baseURL:       strings.TrimRight(cfg.BaseURL, "/"),
		adminEmail:    cfg.AdminEmail,
		adminPassword: cfg.AdminPassword,
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

func (c *Client) Ready() bool {
	return c.baseURL != "" && c.adminPassword != ""
}

func (c *Client) BaseURL() string {
	return c.baseURL
}

func (c *Client) ExchangeAdminSession(ctx context.Context) (*Session, error) {
	if !c.Ready() {
		return nil, errors.New("affine runtime not configured")
	}

	body, _ := json.Marshal(map[string]string{
		"email":    c.adminEmail,
		"password": c.adminPassword,
	})

	request, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/api/auth/sign-in", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	request.Header.Set("Content-Type", "application/json")

	response, err := c.httpClient.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()

	if response.StatusCode >= http.StatusBadRequest {
		return nil, fmt.Errorf("affine runtime sign-in failed: %s", response.Status)
	}

	var payload struct {
		ID    string `json:"id"`
		Email string `json:"email"`
	}
	if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
		return nil, err
	}

	return &Session{
		UserID:         payload.ID,
		UserEmail:      payload.Email,
		RuntimeCookies: response.Header.Values("Set-Cookie"),
	}, nil
}
