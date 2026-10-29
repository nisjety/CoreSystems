package bearerrefresh

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"strings"
	"time"
)

// This parses the trusted issuance response only to enforce its deployment
// binding and renewal deadline. It is not a substitute for the receiver's JWKS
// signature verification at the receiving boundary.
func tokenExpiry(token, org, serviceID string, now time.Time) (time.Time, string, error) {
	profile, _ := profileFor(defaultCapability)
	return tokenExpiryForProfile(token, org, serviceID, now, profile)
}

func tokenExpiryForProfile(token, org, serviceID string, now time.Time, profile capabilityProfile) (time.Time, string, error) {
	invalid := errors.New("service bearer is expired, malformed, or has the wrong binding")
	parts := strings.Split(token, ".")
	if len(token) > 8192 || len(parts) != 3 || parts[2] == "" || strings.ContainsAny(token, " \r\n\t") {
		return time.Time{}, "", invalid
	}
	headerJSON, err := base64.RawURLEncoding.DecodeString(parts[0])
	if err != nil {
		return time.Time{}, "", invalid
	}
	var header struct {
		Algorithm string `json:"alg"`
		KeyID     string `json:"kid"`
	}
	if json.Unmarshal(headerJSON, &header) != nil || header.Algorithm != "RS256" || header.KeyID == "" {
		return time.Time{}, "", invalid
	}
	claimsJSON, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return time.Time{}, "", invalid
	}
	var claims struct {
		Issuer         string          `json:"iss"`
		Audience       string          `json:"aud"`
		Subject        string          `json:"sub"`
		ServiceID      string          `json:"service_id"`
		PrincipalType  string          `json:"principal_type"`
		OrganizationID string          `json:"org_id"`
		UserID         json.RawMessage `json:"user_id"`
		Scopes         []string        `json:"scopes"`
		Expiry         int64           `json:"exp"`
		IssuedAt       int64           `json:"iat"`
		NotBefore      int64           `json:"nbf"`
	}
	if json.Unmarshal(claimsJSON, &claims) != nil || claims.Issuer == "" || claims.Audience != profile.audience || claims.Subject != "service:"+serviceID || claims.ServiceID != claims.Subject || claims.PrincipalType != "service" || claims.OrganizationID != org || len(claims.UserID) != 0 || !exactScopes(claims.Scopes, profile.scopes) || claims.IssuedAt <= 0 || claims.IssuedAt < now.Add(-time.Hour).Unix() || claims.IssuedAt > now.Add(30*time.Second).Unix() || claims.NotBefore <= 0 || claims.NotBefore > now.Unix() || claims.Expiry <= now.Unix() || claims.Expiry <= claims.IssuedAt || claims.Expiry > now.Add(time.Hour).Unix() {
		return time.Time{}, "", invalid
	}
	return time.Unix(claims.Expiry, 0).UTC(), claims.Issuer, nil
}
