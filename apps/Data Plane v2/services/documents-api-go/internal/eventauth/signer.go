// Package eventauth creates producer-scoped signed envelopes for asynchronous
// Data Plane events. Private keys remain producer-local; consumers receive only
// the matching public key.
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

var ErrInvalidEvent = errors.New("invalid signed event")

const tokenTTL = 2 * time.Minute

type Signer struct {
	key      *rsa.PrivateKey
	issuer   string
	keyID    string
	audience string
	scope    string
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

func NewSigner(pemBytes []byte, issuer, keyID, audience, scope string) (*Signer, error) {
	if !strings.HasPrefix(issuer, "service:") || strings.TrimPrefix(issuer, "service:") == "" ||
		strings.TrimSpace(keyID) == "" || strings.TrimSpace(audience) == "" || strings.TrimSpace(scope) == "" {
		return nil, ErrInvalidEvent
	}
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
	return &Signer{key: key, issuer: issuer, keyID: keyID, audience: audience, scope: scope}, nil
}

func (s *Signer) Sign(eventType string, payload []byte) ([]byte, error) {
	if s == nil || strings.TrimSpace(eventType) == "" || len(payload) == 0 {
		return nil, ErrInvalidEvent
	}
	if !allowedDocumentSubject(eventType) {
		return nil, ErrInvalidEvent
	}
	var boundary boundaryFields
	if err := json.Unmarshal(payload, &boundary); err != nil || strings.TrimSpace(boundary.OrgID) == "" || boundary.ZDR == nil {
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
	jtiBytes := make([]byte, 16)
	if _, err := rand.Read(jtiBytes); err != nil {
		return nil, ErrInvalidEvent
	}
	digest := sha256.Sum256(payload)
	tokenClaims := claims{
		PrincipalType: "service",
		OrgID:         boundary.OrgID,
		Scopes:        []string{s.scope},
		ZDR:           *boundary.ZDR,
		EventType:     eventType,
		PayloadSHA256: hex.EncodeToString(digest[:]),
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer: s.issuer, Subject: s.issuer,
			Audience:  jwt.ClaimStrings{s.audience},
			ExpiresAt: jwt.NewNumericDate(now.Add(tokenTTL)),
			NotBefore: jwt.NewNumericDate(now),
			IssuedAt:  jwt.NewNumericDate(now),
			ID:        hex.EncodeToString(jtiBytes),
		},
	}
	if boundary.UserID != nil {
		tokenClaims.UserID = *boundary.UserID
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, tokenClaims)
	token.Header["kid"] = s.keyID
	signed, err := token.SignedString(s.key)
	if err != nil {
		return nil, ErrInvalidEvent
	}
	encoded, err := json.Marshal(envelope{
		Authorization: "Bearer " + signed,
		Data:          base64.RawURLEncoding.EncodeToString(payload),
	})
	if err != nil {
		return nil, ErrInvalidEvent
	}
	return encoded, nil
}

func allowedDocumentSubject(subject string) bool {
	switch subject {
	case "dataplane.documents.created", "dataplane.documents.updated", "dataplane.documents.deleted",
		"dataplane.source_objects.changed", "dataplane.source_objects.deleted":
		return true
	default:
		return false
	}
}
