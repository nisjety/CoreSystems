// Package webpush sends content-free Web Push wake-ups for the attention notifications. It owns the VAPID
// identity (RFC 8292), the aes128gcm payload encryption (RFC 8291 over RFC 8188), the sealed subscription
// store and the per-recipient dispatch. Nothing here reads or sends conversation content.
package webpush

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/url"
	"strings"
	"time"
)

// vapidTokenLifetime is how long one signed VAPID token stays valid. RFC 8292 allows up to 24 hours; a
// shorter lifetime limits how long a captured Authorization header can be replayed.
const vapidTokenLifetime = 12 * time.Hour

// VAPIDKeys is the application server identity for Web Push. Browsers receive the public key to bind their
// subscription to this server; only notification-core holds the private key.
type VAPIDKeys struct {
	publicKey  string
	privateKey *ecdsa.PrivateKey
}

// LoadVAPID parses the base64url public and private halves of the VAPID key pair. It rejects a pair whose
// halves do not belong together, so a mismatch fails at start-up rather than at the first push.
func LoadVAPID(publicKey, privateKey string) (*VAPIDKeys, error) {
	publicBytes, err := decodeBase64URL(publicKey)
	if err != nil {
		return nil, errors.New("VAPID public key must be base64url")
	}
	parsedPublic, err := ecdsa.ParseUncompressedPublicKey(elliptic.P256(), publicBytes)
	if err != nil {
		return nil, errors.New("VAPID public key must be an uncompressed P-256 point")
	}
	privateBytes, err := decodeBase64URL(privateKey)
	if err != nil {
		return nil, errors.New("VAPID private key must be base64url")
	}
	parsedPrivate, err := ecdsa.ParseRawPrivateKey(elliptic.P256(), privateBytes)
	if err != nil {
		return nil, errors.New("VAPID private key must be a P-256 scalar")
	}
	if !parsedPrivate.PublicKey.Equal(parsedPublic) {
		return nil, errors.New("VAPID public key does not belong to the private key")
	}
	return &VAPIDKeys{publicKey: encodeBase64URL(publicBytes), privateKey: parsedPrivate}, nil
}

// PublicKey returns the base64url uncompressed public key that browsers pass as applicationServerKey.
func (k *VAPIDKeys) PublicKey() string {
	return k.publicKey
}

type vapidClaims struct {
	Audience string `json:"aud"`
	Expires  int64  `json:"exp"`
	Subject  string `json:"sub"`
}

// authorization returns the Authorization header for one push to one endpoint: a token signed with the
// private key and bound to the endpoint's origin, followed by the public key the push service checks it
// against.
func (k *VAPIDKeys) authorization(endpoint, subject string, now time.Time) (string, error) {
	audience, err := originOf(endpoint)
	if err != nil {
		return "", err
	}
	header, err := json.Marshal(map[string]string{"typ": "JWT", "alg": "ES256"})
	if err != nil {
		return "", err
	}
	claims, err := json.Marshal(vapidClaims{
		Audience: audience,
		Expires:  now.Add(vapidTokenLifetime).Unix(),
		Subject:  subject,
	})
	if err != nil {
		return "", err
	}
	signingInput := encodeBase64URL(header) + "." + encodeBase64URL(claims)
	digest := sha256.Sum256([]byte(signingInput))
	r, s, err := ecdsa.Sign(rand.Reader, k.privateKey, digest[:])
	if err != nil {
		return "", err
	}
	// JWS ES256 carries the signature as the fixed-width R and S values, not as ASN.1.
	signature := make([]byte, 64)
	r.FillBytes(signature[:32])
	s.FillBytes(signature[32:])
	return "vapid t=" + signingInput + "." + encodeBase64URL(signature) + ", k=" + k.publicKey, nil
}

func originOf(endpoint string) (string, error) {
	parsed, err := url.Parse(endpoint)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return "", errors.New("web push endpoint has no origin")
	}
	return parsed.Scheme + "://" + parsed.Host, nil
}

func encodeBase64URL(value []byte) string {
	return base64.RawURLEncoding.EncodeToString(value)
}

// decodeBase64URL accepts the unpadded form browsers emit and tolerates trailing padding.
func decodeBase64URL(value string) ([]byte, error) {
	return base64.RawURLEncoding.DecodeString(strings.TrimRight(strings.TrimSpace(value), "="))
}
