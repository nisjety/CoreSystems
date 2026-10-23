package chain

import (
	"crypto/ed25519"
	"encoding/base64"
	"errors"
	"fmt"
	"regexp"
	"strings"
	"time"
)

var (
	ErrNoSigningKey = errors.New("chain: no signing key is configured")
	ErrUnknownKeyID = errors.New("chain: signature key id is not in the key ring")
	ErrBadSignature = errors.New("chain: signature does not verify")
)

// keyIDPattern keeps a key id printable and free of the separators the
// AUDIT_CHAIN_VERIFY_KEYS list uses.
var keyIDPattern = regexp.MustCompile(`^[A-Za-z0-9._-]{1,64}$`)

// KeyConfig is the raw key material as it arrives from the environment.
// Private is a 32-byte Ed25519 seed or a 64-byte expanded key; Public is the
// 32-byte public key; VerifyKeys is a comma-separated list of key_id:public
// pairs for retired keys whose checkpoints and attestations must still verify.
type KeyConfig struct {
	KeyID      string
	Private    string
	Public     string
	VerifyKeys string
}

// KeyRing signs with one active key and verifies with every key it knows, so a
// rotation keeps old checkpoints verifiable: the new key becomes active and
// the retired public key moves into VerifyKeys. A ring with no private key can
// verify but not sign.
type KeyRing struct {
	activeID string
	private  ed25519.PrivateKey
	public   map[string]ed25519.PublicKey
}

// NewKeyRing validates the configuration. It does not decide whether a missing
// key is acceptable: an empty config yields a ring that can neither sign nor
// verify, and the caller refuses to start where that is not allowed.
func NewKeyRing(cfg KeyConfig) (*KeyRing, error) {
	ring := &KeyRing{public: map[string]ed25519.PublicKey{}}
	keyID := strings.TrimSpace(cfg.KeyID)
	if keyID != "" && !keyIDPattern.MatchString(keyID) {
		return nil, fmt.Errorf("chain: key id %q must match %s", keyID, keyIDPattern)
	}

	var declared ed25519.PublicKey
	if strings.TrimSpace(cfg.Public) != "" {
		raw, err := decodeKeyMaterial(cfg.Public, ed25519.PublicKeySize)
		if err != nil {
			return nil, fmt.Errorf("chain: public key: %w", err)
		}
		declared = raw
	}

	switch {
	case strings.TrimSpace(cfg.Private) != "":
		if keyID == "" {
			return nil, errors.New("chain: a private key needs a key id")
		}
		raw, err := decodeKeyMaterial(cfg.Private, ed25519.SeedSize, ed25519.PrivateKeySize)
		if err != nil {
			return nil, fmt.Errorf("chain: private key: %w", err)
		}
		if len(raw) == ed25519.SeedSize {
			ring.private = ed25519.NewKeyFromSeed(raw)
		} else {
			ring.private = ed25519.PrivateKey(raw)
		}
		derived := ring.private.Public().(ed25519.PublicKey)
		if declared != nil && !derived.Equal(declared) {
			return nil, errors.New("chain: the public key does not match the private key")
		}
		ring.activeID = keyID
		ring.public[keyID] = derived
	case declared != nil:
		if keyID == "" {
			return nil, errors.New("chain: a public key needs a key id")
		}
		ring.public[keyID] = declared
	}

	for _, entry := range strings.Split(cfg.VerifyKeys, ",") {
		entry = strings.TrimSpace(entry)
		if entry == "" {
			continue
		}
		id, material, ok := strings.Cut(entry, ":")
		id = strings.TrimSpace(id)
		if !ok || !keyIDPattern.MatchString(id) {
			return nil, errors.New("chain: a verify key must be key_id:public_key")
		}
		pub, err := decodeKeyMaterial(material, ed25519.PublicKeySize)
		if err != nil {
			return nil, fmt.Errorf("chain: verify key %q: %w", id, err)
		}
		if existing, dup := ring.public[id]; dup && !existing.Equal(ed25519.PublicKey(pub)) {
			return nil, fmt.Errorf("chain: key id %q is bound to two different public keys", id)
		}
		ring.public[id] = pub
	}
	return ring, nil
}

