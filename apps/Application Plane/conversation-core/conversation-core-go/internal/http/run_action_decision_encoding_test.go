package http

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"strings"
	"testing"
)

// The defect: Control mints these keys base64url, and this file decoded the
// token's own envelope URL-safe while decoding the KEY standard-only. A real
// key containing `-` or `_` therefore refused to start the service, and would
// never have verified a decision if it had started.
func TestDecodeControlPublicKeyAcceptsTheEncodingControlActuallyMints(t *testing.T) {
	// Generate until the URL-safe form differs from the standard one, so the
	// test is actually exercising the alphabet difference rather than a key
	// that happens to encode identically in both.
	var publicKey ed25519.PublicKey
	for attempt := 0; attempt < 200; attempt++ {
		candidate, _, err := ed25519.GenerateKey(rand.Reader)
		if err != nil {
			t.Fatal(err)
		}
		urlSafe := base64.RawURLEncoding.EncodeToString(candidate)
		if strings.ContainsAny(urlSafe, "-_") {
			publicKey = candidate
			break
		}
	}
	if publicKey == nil {
		t.Skip("no key with a URL-safe-specific character was generated")
	}

	for name, encoded := range map[string]string{
		"raw url":      base64.RawURLEncoding.EncodeToString(publicKey),
		"padded url":   base64.URLEncoding.EncodeToString(publicKey),
		"raw standard": base64.RawStdEncoding.EncodeToString(publicKey),
		"padded std":   base64.StdEncoding.EncodeToString(publicKey),
	} {
		decoded, ok := DecodeControlPublicKey(encoded)
		if !ok {
			t.Fatalf("%s encoding was rejected", name)
		}
		if !decoded.Equal(publicKey) {
			t.Fatalf("%s encoding decoded to a different key", name)
		}
	}

	// And the verifier itself, since that is the path a decision travels.
	if _, err := NewRunActionDecisionVerifier(
		"control-space-decision-1",
		base64.RawURLEncoding.EncodeToString(publicKey),
	); err != nil {
		t.Fatalf("verifier rejected a URL-safe Control key: %v", err)
	}
}

func TestDecodeControlPublicKeyRefusesNonKeys(t *testing.T) {
	for name, encoded := range map[string]string{
		"empty":      "   ",
		"not base64": "!!!!",
		"too short":  base64.RawURLEncoding.EncodeToString([]byte("short")),
	} {
		if _, ok := DecodeControlPublicKey(encoded); ok {
			t.Fatalf("%s was accepted as an Ed25519 public key", name)
		}
	}
}
