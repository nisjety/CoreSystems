package org

import (
	"bytes"
	"encoding/base64"
	"strings"
	"testing"
)

func testSIEMKey(fill byte) []byte {
	return bytes.Repeat([]byte{fill}, siemSealKeyBytes)
}

func mustSealer(t *testing.T, keyID string, fill byte) *SIEMSealer {
	t.Helper()
	sealer, err := NewSIEMSealer(keyID, testSIEMKey(fill))
	if err != nil {
		t.Fatalf("new sealer: %v", err)
	}
	return sealer
}

func TestSIEMSealRoundTrip(t *testing.T) {
	sealer := mustSealer(t, "v1", 0x11)
	secret := []byte("hec-token-0123456789abcdef")
	sealed, err := sealer.Seal("org-1", "dest-1", secret)
	if err != nil {
		t.Fatalf("seal: %v", err)
	}
	if bytes.Contains(sealed, secret) {
		t.Fatalf("ciphertext contains the plaintext secret")
	}
	opened, err := sealer.Open("v1", "org-1", "dest-1", sealed)
	if err != nil || !bytes.Equal(opened, secret) {
		t.Fatalf("open = %q, %v; want the secret back", opened, err)
	}

	// A fresh nonce per seal: the same secret never yields the same ciphertext.
	again, err := sealer.Seal("org-1", "dest-1", secret)
	if err != nil || bytes.Equal(sealed, again) {
		t.Fatalf("two seals of one secret were identical (err=%v)", err)
	}
}

func TestSIEMSealRefusesTheWrongKeyAndTheWrongBinding(t *testing.T) {
	sealer := mustSealer(t, "v1", 0x11)
	sealed, err := sealer.Seal("org-1", "dest-1", []byte("hec-token-0123456789abcdef"))
	if err != nil {
		t.Fatalf("seal: %v", err)
	}

	if _, err := mustSealer(t, "v1", 0x22).Open("v1", "org-1", "dest-1", sealed); err == nil {
		t.Fatalf("a different key opened the secret")
	}
	if _, err := sealer.Open("v2", "org-1", "dest-1", sealed); err == nil || !strings.Contains(err.Error(), "key id") {
		t.Fatalf("a different key id was tried instead of refused: %v", err)
	}
	// Bound to the row it was sealed for: copying the ciphertext to another
	// organisation or destination must not open it.
	if _, err := sealer.Open("v1", "org-2", "dest-1", sealed); err == nil {
		t.Fatalf("the secret opened under another organisation")
	}
	if _, err := sealer.Open("v1", "org-1", "dest-2", sealed); err == nil {
		t.Fatalf("the secret opened under another destination")
	}
	tampered := append([]byte(nil), sealed...)
	tampered[len(tampered)-1] ^= 0x01
	if _, err := sealer.Open("v1", "org-1", "dest-1", tampered); err == nil {
		t.Fatalf("a tampered ciphertext opened")
	}
	if _, err := sealer.Open("v1", "org-1", "dest-1", sealed[:5]); err == nil {
		t.Fatalf("a truncated ciphertext opened")
	}
}

func TestSIEMSealerFromEnvAcceptsEveryBase64SpellingAndNeverEchoesTheKey(t *testing.T) {
	// Bytes chosen so the URL-safe and standard spellings differ.
	key := make([]byte, siemSealKeyBytes)
	for i := range key {
		key[i] = byte(0xfb - i)
	}
	for name, encoded := range map[string]string{
		"std":        base64.StdEncoding.EncodeToString(key),
		"std-nopad":  base64.RawStdEncoding.EncodeToString(key),
		"url":        base64.URLEncoding.EncodeToString(key),
		"url-nopad":  base64.RawURLEncoding.EncodeToString(key),
		"whitespace": " " + base64.StdEncoding.EncodeToString(key) + "\n",
	} {
		env := map[string]string{SIEMSealKeyEnv: encoded, SIEMSealKeyIDEnv: "k-2026"}
		sealer, err := NewSIEMSealerFromEnv(func(k string) string { return env[k] })
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if sealer.KeyID() != "k-2026" {
			t.Fatalf("%s: key id = %q", name, sealer.KeyID())
		}
		sealed, err := sealer.Seal("o", "d", []byte("0123456789abcdef"))
		if err != nil {
			t.Fatalf("%s: seal: %v", name, err)
		}
		reference, _ := NewSIEMSealer("k-2026", key)
		if _, err := reference.Open("k-2026", "o", "d", sealed); err != nil {
			t.Fatalf("%s decoded to a different key: %v", name, err)
		}
	}

	badShort := base64.StdEncoding.EncodeToString(key[:16])
	for name, env := range map[string]map[string]string{
		"missing":    {},
		"too short":  {SIEMSealKeyEnv: badShort},
		"not base64": {SIEMSealKeyEnv: "!!!not-base64!!!"},
		"bad key id": {SIEMSealKeyEnv: base64.StdEncoding.EncodeToString(key), SIEMSealKeyIDEnv: "has space"},
	} {
		_, err := NewSIEMSealerFromEnv(func(k string) string { return env[k] })
		if err == nil {
			t.Fatalf("%s: expected an error", name)
		}
		for _, leaked := range []string{badShort, "!!!not-base64!!!"} {
			if strings.Contains(err.Error(), leaked) {
				t.Fatalf("%s: error echoes the key material: %v", name, err)
			}
		}
	}

	// Default key id when none is configured.
	sealer, err := NewSIEMSealerFromEnv(func(k string) string {
		return map[string]string{SIEMSealKeyEnv: base64.StdEncoding.EncodeToString(key)}[k]
	})
	if err != nil || sealer.KeyID() != "v1" {
		t.Fatalf("default key id = %q, %v; want v1", sealer.KeyID(), err)
	}
}

func TestAnUnconfiguredSIEMSealerRefuses(t *testing.T) {
	var sealer *SIEMSealer
	if _, err := sealer.Seal("o", "d", []byte("x")); err == nil {
		t.Fatalf("a nil sealer sealed")
	}
	if _, err := sealer.Open("v1", "o", "d", []byte("x")); err == nil {
		t.Fatalf("a nil sealer opened")
	}
	if sealer.KeyID() != "" {
		t.Fatalf("a nil sealer has a key id")
	}
}