// decodeKeyMaterial accepts base64url without padding (what
// run-control-plane.sh writes despite the _BASE64 name) and standard base64,
// padded or not, and returns the bytes only when they have an allowed length.
func decodeKeyMaterial(raw string, sizes ...int) ([]byte, error) {
	raw = strings.TrimSpace(raw)
	for _, enc := range []*base64.Encoding{
		base64.RawURLEncoding, base64.URLEncoding, base64.StdEncoding, base64.RawStdEncoding,
	} {
		decoded, err := enc.DecodeString(raw)
		if err != nil {
			continue
		}
		for _, size := range sizes {
			if len(decoded) == size {
				return decoded, nil
			}
		}
		return nil, fmt.Errorf("is %d bytes, want %v", len(decoded), sizes)
	}
	return nil, errors.New("is not valid base64")
}

// CanSign reports whether the ring holds a private key.
func (k *KeyRing) CanSign() bool { return k != nil && k.private != nil }

// ActiveKeyID is the key id new signatures carry; empty when the ring cannot sign.
func (k *KeyRing) ActiveKeyID() string {
	if k == nil {
		return ""
	}
	return k.activeID
}

// PublicKey returns the public key registered under keyID.
func (k *KeyRing) PublicKey(keyID string) (ed25519.PublicKey, bool) {
	if k == nil {
		return nil, false
	}
	pub, ok := k.public[keyID]
	return pub, ok
}

func (k *KeyRing) sign(payload []byte) (string, []byte, error) {
	if !k.CanSign() {
		return "", nil, ErrNoSigningKey
	}
	return k.activeID, ed25519.Sign(k.private, payload), nil
}

func (k *KeyRing) verify(keyID string, payload, signature []byte) error {
	pub, ok := k.PublicKey(keyID)
	if !ok {
		return ErrUnknownKeyID
	}
	if len(signature) != ed25519.SignatureSize || !ed25519.Verify(pub, payload, signature) {
		return ErrBadSignature
	}
	return nil
}

// SignAttestation signs the head of an org chain at one moment.
func (k *KeyRing) SignAttestation(orgID string, chainSeq int64, rowHash []byte, signedAt time.Time) (string, []byte, error) {
	payload, err := AttestationPayload(orgID, chainSeq, rowHash, signedAt)
	if err != nil {
		return "", nil, err
	}
	return k.sign(payload)
}

// VerifyAttestation checks an attestation against the key id it names.
func (k *KeyRing) VerifyAttestation(orgID string, chainSeq int64, rowHash []byte, signedAt time.Time, keyID string, signature []byte) error {
	payload, err := AttestationPayload(orgID, chainSeq, rowHash, signedAt)
	if err != nil {
		return err
	}
	return k.verify(keyID, payload, signature)
}

// SignCheckpoint signs a retention checkpoint.
func (k *KeyRing) SignCheckpoint(orgID string, throughSeq int64, throughHash []byte, prunedTotal int64, createdAt time.Time) (string, []byte, error) {
	payload, err := CheckpointPayload(orgID, throughSeq, throughHash, prunedTotal, createdAt)
	if err != nil {
		return "", nil, err
	}
	return k.sign(payload)
}

// VerifyCheckpoint checks a retention checkpoint against the key id it names.
func (k *KeyRing) VerifyCheckpoint(orgID string, throughSeq int64, throughHash []byte, prunedTotal int64, createdAt time.Time, keyID string, signature []byte) error {
	payload, err := CheckpointPayload(orgID, throughSeq, throughHash, prunedTotal, createdAt)
	if err != nil {
		return err
	}
	return k.verify(keyID, payload, signature)
}
