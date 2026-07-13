// Package eventauth signs wiki mutation events with a producer-scoped RS256
// identity. Consumers must verify the envelope before reading its tenant or
// content fields.
package eventauth

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"errors"
	"strings"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

const (
	Issuer               = "service:wiki-store-go"
	KeyID                = "wiki-events-v1"
	Audience             = "dataplane-events"
	Scope                = "events:wiki:publish"
	SubjectWikiPublished = "dataplane.wiki.version.published"
	tokenTTL             = 2 * time.Minute
)

var ErrInvalidEvent = errors.New("invalid signed wiki event")

type Signer struct {
	key *rsa.PrivateKey
}

type claims struct {
	PrincipalType string   `json:"principal_type"`
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id,omitempty"`
	Scopes        []string `json:"scopes"`
	ZDR           bool     `json:"zdr"`
	EventType     string   `json:"event_type"`
	PayloadSHA256 string   `json:"payload_sha256"`
	jwt.RegisteredClaims
}

type boundaryFields struct {
	OrgID  string  `json:"org_id"`
	UserID *string `json:"user_id,omitempty"`
	ZDR    *bool   `json:"zdr"`
}

type envelope struct {
	Authorization string `json:"authorization"`
	Data          string `json:"data"`
}

func NewSigner(pemBytes []byte) (*Signer, error) {
	block, _ := pem.Decode(pemBytes)
	if block == nil {
		return nil, ErrInvalidEvent
	}
	key, err := x509.ParsePKCS1PrivateKey(block.Bytes)
	if err != nil {
		parsed, pkcs8Err := x509.ParsePKCS8PrivateKey(block.Bytes)
		if pkcs8Err != nil {
			return nil, ErrInvalidEvent
		}
		var ok bool
		key, ok = parsed.(*rsa.PrivateKey)
		if !ok {
			return nil, ErrInvalidEvent
		}
	}
	if key.N.BitLen() < 2048 {
		return nil, ErrInvalidEvent
	}
	return &Signer{key: key}, nil
}

func (s *Signer) Sign(eventType string, payload []byte) ([]byte, error) {
	if s == nil || eventType != SubjectWikiPublished || len(payload) == 0 {
		return nil, ErrInvalidEvent
	}
	var boundary boundaryFields
	if err := json.Unmarshal(payload, &boundary); err != nil ||
		strings.TrimSpace(boundary.OrgID) == "" || boundary.ZDR == nil || *boundary.ZDR {
		return nil, ErrInvalidEvent
	}
	if boundary.UserID != nil && strings.TrimSpace(*boundary.UserID) == "" {
		return nil, ErrInvalidEvent
	}
	var object map[string]json.RawMessage
	if err := json.Unmarshal(payload, &object); err != nil || object == nil {
		return nil, ErrInvalidEvent
	}

	now := time.Now().UTC()
	jti := make([]byte, 16)
	if _, err := rand.Read(jti); err != nil {
		return nil, ErrInvalidEvent
	}
	digest := sha256.Sum256(payload)
	tokenClaims := claims{
		PrincipalType: "service",
		OrgID:         boundary.OrgID,
		Scopes:        []string{Scope},
		ZDR:           false,
		EventType:     eventType,
		PayloadSHA256: hex.EncodeToString(digest[:]),
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer: Issuer, Subject: Issuer, Audience: jwt.ClaimStrings{Audience},
			ExpiresAt: jwt.NewNumericDate(now.Add(tokenTTL)),
			NotBefore: jwt.NewNumericDate(now), IssuedAt: jwt.NewNumericDate(now),
			ID: hex.EncodeToString(jti),
		},
	}
	if boundary.UserID != nil {
		tokenClaims.UserID = *boundary.UserID
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, tokenClaims)
	token.Header["kid"] = KeyID
	signed, err := token.SignedString(s.key)
	if err != nil {
		return nil, ErrInvalidEvent
	}
	wire, err := json.Marshal(envelope{
		Authorization: "Bearer " + signed,
		Data:          base64.RawURLEncoding.EncodeToString(payload),
	})
	if err != nil {
		return nil, ErrInvalidEvent
	}
	return wire, nil
}
