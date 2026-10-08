package actions

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"

	"github.com/triodelab/integration-corev2/internal/providers"
)

// ProviderAuthError reports that a provider refused a call because of the
// connection's credential rather than the request itself: HTTP 401 (the token
// is no longer accepted) or 403 (the token does not carry the scope the call
// needs, or the account may not do this). Reconnecting is the one remedy the
// platform can offer, so callers surface it as "reconnect required" instead of
// a generic provider failure.
//
// It carries the provider's short error code when the response had one, never
// the provider's message, which can echo request content.
type ProviderAuthError struct {
	Status int
	Code   string
}

func (e *ProviderAuthError) Error() string {
	return fmt.Sprintf("provider endpoint returned status %d (credential or scope rejected)", e.Status)
}

// AsProviderAuthError unwraps a ProviderAuthError from err.
func AsProviderAuthError(err error) (*ProviderAuthError, bool) {
	var rejection *ProviderAuthError
	if errors.As(err, &rejection) {
		return rejection, true
	}
	return nil, false
}

// reconnectableError keeps the reconnect classification only for providers that
// have a reconnect flow. A provider whose credential is server configuration
// (Okta's API token) gets the plain failure back: telling a user to reconnect
// something they cannot reconnect would be wrong.
func reconnectableError(providerKey string, err error) error {
	rejection, ok := AsProviderAuthError(err)
	if !ok {
		return err
	}
	if _, oauthProvider := providers.FindOAuth(providerKey); oauthProvider {
		return err
	}
	return fmt.Errorf("provider action endpoint returned status %d", rejection.Status)
}

// maxProviderErrorBodyBytes bounds how much of a refusal's body is read just to
// classify it.
const maxProviderErrorBodyBytes = 8 << 10

// providerAuthRejection returns a *ProviderAuthError when resp is a 401, or a
// 403 that is about the credential. A 403 that is really throttling or quota
// exhaustion (Google reports those as 403, GitHub sets Retry-After or an
// exhausted X-RateLimit-Remaining) returns nil: reconnecting would not help and
// saying so would mislead.
func providerAuthRejection(resp *http.Response, body []byte) *ProviderAuthError {
	switch resp.StatusCode {
	case http.StatusUnauthorized:
	case http.StatusForbidden:
		if providerThrottled(resp, body) {
			return nil
		}
	default:
		return nil
	}
	return &ProviderAuthError{Status: resp.StatusCode, Code: providerErrorCode(body)}
}

func providerThrottled(resp *http.Response, body []byte) bool {
	if strings.TrimSpace(resp.Header.Get("Retry-After")) != "" {
		return true
	}
	if strings.TrimSpace(resp.Header.Get("X-RateLimit-Remaining")) == "0" {
		return true
	}
	for _, token := range providerErrorTokens(body) {
		lowered := strings.ToLower(token)
		for _, marker := range []string{"ratelimit", "rate_limit", "quota", "resource_exhausted", "dailylimit", "toomanyrequests", "throttl", "activitylimit"} {
			if strings.Contains(lowered, marker) {
				return true
			}
		}
	}
	return false
}

// providerErrorTokens collects the machine-readable strings of a Google, Graph
// or flat error body: error.code, error.status, error.errors[].reason,
// error.details[].reason, and a bare "error" string.
func providerErrorTokens(body []byte) []string {
	var decoded struct {
		Error json.RawMessage `json:"error"`
	}
	if err := json.Unmarshal(body, &decoded); err != nil || len(decoded.Error) == 0 {
		return nil
	}
	var flat string
	if err := json.Unmarshal(decoded.Error, &flat); err == nil {
		return []string{flat}
	}
	var object struct {
		Code   any    `json:"code"`
		Status string `json:"status"`
		Errors []struct {
			Reason string `json:"reason"`
		} `json:"errors"`
		Details []struct {
			Reason string `json:"reason"`
		} `json:"details"`
	}
	if err := json.Unmarshal(decoded.Error, &object); err != nil {
		return nil
	}
	var tokens []string
	if code, ok := object.Code.(string); ok {
		tokens = append(tokens, code)
	}
	tokens = append(tokens, object.Status)
	for _, item := range object.Errors {
		tokens = append(tokens, item.Reason)
	}
	for _, item := range object.Details {
		tokens = append(tokens, item.Reason)
	}
	out := tokens[:0]
	for _, token := range tokens {
		if strings.TrimSpace(token) != "" {
			out = append(out, token)
		}
	}
	return out
}

// providerErrorCode is the first short machine-readable code in the body,
// reduced to a safe charset and length so it can be logged and returned.
func providerErrorCode(body []byte) string {
	tokens := providerErrorTokens(body)
	if len(tokens) == 0 {
		return ""
	}
	var code strings.Builder
	for _, r := range tokens[0] {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9', r == '_', r == '.', r == '-', r == ':':
			code.WriteRune(r)
		}
		if code.Len() >= 64 {
			break
		}
	}
	return code.String()
}
