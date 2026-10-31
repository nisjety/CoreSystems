package chain

import (
	"crypto/ed25519"
	"encoding/base64"
	"errors"
	"strings"
	"testing"
	"time"
)

// testSeed returns a deterministic 32-byte seed and its public key, so a test
// can present the same key in every encoding the loader accepts.
func testSeed(b byte) ([]byte, ed25519.PrivateKey) {
	seed := make([]byte, ed25519.SeedSize)
	for i := range seed {
		seed[i] = b + byte(i)
	}
	return seed, ed25519.NewKeyFromSeed(seed)
}

func testHash(b byte) []byte {
	h := make([]byte, HashSize)
	for i := range h {
		h[i] = b
	}
	return h
}

var signedAt = time.Date(2026, 10, 22, 9, 30, 15, 123456000, time.UTC)

func newRing(t *testing.T, cfg KeyConfig) *KeyRing {
	t.Helper()
	ring, err := NewKeyRing(cfg)
	if err != nil {
		t.Fatalf("NewKeyRing: %v", err)
	}
	return ring
}

func TestKeyRingAcceptsBase64urlAndStandardEncodings(t *testing.T) {
	// A seed whose base64 uses both '+' and '/' (and so '-' and '_' in url form)
	// is the case where the two alphabets decode to different strings.
	seed := make([]byte, ed25519.SeedSize)
	for i := range seed {
		seed[i] = 0xfb
		if i%2 == 1 {
			seed[i] = 0xff
		}
	}
	priv := ed25519.NewKeyFromSeed(seed)
	pub := priv.Public().(ed25519.PublicKey)

	encodings := map[string]*base64.Encoding{
		"base64url no pad": base64.RawURLEncoding,
		"base64url padded": base64.URLEncoding,
		"standard padded":  base64.StdEncoding,
		"standard no pad":  base64.RawStdEncoding,
	}
	want, err := newRing(t, KeyConfig{KeyID: "k1", Private: base64.RawURLEncoding.EncodeToString(seed)}).
		signCheckpointForTest()
	if err != nil {
		t.Fatal(err)
	}
	for name, enc := range encodings {
		for form, private := range map[string][]byte{"seed": seed, "expanded": priv} {
			ring := newRing(t, KeyConfig{
				KeyID:   "k1",
				Private: enc.EncodeToString(private),
				Public:  enc.EncodeToString(pub),
			})
			if !ring.CanSign() || ring.ActiveKeyID() != "k1" {
				t.Fatalf("%s %s: ring cannot sign", name, form)
			}
			got, err := ring.signCheckpointForTest()
			if err != nil {
				t.Fatal(err)
			}
			if string(got) != string(want) {
				t.Fatalf("%s %s: a different key was loaded (Ed25519 signatures are deterministic)", name, form)
			}
		}
	}
}

// signCheckpointForTest signs one fixed checkpoint and returns the signature.
func (k *KeyRing) signCheckpointForTest() ([]byte, error) {
	_, sig, err := k.SignCheckpoint("org-1", 7, testHash(1), 7, signedAt)
	return sig, err
}

