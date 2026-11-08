package decisionkey

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"strings"
	"testing"
)

func testKey(seed byte) ed25519.PublicKey {
	return ed25519.NewKeyFromSeed(bytes.Repeat([]byte{seed}, ed25519.SeedSize)).Public().(ed25519.PublicKey)
}

// A key whose standard-base64 form differs from its URL-safe form, so the two
// decoders cannot be confused with each other.
func keyWithURLSpecificChars(t *testing.T) ed25519.PublicKey {
	t.Helper()
	for seed := 0; seed < 255; seed++ {
		key := testKey(byte(seed))
		if strings.ContainsAny(base64.RawStdEncoding.EncodeToString(key), "+/") {
			return key
		}
	}
	t.Fatal("no test key with +// in standard base64")
	return nil
}

func TestDecodePublicKeyPrefersURLSafeUnpadded(t *testing.T) {
	key := keyWithURLSpecificChars(t)
	got, canonical, err := DecodePublicKey(base64.RawURLEncoding.EncodeToString(key))
	if err != nil || !canonical || !bytes.Equal(got, key) {
		t.Fatalf("unpadded base64url: got canonical=%v err=%v equal=%v", canonical, err, bytes.Equal(got, key))
	}
}

func TestDecodePublicKeyAcceptsOtherEncodingsAsNonCanonical(t *testing.T) {
	key := keyWithURLSpecificChars(t)
	for name, encoded := range map[string]string{
		"padded url-safe": base64.URLEncoding.EncodeToString(key),
		"standard":        base64.StdEncoding.EncodeToString(key),
		"raw standard":    base64.RawStdEncoding.EncodeToString(key),
	} {
		got, canonical, err := DecodePublicKey("  " + encoded + "\n")
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if canonical {
			t.Fatalf("%s: reported canonical", name)
		}
		if !bytes.Equal(got, key) {
			t.Fatalf("%s: decoded to a different key", name)
		}
	}
}

func TestDecodePublicKeyRefusesGarbageEmptyAndWrongLength(t *testing.T) {
	short := base64.RawURLEncoding.EncodeToString([]byte("too short"))
	for name, raw := range map[string]string{
		"empty":        "",
		"blank":        "   ",
		"garbage":      "not base64 at all!!",
		"wrong length": short,
	} {
		key, _, err := DecodePublicKey(raw)
		if err == nil || key != nil {
			t.Fatalf("%s: expected refusal, got key=%v err=%v", name, key, err)
		}
		if raw != "" && strings.Contains(err.Error(), strings.TrimSpace(raw)) && strings.TrimSpace(raw) != "" {
			t.Fatalf("%s: error echoes the value: %v", name, err)
		}
	}
}
