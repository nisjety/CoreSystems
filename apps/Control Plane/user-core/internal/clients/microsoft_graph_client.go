// Package clients — Microsoft Graph identity client.
//
// G41 (velion-gap.md §8.30 / Slice D): when a Microsoft OAuth account is
// linked to a user, this client fetches the user's Graph profile + photo
// so user-core can soft-update the local row with displayName, jobTitle,
// mail, office location, preferred language, mobile phone, and avatar.
//
// Scope is intentionally narrow: only the two endpoints the zero-input
// enterprise onboarding roadmap calls out — `GET /v1.0/me` and
// `GET /v1.0/me/photo/$value`. Everything else is out of scope.
package clients

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// Default Graph base URL. Override via `MICROSOFT_GRAPH_BASE_URL` for tenants
// pinned to a sovereign cloud (Azure Government, Azure China) — never
// hard-code those URLs into source.
const defaultGraphBaseURL = "https://graph.microsoft.com/v1.0"

// MicrosoftGraphClient holds the base URL + HTTP transport. The access
// token is passed per-call (one user → one token), not held on the client.
type MicrosoftGraphClient struct {
	baseURL    string
	httpClient *http.Client
}

// NewMicrosoftGraphClient returns a client pinned to `baseURL` (defaults
// to `https://graph.microsoft.com/v1.0` when empty). Always returns a
// non-nil client because the network surface is Microsoft-owned — no
// reason to make the dependency optional like the auth-core client.
func NewMicrosoftGraphClient(baseURL string) *MicrosoftGraphClient {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" {
		baseURL = defaultGraphBaseURL
	}
	return &MicrosoftGraphClient{
		baseURL: baseURL,
		httpClient: &http.Client{
			// 8s covers a cold Graph response (rare slow path observed in
			// the wild). The handler swallows errors so a Graph timeout
			// degrades to a Better-Auth-hint-only update, not a fatal.
			Timeout: 8 * time.Second,
		},
	}
}

// GraphProfile is a minimal projection of `GET /v1.0/me`.
// Field names match Graph's casing so JSON-unmarshalling is direct.
//
// Reference: https://learn.microsoft.com/en-us/graph/api/user-get
type GraphProfile struct {
	ID                string   `json:"id"`
	DisplayName       string   `json:"displayName"`
	GivenName         string   `json:"givenName"`
	Surname           string   `json:"surname"`
	UserPrincipalName string   `json:"userPrincipalName"`
	Mail              string   `json:"mail"`
	JobTitle          string   `json:"jobTitle"`
	OfficeLocation    string   `json:"officeLocation"`
	Department        string   `json:"department"`
	PreferredLanguage string   `json:"preferredLanguage"`
	MobilePhone       string   `json:"mobilePhone"`
	BusinessPhones    []string `json:"businessPhones"`
}

// GetMe calls `GET /v1.0/me` with `accessToken`. Returns the profile on 200,
// an error otherwise. 401/403 from Graph surfaces as a clear "access token
// rejected" error so the caller can decide to skip enrichment without
// retrying.
func (c *MicrosoftGraphClient) GetMe(ctx context.Context, accessToken string) (*GraphProfile, error) {
	accessToken = strings.TrimSpace(accessToken)
	if accessToken == "" {
		return nil, errors.New("access token is required")
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.baseURL+"/me", nil)
	if err != nil {
		return nil, fmt.Errorf("build /me request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call graph /me: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read graph /me response: %w", err)
	}

	if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
		return nil, fmt.Errorf("graph rejected access token (%d): %s", resp.StatusCode, truncate(string(body), 200))
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("graph /me %d: %s", resp.StatusCode, truncate(string(body), 200))
	}

	var profile GraphProfile
	if err := json.Unmarshal(body, &profile); err != nil {
		return nil, fmt.Errorf("decode graph /me response: %w", err)
	}
	return &profile, nil
}

// GraphPhoto holds the bytes + MIME type of a user's photo.
type GraphPhoto struct {
	ContentType string
	Bytes       []byte
}

// DataURL renders the photo as an inline `data:` URL suitable for storing in
// `user.avatar` without extra blob storage. Returns "" when the photo is
// missing — saves callers a nil check.
func (p *GraphPhoto) DataURL() string {
	if p == nil || len(p.Bytes) == 0 {
		return ""
	}
	contentType := strings.TrimSpace(p.ContentType)
	if contentType == "" {
		contentType = "image/jpeg" // Graph default
	}
	return "data:" + contentType + ";base64," + base64.StdEncoding.EncodeToString(p.Bytes)
}

// GetPhotoValue calls `GET /v1.0/me/photo/$value`. Returns nil photo + nil
// error when the user has no photo (404) — that is not an error condition.
// All other non-2xx responses are returned as errors.
func (c *MicrosoftGraphClient) GetPhotoValue(ctx context.Context, accessToken string) (*GraphPhoto, error) {
	accessToken = strings.TrimSpace(accessToken)
	if accessToken == "" {
		return nil, errors.New("access token is required")
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.baseURL+"/me/photo/$value", nil)
	if err != nil {
		return nil, fmt.Errorf("build /me/photo request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call graph /me/photo: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		// User has no photo set in Entra. Common; not an error.
		return nil, nil
	}
	if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 1024))
		return nil, fmt.Errorf("graph rejected access token (%d): %s", resp.StatusCode, truncate(string(body), 200))
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 1024))
		return nil, fmt.Errorf("graph /me/photo %d: %s", resp.StatusCode, truncate(string(body), 200))
	}

	// Cap at 5 MiB. Real Entra photos are ~50 KiB; a 5 MiB cap is a generous
	// guard against a misbehaving upstream filling memory with a giant blob.
	const maxPhotoBytes = 5 * 1024 * 1024
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxPhotoBytes+1))
	if err != nil {
		return nil, fmt.Errorf("read graph /me/photo response: %w", err)
	}
	if len(body) > maxPhotoBytes {
		return nil, fmt.Errorf("graph /me/photo response > %d bytes", maxPhotoBytes)
	}

	contentType := strings.TrimSpace(resp.Header.Get("Content-Type"))
	return &GraphPhoto{ContentType: contentType, Bytes: body}, nil
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