func TestKeyRingRejectsBadConfiguration(t *testing.T) {
	seed, priv := testSeed(1)
	_, otherPriv := testSeed(40)
	good := base64.RawURLEncoding.EncodeToString(seed)
	pub := base64.RawURLEncoding.EncodeToString(priv.Public().(ed25519.PublicKey))
	otherPub := base64.RawURLEncoding.EncodeToString(otherPriv.Public().(ed25519.PublicKey))

	for name, cfg := range map[string]KeyConfig{
		"private key without key id":       {Private: good},
		"public key without key id":        {Public: pub},
		"key id with a separator":          {KeyID: "a:b", Private: good},
		"key id with a comma":              {KeyID: "a,b", Private: good},
		"private key not base64":           {KeyID: "k", Private: "not base64 !!"},
		"private key wrong length":         {KeyID: "k", Private: base64.RawURLEncoding.EncodeToString(seed[:31])},
		"public key does not match":        {KeyID: "k", Private: good, Public: otherPub},
		"public key wrong length":          {KeyID: "k", Public: base64.RawURLEncoding.EncodeToString(seed[:16])},
		"verify entry without separator":   {VerifyKeys: "old" + pub},
		"verify entry with bad key":        {VerifyKeys: "old:AAAA"},
		"verify key rebinds the active id": {KeyID: "k", Private: good, VerifyKeys: "k:" + otherPub},
	} {
		if _, err := NewKeyRing(cfg); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
}

func TestKeyRingWithoutKeysCannotSignOrVerify(t *testing.T) {
	ring := newRing(t, KeyConfig{})
	if ring.CanSign() || ring.ActiveKeyID() != "" {
		t.Fatal("an empty ring claims a signing key")
	}
	if _, _, err := ring.SignAttestation("org-1", 1, testHash(1), signedAt); !errors.Is(err, ErrNoSigningKey) {
		t.Fatalf("sign without a key: %v, want ErrNoSigningKey", err)
	}
	if err := ring.VerifyAttestation("org-1", 1, testHash(1), signedAt, "k", make([]byte, 64)); !errors.Is(err, ErrUnknownKeyID) {
		t.Fatalf("verify with no keys: %v, want ErrUnknownKeyID", err)
	}
	var nilRing *KeyRing
	if nilRing.CanSign() || nilRing.ActiveKeyID() != "" {
		t.Fatal("a nil ring must behave as an empty ring")
	}
}

func TestAttestationSignatureRoundTripAndTampering(t *testing.T) {
	seed, _ := testSeed(1)
	ring := newRing(t, KeyConfig{KeyID: "k1", Private: base64.RawURLEncoding.EncodeToString(seed)})
	keyID, sig, err := ring.SignAttestation("org-1", 12, testHash(9), signedAt)
	if err != nil {
		t.Fatal(err)
	}
	if keyID != "k1" {
		t.Fatalf("key id = %q", keyID)
	}
	if err := ring.VerifyAttestation("org-1", 12, testHash(9), signedAt, keyID, sig); err != nil {
		t.Fatalf("genuine attestation: %v", err)
	}
	for name, check := range map[string]func() error{
		"other org":  func() error { return ring.VerifyAttestation("org-2", 12, testHash(9), signedAt, keyID, sig) },
		"other seq":  func() error { return ring.VerifyAttestation("org-1", 13, testHash(9), signedAt, keyID, sig) },
		"other hash": func() error { return ring.VerifyAttestation("org-1", 12, testHash(8), signedAt, keyID, sig) },
		"other time": func() error {
			return ring.VerifyAttestation("org-1", 12, testHash(9), signedAt.Add(time.Microsecond), keyID, sig)
		},
		"short sig":   func() error { return ring.VerifyAttestation("org-1", 12, testHash(9), signedAt, keyID, sig[:63]) },
		"empty sig":   func() error { return ring.VerifyAttestation("org-1", 12, testHash(9), signedAt, keyID, nil) },
		"flipped bit": func() error { return ring.VerifyAttestation("org-1", 12, testHash(9), signedAt, keyID, flip(sig)) },
	} {
		if err := check(); !errors.Is(err, ErrBadSignature) {
			t.Errorf("%s: %v, want ErrBadSignature", name, err)
		}
	}
	// A checkpoint signature is not an attestation: the magics keep the two
	// signed payloads from ever being interchangeable.
	_, cpSig, err := ring.SignCheckpoint("org-1", 12, testHash(9), 12, signedAt)
	if err != nil {
		t.Fatal(err)
	}
	if err := ring.VerifyAttestation("org-1", 12, testHash(9), signedAt, keyID, cpSig); !errors.Is(err, ErrBadSignature) {
		t.Fatalf("checkpoint signature accepted as an attestation: %v", err)
	}
}

func TestCheckpointSignatureRoundTripAndTampering(t *testing.T) {
	seed, _ := testSeed(1)
	ring := newRing(t, KeyConfig{KeyID: "k1", Private: base64.RawURLEncoding.EncodeToString(seed)})
	keyID, sig, err := ring.SignCheckpoint("org-1", 5000, testHash(3), 5000, signedAt)
	if err != nil {
		t.Fatal(err)
	}
	if err := ring.VerifyCheckpoint("org-1", 5000, testHash(3), 5000, signedAt, keyID, sig); err != nil {
		t.Fatalf("genuine checkpoint: %v", err)
	}
	for name, check := range map[string]func() error{
		"other through seq":  func() error { return ring.VerifyCheckpoint("org-1", 4999, testHash(3), 5000, signedAt, keyID, sig) },
		"other through hash": func() error { return ring.VerifyCheckpoint("org-1", 5000, testHash(4), 5000, signedAt, keyID, sig) },
		"other pruned total": func() error { return ring.VerifyCheckpoint("org-1", 5000, testHash(3), 4999, signedAt, keyID, sig) },
		"other org":          func() error { return ring.VerifyCheckpoint("org-2", 5000, testHash(3), 5000, signedAt, keyID, sig) },
		"other created_at": func() error {
			return ring.VerifyCheckpoint("org-1", 5000, testHash(3), 5000, signedAt.Add(time.Second), keyID, sig)
		},
		"flipped bit": func() error {
			return ring.VerifyCheckpoint("org-1", 5000, testHash(3), 5000, signedAt, keyID, flip(sig))
		},
	} {
		if err := check(); !errors.Is(err, ErrBadSignature) {
			t.Errorf("%s: %v, want ErrBadSignature", name, err)
		}
	}
}

func TestSigningRejectsUnsignableInputs(t *testing.T) {
	seed, _ := testSeed(1)
	ring := newRing(t, KeyConfig{KeyID: "k1", Private: base64.RawURLEncoding.EncodeToString(seed)})
	if _, _, err := ring.SignCheckpoint("org-1", 0, testHash(1), 0, signedAt); !errors.Is(err, ErrBadSequence) {
		t.Errorf("sequence 0: %v", err)
	}
	if _, _, err := ring.SignCheckpoint("org-1", 1, testHash(1), -1, signedAt); !errors.Is(err, ErrBadCount) {
		t.Errorf("negative pruned total: %v", err)
	}
	if _, _, err := ring.SignAttestation("org\n1", 1, testHash(1), signedAt); !errors.Is(err, ErrOrgNewline) {
		t.Errorf("org with a newline: %v", err)
	}
	if _, _, err := ring.SignAttestation("org-1", 1, testHash(1)[:5], signedAt); !errors.Is(err, ErrBadHash) {
		t.Errorf("short hash: %v", err)
	}
}

func TestWrongKeyIDAndRotation(t *testing.T) {
	oldSeed, oldPriv := testSeed(1)
	newSeed, newPriv := testSeed(90)
	oldB64 := base64.RawURLEncoding.EncodeToString(oldSeed)
	oldPub := base64.RawURLEncoding.EncodeToString(oldPriv.Public().(ed25519.PublicKey))
	newB64 := base64.RawURLEncoding.EncodeToString(newSeed)
	newPub := base64.RawURLEncoding.EncodeToString(newPriv.Public().(ed25519.PublicKey))

	oldRing := newRing(t, KeyConfig{KeyID: "audit-2026-a", Private: oldB64})
	oldKeyID, oldSig, err := oldRing.SignCheckpoint("org-1", 100, testHash(5), 100, signedAt)
	if err != nil {
		t.Fatal(err)
	}

	// After rotation the new key signs, and the retired public key still
	// verifies what it signed.
	rotated := newRing(t, KeyConfig{KeyID: "audit-2026-b", Private: newB64, VerifyKeys: "audit-2026-a:" + oldPub})
	if rotated.ActiveKeyID() != "audit-2026-b" {
		t.Fatalf("active key = %q", rotated.ActiveKeyID())
	}
	if err := rotated.VerifyCheckpoint("org-1", 100, testHash(5), 100, signedAt, oldKeyID, oldSig); err != nil {
		t.Fatalf("a retired key's checkpoint must verify after rotation: %v", err)
	}
	newKeyID, newSig, err := rotated.SignCheckpoint("org-1", 200, testHash(6), 200, signedAt)
	if err != nil || newKeyID != "audit-2026-b" {
		t.Fatalf("sign with the rotated key: id=%q err=%v", newKeyID, err)
	}
	if err := rotated.VerifyCheckpoint("org-1", 200, testHash(6), 200, signedAt, newKeyID, newSig); err != nil {
		t.Fatal(err)
	}

	// A ring that dropped the retired key cannot vouch for its checkpoints.
	forgotten := newRing(t, KeyConfig{KeyID: "audit-2026-b", Private: newB64})
	if err := forgotten.VerifyCheckpoint("org-1", 100, testHash(5), 100, signedAt, oldKeyID, oldSig); !errors.Is(err, ErrUnknownKeyID) {
		t.Fatalf("checkpoint of a key that left the ring: %v, want ErrUnknownKeyID", err)
	}
	// Naming the wrong (known) key id fails the signature rather than the lookup.
	if err := rotated.VerifyCheckpoint("org-1", 100, testHash(5), 100, signedAt, "audit-2026-b", oldSig); !errors.Is(err, ErrBadSignature) {
		t.Fatalf("checkpoint relabelled with another key id: %v, want ErrBadSignature", err)
	}
	// A public-only ring verifies without being able to sign.
	verifyOnly := newRing(t, KeyConfig{KeyID: "audit-2026-b", Public: newPub})
	if verifyOnly.CanSign() {
		t.Fatal("a public-only ring must not sign")
	}
	if err := verifyOnly.VerifyCheckpoint("org-1", 200, testHash(6), 200, signedAt, newKeyID, newSig); err != nil {
		t.Fatalf("public-only ring: %v", err)
	}
	if _, ok := verifyOnly.PublicKey("nope"); ok {
		t.Fatal("PublicKey invented a key")
	}
}

func TestKeyErrorsNameNoKeyMaterial(t *testing.T) {
	secret := "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" // wrong length on purpose
	_, err := NewKeyRing(KeyConfig{KeyID: "k", Private: secret})
	if err == nil {
		t.Fatal("wrong-length key accepted")
	}
	if strings.Contains(err.Error(), secret) {
		t.Fatalf("error repeats the key material: %v", err)
	}
}

func flip(b []byte) []byte {
	out := append([]byte(nil), b...)
	out[0] ^= 1
	return out
}
