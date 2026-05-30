package channel

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"
)

// Claims holds the decoded JWT payload fields that bridge-core cares about.
type Claims struct {
	OrgID     string    `json:"org_id"`
	UserID    string    `json:"user_id"`
	SessionID string    `json:"session_id"`
	ExpiresAt time.Time `json:"exp"`
}

// jwtPayload is the raw JSON structure inside the JWT payload segment.
type jwtPayload struct {
	OrgID     string `json:"org_id"`
	UserID    string `json:"user_id"`
	SessionID string `json:"session_id"`
	Exp       int64  `json:"exp"`
}

// JWTValidator validates HMAC-SHA256 signed JWTs using a secret loaded from
// the BRIDGE_JWT_SECRET environment variable.
type JWTValidator struct {
	secret []byte
}

// NewJWTValidator creates a validator. If secret is nil the validator reads
// the secret from the BRIDGE_JWT_SECRET environment variable at construction
// time. Returns an error if no secret is available.
func NewJWTValidator(secret []byte) (*JWTValidator, error) {
	if len(secret) == 0 {
		envSecret := os.Getenv("BRIDGE_JWT_SECRET")
		if envSecret == "" {
			return nil, fmt.Errorf("BRIDGE_JWT_SECRET is not set")
		}
		secret = []byte(envSecret)
	}
	return &JWTValidator{secret: secret}, nil
}

// Validate parses and verifies a JWT token string. It checks the HMAC-SHA256
// signature and expiration. Returns the decoded claims on success.
func (v *JWTValidator) Validate(tokenString string) (Claims, error) {
	parts := strings.Split(tokenString, ".")
	if len(parts) != 3 {
		return Claims{}, fmt.Errorf("invalid token: expected 3 segments, got %d", len(parts))
	}

	headerB64, payloadB64, signatureB64 := parts[0], parts[1], parts[2]

	// Verify signature: HMAC-SHA256(header.payload)
	signingInput := headerB64 + "." + payloadB64
	mac := hmac.New(sha256.New, v.secret)
	mac.Write([]byte(signingInput))
	expectedSig := mac.Sum(nil)

	actualSig, err := base64.RawURLEncoding.DecodeString(signatureB64)
	if err != nil {
		return Claims{}, fmt.Errorf("invalid signature encoding: %w", err)
	}

	if !hmac.Equal(actualSig, expectedSig) {
		return Claims{}, fmt.Errorf("invalid token signature")
	}

	// Decode payload
	payloadBytes, err := base64.RawURLEncoding.DecodeString(payloadB64)
	if err != nil {
		return Claims{}, fmt.Errorf("invalid payload encoding: %w", err)
	}

	var payload jwtPayload
	if err := json.Unmarshal(payloadBytes, &payload); err != nil {
		return Claims{}, fmt.Errorf("invalid payload JSON: %w", err)
	}

	expiresAt := time.Unix(payload.Exp, 0)
	if time.Now().After(expiresAt) {
		return Claims{}, fmt.Errorf("token expired at %s", expiresAt.Format(time.RFC3339))
	}

	return Claims{
		OrgID:     payload.OrgID,
		UserID:    payload.UserID,
		SessionID: payload.SessionID,
		ExpiresAt: expiresAt,
	}, nil
}
