package webpush

import (
	"crypto/ecdh"
	"crypto/ecdsa"
	"crypto/rand"
	"crypto/sha256"
	"encoding/json"
	"math/big"
	"strings"
	"testing"
	"time"
)

// newTestVAPID returns a fresh base64url VAPID pair, generated the way the bootstrap does: the private scalar and
// its uncompressed public point.
func newTestVAPID(t *testing.T) (publicKey, privateKey string) {
	t.Helper()
	private, err := ecdh.P256().GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	return encodeBase64URL(private.PublicKey().Bytes()), encodeBase64URL(private.Bytes())
}

func TestLoadVAPIDAcceptsAMatchingPairAndRefusesAMismatch(t *testing.T) {
	publicKey, privateKey := newTestVAPID(t)
	otherPublicKey, _ := newTestVAPID(t)

	keys, err := LoadVAPID(publicKey, privateKey)
	if err != nil {
		t.Fatalf("LoadVAPID(matching pair) error = %v", err)
	}
	if keys.PublicKey() != publicKey {
		t.Fatalf("PublicKey() = %q, want %q", keys.PublicKey(), publicKey)
	}

	for _, tc := range []struct {
		name       string
		publicKey  string
		privateKey string
	}{
		{name: "public key of another pair", publicKey: otherPublicKey, privateKey: privateKey},
		{name: "public key is not base64url", publicKey: "not base64", privateKey: privateKey},
		{name: "private key is short", publicKey: publicKey, privateKey: encodeBase64URL(make([]byte, 16))},
		{name: "private key is zero", publicKey: publicKey, privateKey: encodeBase64URL(make([]byte, 32))},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := LoadVAPID(tc.publicKey, tc.privateKey); err == nil {
				t.Fatal("LoadVAPID() error = nil, want a refused key pair")
			}
		})
	}
}

func TestAuthorizationIsAVerifiableTokenBoundToTheEndpointOrigin(t *testing.T) {
	publicKey, privateKey := newTestVAPID(t)
	keys, err := LoadVAPID(publicKey, privateKey)
	if err != nil {
		t.Fatalf("LoadVAPID() error = %v", err)
	}
	now := time.Date(2026, time.October, 31, 9, 0, 0, 0, time.UTC)

	header, err := keys.authorization("https://push.example.net/send/abc123", "mailto:ops@example.test", now)
	if err != nil {
		t.Fatalf("authorization() error = %v", err)
	}
	if !strings.HasPrefix(header, "vapid t=") {
		t.Fatalf("Authorization = %q, want the vapid scheme", header)
	}
	parts := strings.SplitN(strings.TrimPrefix(header, "vapid t="), ", k=", 2)
	if len(parts) != 2 || parts[1] != publicKey {
		t.Fatalf("Authorization key parameter = %q, want the VAPID public key", header)
	}
	segments := strings.Split(parts[0], ".")
	if len(segments) != 3 {
		t.Fatalf("token has %d segments, want a JWS of three", len(segments))
	}

	var claims struct {
		Audience string `json:"aud"`
		Expires  int64  `json:"exp"`
		Subject  string `json:"sub"`
	}
	claimBytes := mustDecode(t, segments[1])
	if err := json.Unmarshal(claimBytes, &claims); err != nil {
		t.Fatalf("claims are not JSON: %v", err)
	}
	if claims.Audience != "https://push.example.net" {
		t.Fatalf("aud = %q, want the endpoint origin only", claims.Audience)
	}
	if claims.Subject != "mailto:ops@example.test" {
		t.Fatalf("sub = %q", claims.Subject)
	}
	if lifetime := time.Unix(claims.Expires, 0).Sub(now); lifetime <= 0 || lifetime > 24*time.Hour {
		t.Fatalf("token lifetime = %s, want within the 24 hour RFC 8292 limit", lifetime)
	}

	digest := sha256.Sum256([]byte(segments[0] + "." + segments[1]))
	signature := mustDecode(t, segments[2])
	if len(signature) != 64 {
		t.Fatalf("ES256 signature is %d bytes, want 64", len(signature))
	}
	r := new(big.Int).SetBytes(signature[:32])
	s := new(big.Int).SetBytes(signature[32:])
	if !ecdsa.Verify(&keys.privateKey.PublicKey, digest[:], r, s) {
		t.Fatal("the token signature does not verify under the VAPID public key")
	}
}
