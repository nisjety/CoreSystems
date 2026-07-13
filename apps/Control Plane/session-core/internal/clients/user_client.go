package clients

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

func signUserCoreDelegation(req *http.Request, token, userID string, now time.Time) {
	timestamp := now.UTC().Format(time.RFC3339)
	bodyDigestBytes := sha256.Sum256(nil)
	bodyDigest := base64.RawURLEncoding.EncodeToString(bodyDigestBytes[:])
	canonical := strings.Join([]string{
		"v1",
		"session-core",
		"user-core",
		timestamp,
		req.Method,
		req.URL.RequestURI(),
		strings.TrimSpace(userID),
		"",
		"",
		"",
		"",
		bodyDigest,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(token))
	_, _ = mac.Write([]byte(canonical))
	req.Header.Set("X-Delegation-Timestamp", timestamp)
	req.Header.Set("X-Delegation-Body-SHA256", bodyDigest)
	req.Header.Set("X-Delegation-Signature", base64.RawURLEncoding.EncodeToString(mac.Sum(nil)))
}

// UserSessionContext mirrors user-core's GET /api/v1/me/session-context response.
type UserSessionContext struct {
	UserID           string `json:"userId"`
	OrgID            string `json:"orgId,omitempty"`
	Role             string `json:"role,omitempty"`
	OnboardingStatus string `json:"onboardingStatus"`
}

// UserProfile mirrors user-core's GET /api/v1/users/me response (subset).
type UserProfile struct {
	ID                 string `json:"id"`
	Email              string `json:"email"`
	Name               string `json:"name,omitempty"`
	Image              string `json:"image,omitempty"`
	OnboardingComplete bool   `json:"onboardingComplete,omitempty"`
}

type userProfilePayload struct {
	ID                      string `json:"id"`
	Email                   string `json:"email"`
	Name                    string `json:"name,omitempty"`
	DisplayName             string `json:"display_name,omitempty"`
	Image                   string `json:"image,omitempty"`
	Avatar                  string `json:"avatar,omitempty"`
	OnboardingCompleteSnake bool   `json:"onboarding_complete,omitempty"`
	OnboardingCompleteCamel bool   `json:"onboardingComplete,omitempty"`
}

func (p userProfilePayload) toUserProfile() *UserProfile {
	name := p.Name
	if name == "" {
		name = p.DisplayName
	}

	image := p.Image
	if image == "" {
		image = p.Avatar
	}

	return &UserProfile{
		ID:                 p.ID,
		Email:              p.Email,
		Name:               name,
		Image:              image,
		OnboardingComplete: p.OnboardingCompleteSnake || p.OnboardingCompleteCamel,
	}
}

type userProfileResponse struct {
	User *userProfilePayload `json:"user,omitempty"`
	userProfilePayload
}

// UserClient calls user-core to fetch routing context and full profile.
// G10: used by the Control Session aggregator.
type UserClient struct {
	baseURL      string
	serviceToken string
	httpClient   *http.Client
}

// NewUserClient returns nil when baseURL is empty (disables the call site).
func NewUserClient(baseURL, serviceToken string) *UserClient {
	if baseURL == "" {
		return nil
	}
	return &UserClient{
		baseURL:      baseURL,
		serviceToken: serviceToken,
		httpClient:   &http.Client{Timeout: 5 * time.Second},
	}
}

func (c *UserClient) authedRequest(ctx context.Context, method, path, userID string) (*http.Request, error) {
	url := c.baseURL + path
	req, err := http.NewRequestWithContext(ctx, method, url, nil)
	if err != nil {
		return nil, err
	}
	if c.serviceToken != "" {
		req.Header.Set("X-Service-Token", c.serviceToken)
		req.Header.Set("X-Service-Id", "session-core")
	}
	if userID != "" {
		req.Header.Set("X-User-Id", userID)
		if c.serviceToken != "" {
			signUserCoreDelegation(req, c.serviceToken, userID, time.Now())
		}
	}
	req.Header.Set("Accept", "application/json")
	return req, nil
}

// GetSessionContext returns user-core's post-login routing context for userID.
func (c *UserClient) GetSessionContext(ctx context.Context, userID string) (*UserSessionContext, error) {
	req, err := c.authedRequest(ctx, http.MethodGet, "/api/v1/me/session-context", userID)
	if err != nil {
		return nil, fmt.Errorf("user-client: build request: %w", err)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("user-client: call user-core session-context: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("user-client: session-context returned %d", resp.StatusCode)
	}
	var out UserSessionContext
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, fmt.Errorf("user-client: decode session-context: %w", err)
	}
	return &out, nil
}

// GetProfile returns user-core's profile for userID.
func (c *UserClient) GetProfile(ctx context.Context, userID string) (*UserProfile, error) {
	req, err := c.authedRequest(ctx, http.MethodGet, "/api/v1/users/me", userID)
	if err != nil {
		return nil, fmt.Errorf("user-client: build request: %w", err)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("user-client: call user-core users/me: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("user-client: users/me returned %d", resp.StatusCode)
	}
	var out userProfileResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, fmt.Errorf("user-client: decode users/me: %w", err)
	}
	if out.User != nil {
		return out.User.toUserProfile(), nil
	}
	return out.userProfilePayload.toUserProfile(), nil
}
