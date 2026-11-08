package webpush

import (
	"bytes"
	"crypto/rand"
	"encoding/hex"
	"testing"
)

func newTestSealer(t *testing.T) *sealer {
	t.Helper()
	key := make([]byte, 32)
	if _, err := rand.Read(key); err != nil {
		t.Fatalf("rand.Read() error = %v", err)
	}
	sealer, err := newSealer(hex.EncodeToString(key))
	if err != nil {
		t.Fatalf("newSealer() error = %v", err)
	}
	return sealer
}

func TestSealerBindsEachFieldToItsRowAndColumn(t *testing.T) {
	sealer := newTestSealer(t)
	hash := endpointHashOf("https://fcm.googleapis.com/fcm/send/abc")
	additional := subscriptionAdditionalData("endpoint", "org-1", "user-1", hash)
	plaintext := []byte("https://fcm.googleapis.com/fcm/send/abc")

	sealed, err := sealer.seal(plaintext, additional)
	if err != nil {
		t.Fatalf("seal() error = %v", err)
	}
	if bytes.Contains(sealed, plaintext) {
		t.Fatal("sealed endpoint still contains the plaintext")
	}
	opened, err := sealer.open(sealed, additional)
	if err != nil || !bytes.Equal(opened, plaintext) {
		t.Fatalf("open() = %q, %v; want the original endpoint", opened, err)
	}

	for name, wrong := range map[string][]byte{
		"another organization": subscriptionAdditionalData("endpoint", "org-2", "user-1", hash),
		"another user":         subscriptionAdditionalData("endpoint", "org-1", "user-2", hash),
		"another endpoint":     subscriptionAdditionalData("endpoint", "org-1", "user-1", endpointHashOf("https://other.example/x")),
		"another column":       subscriptionAdditionalData("p256dh", "org-1", "user-1", hash),
	} {
		if _, err := sealer.open(sealed, wrong); err == nil {
			t.Fatalf("a ciphertext opened under %s, want the binding to refuse it", name)
		}
	}

	tampered := append([]byte{}, sealed...)
	tampered[len(tampered)-1] ^= 0x01
	if _, err := sealer.open(tampered, additional); err == nil {
		t.Fatal("a tampered ciphertext opened")
	}
	if _, err := sealer.open(sealed[:4], additional); err == nil {
		t.Fatal("a truncated field opened")
	}
}

func TestSealerUsesAFreshNonceForEveryField(t *testing.T) {
	sealer := newTestSealer(t)
	additional := subscriptionAdditionalData("auth", "org-1", "user-1", endpointHashOf("https://a.example/x"))
	first, err := sealer.seal([]byte("same"), additional)
	if err != nil {
		t.Fatalf("seal() error = %v", err)
	}
	second, err := sealer.seal([]byte("same"), additional)
	if err != nil {
		t.Fatalf("seal() error = %v", err)
	}
	if bytes.Equal(first, second) {
		t.Fatal("two seals of one value are identical, so the nonce is not fresh")
	}
}

func TestNewSealerRefusesAKeyThatIsNotThirtyTwoBytesOfHex(t *testing.T) {
	for _, key := range []string{
		"",
		"not-hex-at-all",
		hex.EncodeToString(make([]byte, 31)),
		hex.EncodeToString(make([]byte, 33)),
	} {
		if _, err := newSealer(key); err == nil {
			t.Fatalf("newSealer(%q) error = nil, want a refused key", key)
		}
	}
}
