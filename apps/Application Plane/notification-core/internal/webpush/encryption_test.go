package webpush

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"crypto/ecdh"
	"crypto/hkdf"
	"crypto/rand"
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"testing"
)

// The RFC 8291 Appendix A example. Every value is the RFC's own, so the known-answer test checks the key schedule,
// the record layout and AES-128-GCM byte for byte rather than only checking that encryption round-trips.
const (
	rfcPlaintext     = "When I grow up, I want to be a watermelon"
	rfcASPrivate     = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw"
	rfcASPublic      = "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"
	rfcUAPublic      = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"
	rfcSalt          = "DGv6ra1nlYgDCS1FRnbzlw"
	rfcAuthSecret    = "BTBZMqHH6r4Tts7J_aSIgg"
	rfcHeader        = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"
	rfcCiphertext    = "8pfeW0KbunFT06SuDKoJH9Ql87S1QUrdirN6GcG7sFz1y1sqLgVi1VhjVkHsUoEsbI_0LpXMuGvnzQ"
	receiverKeyBytes = 65
)

func mustDecode(t *testing.T, value string) []byte {
	t.Helper()
	decoded, err := decodeBase64URL(value)
	if err != nil {
		t.Fatalf("decodeBase64URL(%q) error = %v", value, err)
	}
	return decoded
}

func TestSealRecordMatchesTheRFC8291AppendixA(t *testing.T) {
	ephemeral, err := ecdh.P256().NewPrivateKey(mustDecode(t, rfcASPrivate))
	if err != nil {
		t.Fatalf("NewPrivateKey() error = %v", err)
	}
	if !bytes.Equal(ephemeral.PublicKey().Bytes(), mustDecode(t, rfcASPublic)) {
		t.Fatal("the RFC application server key does not derive the RFC public key")
	}
	receiver, err := ecdh.P256().NewPublicKey(mustDecode(t, rfcUAPublic))
	if err != nil {
		t.Fatalf("NewPublicKey() error = %v", err)
	}

	got, err := sealRecord(ephemeral, mustDecode(t, rfcSalt), receiver, mustDecode(t, rfcAuthSecret), []byte(rfcPlaintext))
	if err != nil {
		t.Fatalf("sealRecord() error = %v", err)
	}
	want := append(mustDecode(t, rfcHeader), mustDecode(t, rfcCiphertext)...)
	if !bytes.Equal(got, want) {
		t.Fatalf("sealRecord() = %s, want the RFC 8291 message %s", encodeBase64URL(got), encodeBase64URL(want))
	}
}

// openWakeUp is the receiver's half of RFC 8291. It derives the same keys from its own private key and the key id
// in the header, then opens the single record and checks the padding delimiter.
func openWakeUp(t *testing.T, receiver *ecdh.PrivateKey, authSecret, message []byte) []byte {
	t.Helper()
	if len(message) < 21 {
		t.Fatalf("message is %d bytes, shorter than the record header", len(message))
	}
	salt := message[:16]
	if got := binary.BigEndian.Uint32(message[16:20]); got != recordSize {
		t.Fatalf("rs = %d, want %d", got, recordSize)
	}
	keyIDLength := int(message[20])
	if keyIDLength != receiverKeyBytes {
		t.Fatalf("idlen = %d, want %d", keyIDLength, receiverKeyBytes)
	}
	senderPublic, err := ecdh.P256().NewPublicKey(message[21 : 21+keyIDLength])
	if err != nil {
		t.Fatalf("sender key id is not a P-256 point: %v", err)
	}
	sharedSecret, err := receiver.ECDH(senderPublic)
	if err != nil {
		t.Fatalf("ECDH() error = %v", err)
	}
	keyInfo := append(append([]byte("WebPush: info\x00"), receiver.PublicKey().Bytes()...), senderPublic.Bytes()...)
	ikm, err := hkdf.Key(sha256.New, sharedSecret, authSecret, string(keyInfo), 32)
	if err != nil {
		t.Fatalf("hkdf ikm error = %v", err)
	}
	contentKey, err := hkdf.Key(sha256.New, ikm, salt, "Content-Encoding: aes128gcm\x00", 16)
	if err != nil {
		t.Fatalf("hkdf cek error = %v", err)
	}
	nonce, err := hkdf.Key(sha256.New, ikm, salt, "Content-Encoding: nonce\x00", 12)
	if err != nil {
		t.Fatalf("hkdf nonce error = %v", err)
	}
	block, err := aes.NewCipher(contentKey)
	if err != nil {
		t.Fatalf("aes.NewCipher() error = %v", err)
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		t.Fatalf("cipher.NewGCM() error = %v", err)
	}
	record, err := aead.Open(nil, nonce, message[21+keyIDLength:], nil)
	if err != nil {
		t.Fatalf("aead.Open() error = %v", err)
	}
	if len(record) == 0 || record[len(record)-1] != 0x02 {
		t.Fatal("record does not end with the 0x02 last-record delimiter")
	}
	return record[:len(record)-1]
}

func TestEncryptWakeUpRoundTripsThroughARecipient(t *testing.T) {
	receiver, err := ecdh.P256().GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	authSecret := make([]byte, 16)
	if _, err := rand.Read(authSecret); err != nil {
		t.Fatalf("rand.Read() error = %v", err)
	}
	p256dh := encodeBase64URL(receiver.PublicKey().Bytes())
	auth := encodeBase64URL(authSecret)

	message, err := encryptWakeUp(p256dh, auth, []byte(wakeUpPayload), rand.Reader)
	if err != nil {
		t.Fatalf("encryptWakeUp() error = %v", err)
	}
	if got := openWakeUp(t, receiver, authSecret, message); string(got) != wakeUpPayload {
		t.Fatalf("recipient read %q, want %q", got, wakeUpPayload)
	}
}

func TestEncryptWakeUpRefusesMalformedSubscriptionKeys(t *testing.T) {
	receiver, err := ecdh.P256().GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	validPoint := encodeBase64URL(receiver.PublicKey().Bytes())
	validAuth := encodeBase64URL(make([]byte, 16))
	compressed := encodeBase64URL(append([]byte{0x02}, receiver.PublicKey().Bytes()[1:33]...))

	cases := []struct {
		name   string
		p256dh string
		auth   string
	}{
		{name: "p256dh is not base64url", p256dh: "%%%", auth: validAuth},
		{name: "p256dh is a compressed point", p256dh: compressed, auth: validAuth},
		{name: "p256dh is not on the curve", p256dh: encodeBase64URL(make([]byte, 65)), auth: validAuth},
		{name: "auth secret is 15 bytes", p256dh: validPoint, auth: encodeBase64URL(make([]byte, 15))},
		{name: "auth secret is empty", p256dh: validPoint, auth: ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := encryptWakeUp(tc.p256dh, tc.auth, []byte(wakeUpPayload), rand.Reader)
			if !errors.Is(err, ErrInvalidSubscription) {
				t.Fatalf("encryptWakeUp() error = %v, want ErrInvalidSubscription", err)
			}
		})
	}
}
