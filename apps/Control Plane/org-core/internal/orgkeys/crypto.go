// Package orgkeys is org-core's key custody for data other planes encrypt
// (WS17.E2, X-8). The hierarchy is: a master key (from the environment now, a
// KMS later) wraps one key-encryption key (KEK) per organisation, stored wrapped
// in org-core; each KEK wraps per-object data keys (DEKs) that the caller keeps
// wrapped beside its ciphertext. A caller never holds a KEK. Destroying an org's
// KEKs (the crypto-shred route, or the FK cascade of the org hard delete) makes
// every DEK wrapped under them unrecoverable.
//
// Nothing in this package logs key material, wrapped or not, and no error it
// returns carries any.
package orgkeys

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"errors"
	"fmt"
	"io"
)

const (
	// KeySize is the size of every key in the hierarchy: AES-256.
	KeySize = 32
	// nonceSize is the GCM standard nonce. Nonces are random per seal.
	nonceSize = 12
	// gcmTagSize is the GCM authentication tag appended to every ciphertext.
	gcmTagSize = 16
	// Algorithm is the JOSE name of the DEK wrap, returned to callers.
	Algorithm = "A256GCM"
)

// errOpen is the one error an open returns, whatever the cause (wrong key,
// wrong associated data, tampered or truncated blob): the caller cannot tell
// them apart and neither can an attacker.
var errOpen = errors.New("orgkeys: open failed")

var randReader io.Reader = rand.Reader

// newKey returns KeySize random bytes.
func newKey() ([]byte, error) {
	key := make([]byte, KeySize)
	if _, err := io.ReadFull(randReader, key); err != nil {
		return nil, fmt.Errorf("orgkeys: random key: %w", err)
	}
	return key, nil
}

// seal returns nonce(12) || AES-256-GCM(key, plaintext, aad), the pattern of
// integration-corev2's credential vault.
func seal(key, plaintext, aad []byte) ([]byte, error) {
	if len(key) != KeySize {
		return nil, fmt.Errorf("orgkeys: seal key must be %d bytes", KeySize)
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("orgkeys: cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("orgkeys: gcm: %w", err)
	}
	nonce := make([]byte, nonceSize, nonceSize+len(plaintext)+gcm.Overhead())
	if _, err := io.ReadFull(randReader, nonce); err != nil {
		return nil, fmt.Errorf("orgkeys: random nonce: %w", err)
	}
	return gcm.Seal(nonce, nonce, plaintext, aad), nil
}

// open reverses seal. Every failure is errOpen.
func open(key, sealed, aad []byte) ([]byte, error) {
	if len(key) != KeySize || len(sealed) < nonceSize+gcmTagSize {
		return nil, errOpen
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, errOpen
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, errOpen
	}
	plaintext, err := gcm.Open(nil, sealed[:nonceSize], sealed[nonceSize:], aad)
	if err != nil {
		return nil, errOpen
	}
	return plaintext, nil
}

// zero overwrites key material once it is no longer needed. Best effort: the Go
// runtime may have copied the slice, but the buffers this package owns do not
// linger.
func zero(b []byte) {
	for i := range b {
		b[i] = 0
	}
}
