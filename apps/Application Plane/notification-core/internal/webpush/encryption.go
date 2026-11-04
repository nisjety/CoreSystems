package webpush

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/ecdh"
	"crypto/hkdf"
	"crypto/sha256"
	"encoding/binary"
	"io"
)

// recordSize is the RFC 8188 rs field. The wake-up fits in one record, so the value only has to be large
// enough for it.
const recordSize = 4096

// encryptWakeUp encrypts plaintext for one browser subscription with the aes128gcm content coding (RFC 8291
// over RFC 8188). The browser's p256dh point and auth secret come from the subscription; the sender's
// ephemeral key pair and the record salt come from random.
func encryptWakeUp(p256dh, auth string, plaintext []byte, random io.Reader) ([]byte, error) {
	receiverPublic, authSecret, err := parseSubscriptionKeys(p256dh, auth)
	if err != nil {
		return nil, err
	}
	ephemeral, err := ecdh.P256().GenerateKey(random)
	if err != nil {
		return nil, err
	}
	salt := make([]byte, 16)
	if _, err := io.ReadFull(random, salt); err != nil {
		return nil, err
	}
	return sealRecord(ephemeral, salt, receiverPublic, authSecret, plaintext)
}

// sealRecord is the deterministic core of encryptWakeUp. It is split out so the RFC 8291 Appendix A vector can
// check the exact bytes of a message.
func sealRecord(ephemeral *ecdh.PrivateKey, salt []byte, receiverPublic *ecdh.PublicKey, authSecret, plaintext []byte) ([]byte, error) {
	sharedSecret, err := ephemeral.ECDH(receiverPublic)
	if err != nil {
		return nil, err
	}
	senderPublic := ephemeral.PublicKey().Bytes()
	keyInfo := append(append([]byte("WebPush: info\x00"), receiverPublic.Bytes()...), senderPublic...)
	ikm, err := hkdf.Key(sha256.New, sharedSecret, authSecret, string(keyInfo), 32)
	if err != nil {
		return nil, err
	}
	contentKey, err := hkdf.Key(sha256.New, ikm, salt, "Content-Encoding: aes128gcm\x00", 16)
	if err != nil {
		return nil, err
	}
	nonce, err := hkdf.Key(sha256.New, ikm, salt, "Content-Encoding: nonce\x00", 12)
	if err != nil {
		return nil, err
	}
	block, err := aes.NewCipher(contentKey)
	if err != nil {
		return nil, err
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	// 0x02 marks the last record of a message. The wake-up is a single record with no extra padding.
	record := append(append([]byte{}, plaintext...), 0x02)
	ciphertext := aead.Seal(nil, nonce, record, nil)

	header := make([]byte, 0, len(salt)+4+1+len(senderPublic))
	header = append(header, salt...)
	header = binary.BigEndian.AppendUint32(header, recordSize)
	header = append(header, byte(len(senderPublic)))
	header = append(header, senderPublic...)
	return append(header, ciphertext...), nil
}

// parseSubscriptionKeys decodes the browser's p256dh point, which must be an uncompressed P-256 point, and its
// 16-byte auth secret. Anything else is an invalid subscription.
func parseSubscriptionKeys(p256dh, auth string) (*ecdh.PublicKey, []byte, error) {
	publicBytes, err := decodeBase64URL(p256dh)
	if err != nil {
		return nil, nil, ErrInvalidSubscription
	}
	receiverPublic, err := ecdh.P256().NewPublicKey(publicBytes)
	if err != nil {
		return nil, nil, ErrInvalidSubscription
	}
	authSecret, err := decodeBase64URL(auth)
	if err != nil || len(authSecret) != 16 {
		return nil, nil, ErrInvalidSubscription
	}
	return receiverPublic, authSecret, nil
}
