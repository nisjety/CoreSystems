package org

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"encoding/base64"
	"errors"
	"fmt"
	"regexp"
	"strings"
)

const (
	// SIEMSealKeyEnv holds the 32-byte AES-256 key that seals destination
	// secrets at rest; SIEMSealKeyIDEnv names it so a later key can be told
	// apart from the one that sealed an existing row.
	SIEMSealKeyEnv   = "ORG_CORE_SIEM_SEAL_KEY"
	SIEMSealKeyIDEnv = "ORG_CORE_SIEM_SEAL_KEY_ID"

	defaultSIEMSealKeyID = "v1"
	siemSealKeyBytes     = 32
)

var siemSealKeyIDPattern = regexp.MustCompile(`^[A-Za-z0-9._-]{1,32}$`)

// ErrSIEMSealUnavailable is returned by a sealer that was not configured.
var ErrSIEMSealUnavailable = errors.New("siem secret sealing is not configured")

// SIEMSealer encrypts destination secrets with AES-256-GCM. The secret is
// write-only: nothing but the exporter read ever opens it, and that read is
// behind its own scope.
type SIEMSealer struct {
	keyID string
	aead  cipher.AEAD
}

// NewSIEMSealer builds a sealer from a raw 32-byte key.
func NewSIEMSealer(keyID string, key []byte) (*SIEMSealer, error) {
	if !siemSealKeyIDPattern.MatchString(keyID) {
		return nil, errors.New("siem seal key id must be 1 to 32 characters of letters, digits, '.', '_' or '-'")
	}
	if len(key) != siemSealKeyBytes {
		return nil, fmt.Errorf("siem seal key must be exactly %d bytes", siemSealKeyBytes)
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("siem seal cipher: %w", err)
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("siem seal gcm: %w", err)
	}
	return &SIEMSealer{keyID: keyID, aead: aead}, nil
}

// NewSIEMSealerFromEnv reads ORG_CORE_SIEM_SEAL_KEY and its key id through
// getenv. run-control-plane.sh writes standard padded base64; the unpadded and
// URL-safe forms are accepted so a hand-set key does not fail on an encoding
// choice. The key itself is never included in an error.
func NewSIEMSealerFromEnv(getenv func(string) string) (*SIEMSealer, error) {
	raw := strings.TrimSpace(getenv(SIEMSealKeyEnv))
	if raw == "" {
		return nil, fmt.Errorf("%s is required", SIEMSealKeyEnv)
	}
	var key []byte
	for _, encoding := range []*base64.Encoding{
		base64.StdEncoding, base64.RawStdEncoding, base64.URLEncoding, base64.RawURLEncoding,
	} {
		decoded, err := encoding.DecodeString(raw)
		if err == nil && len(decoded) == siemSealKeyBytes {
			key = decoded
			break
		}
	}
	if key == nil {
		return nil, fmt.Errorf("%s must be base64 of exactly %d bytes", SIEMSealKeyEnv, siemSealKeyBytes)
	}
	keyID := strings.TrimSpace(getenv(SIEMSealKeyIDEnv))
	if keyID == "" {
		keyID = defaultSIEMSealKeyID
	}
	return NewSIEMSealer(keyID, key)
}

// KeyID is the id stored beside every ciphertext this sealer produces.
func (s *SIEMSealer) KeyID() string {
	if s == nil {
		return ""
	}
	return s.keyID
}

// siemSealAAD binds a ciphertext to its organisation, destination and key id,
// so a sealed secret copied onto another row does not open.
func siemSealAAD(orgID, destinationID, keyID string) []byte {
	return []byte("siem-destination\x00" + orgID + "\x00" + destinationID + "\x00" + keyID)
}

// Seal returns nonce || ciphertext.
func (s *SIEMSealer) Seal(orgID, destinationID string, secret []byte) ([]byte, error) {
	if s == nil || s.aead == nil {
		return nil, ErrSIEMSealUnavailable
	}
	nonce := make([]byte, s.aead.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return nil, fmt.Errorf("siem seal nonce: %w", err)
	}
	return s.aead.Seal(nonce, nonce, secret, siemSealAAD(orgID, destinationID, s.keyID)), nil
}

// Open reverses Seal. A ciphertext sealed under another key id is refused
// rather than tried, so a key rotation fails loudly instead of as a bad tag.
func (s *SIEMSealer) Open(keyID, orgID, destinationID string, sealed []byte) ([]byte, error) {
	if s == nil || s.aead == nil {
		return nil, ErrSIEMSealUnavailable
	}
	if keyID != s.keyID {
		return nil, fmt.Errorf("siem secret was sealed under key id %q, not %q", keyID, s.keyID)
	}
	if len(sealed) < s.aead.NonceSize() {
		return nil, errors.New("siem sealed secret is truncated")
	}
	nonce, ciphertext := sealed[:s.aead.NonceSize()], sealed[s.aead.NonceSize():]
	secret, err := s.aead.Open(nil, nonce, ciphertext, siemSealAAD(orgID, destinationID, keyID))
	if err != nil {
		return nil, errors.New("siem sealed secret did not authenticate")
	}
	return secret, nil
}
