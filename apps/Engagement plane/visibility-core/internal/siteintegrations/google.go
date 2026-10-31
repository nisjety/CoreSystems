package siteintegrations

import (
	"context"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type ConnectSession struct {
	SessionID        string `json:"sessionId"`
	AuthorizationURL string `json:"authorizationUrl"`
	ExpiresAt        string `json:"expiresAt"`
}

// BeginGoogle only prepares an explicit human consent flow. It grants nothing,
// obtains no OAuth token, and never provisions a website or provider project.
func (b *Broker) BeginGoogle(ctx context.Context, returnURL string) (ConnectSession, error) {
	trustedReturnURL, callbackURL, err := pulse.ResolveGoogleConsentURLs(b.cfg.GoogleConsentReturnURL, b.cfg.GoogleOAuthCallbackURL)
	if err != nil || returnURL != trustedReturnURL {
		return ConnectSession{}, errUnavailable
	}
	var envelope struct {
		Success bool `json:"success"`
		Data    struct {
			SessionID        string `json:"sessionToken"`
			AuthorizationURL string `json:"authorizationUrl"`
			ExpiresAt        string `json:"expiresAt"`
		} `json:"data"`
	}
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	if b.request(ctx, "POST", "/api/v1/providers/google/connect-session", map[string]any{"bundles": []string{"search_console", "analytics_readonly"}, "returnUrl": trustedReturnURL}, &envelope) != nil || !envelope.Success || !validID(envelope.Data.SessionID) || !validGoogleAuthorizationURLForCallback(envelope.Data.AuthorizationURL, callbackURL) {
		return ConnectSession{}, errUnavailable
	}
	expires, err := time.Parse(time.RFC3339, envelope.Data.ExpiresAt)
	if err != nil || !expires.After(time.Now()) || expires.After(time.Now().Add(20*time.Minute)) {
		return ConnectSession{}, errUnavailable
	}
	return ConnectSession{SessionID: envelope.Data.SessionID, AuthorizationURL: envelope.Data.AuthorizationURL, ExpiresAt: envelope.Data.ExpiresAt}, nil
}

func (b *Broker) GoogleStatus(ctx context.Context, sessionID string) (string, error) {
	if !validID(sessionID) {
		return "", errUnavailable
	}
	var envelope struct {
		Success bool                                                `json:"success"`
		Data    struct{ ID, ProviderKey, Status, ExpiresAt string } `json:"data"`
	}
	if b.request(ctx, "GET", "/api/v1/connect-sessions/"+sessionID+"/status", nil, &envelope) != nil || !envelope.Success || envelope.Data.ID != sessionID || envelope.Data.ProviderKey != "google" || !has([]string{"pending", "completed", "failed"}, envelope.Data.Status) {
		return "", errUnavailable
	}
	if envelope.Data.Status == "completed" {
		return "connected", nil
	}
	if envelope.Data.Status == "failed" {
		return "error", nil
	}
	if envelope.Data.ExpiresAt != "" {
		expires, err := time.Parse(time.RFC3339, envelope.Data.ExpiresAt)
		if err != nil {
			return "", errUnavailable
		}
		if !expires.After(time.Now()) {
			return "expired", nil
		}
	}
	return "pending", nil
}

func validGoogleAuthorizationURL(raw string) bool {
	return validGoogleAuthorizationURLForCallback(raw, pulse.DefaultGoogleOAuthCallbackURL)
}

func validGoogleAuthorizationURLForCallback(raw, callbackURL string) bool {
	if len(raw) > 8192 {
		return false
	}
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Scheme != "https" || parsed.Host != "accounts.google.com" || parsed.User != nil || parsed.Path != "/o/oauth2/v2/auth" || parsed.Fragment != "" {
		return false
	}
	query, err := url.ParseQuery(parsed.RawQuery)
	if err != nil {
		return false
	}
	for _, values := range query {
		if len(values) != 1 {
			return false
		}
	}
	if query.Get("response_type") != "code" || query.Get("redirect_uri") != callbackURL || query.Get("code_challenge_method") != "S256" || len(query.Get("state")) < 32 || len(query.Get("state")) > 256 || !pkceChallenge(query.Get("code_challenge")) || !strings.HasSuffix(query.Get("client_id"), ".apps.googleusercontent.com") {
		return false
	}
	scopes := strings.Fields(query.Get("scope"))
	allowed := []string{"openid", "email", "profile", "https://www.googleapis.com/auth/userinfo.profile", "https://www.googleapis.com/auth/userinfo.email", "https://www.googleapis.com/auth/webmasters.readonly", analyticsScope}
	for _, scope := range scopes {
		if !has(allowed, scope) {
			return false
		}
	}
	return has(scopes, analyticsScope) && has(scopes, "https://www.googleapis.com/auth/webmasters.readonly")
}
func pkceChallenge(value string) bool {
	if len(value) < 43 || len(value) > 128 {
		return false
	}
	for _, char := range value {
		if !(char >= 'A' && char <= 'Z' || char >= 'a' && char <= 'z' || char >= '0' && char <= '9' || char == '-' || char == '_') {
			return false
		}
	}
	return true
}
