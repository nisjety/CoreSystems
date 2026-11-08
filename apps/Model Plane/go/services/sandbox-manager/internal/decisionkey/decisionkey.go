// Package decisionkey decodes the Control Space decision verification key
// (CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64) for sandbox-manager.
//
// Control emits the key as unpadded base64url. A decoder that understood only
// that form (or only standard base64) would start some Model Plane services and
// be refused by others, so every consumer follows the same rule: unpadded
// base64url is preferred, padded base64url and standard base64 are accepted
// (the caller logs one boot warning), anything else is refused.
//
// capability-core and orchestrator-core carry the full operability package
// (boot check, fingerprint, unknown-key reporting); this copy holds only the
// decoder, kept in step with theirs, because the services are separate Go
// modules.
package decisionkey

import (
	"crypto/ed25519"
	"encoding/base64"
	"errors"
	"fmt"
	"strings"
)

// DecodePublicKey decodes a base64 Ed25519 public key. Unpadded URL-safe base64
// (the form Control emits) is tried first; padded URL-safe and standard base64
// are accepted as fallbacks because they decode to the same 32 bytes. canonical
// reports whether the value was in the unpadded URL-safe form. The error never
// includes the value.
func DecodePublicKey(raw string) (key ed25519.PublicKey, canonical bool, err error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return nil, false, errors.New("public key is empty")
	}
	encodings := []struct {
		encoding  *base64.Encoding
		canonical bool
	}{
		{base64.RawURLEncoding, true},
		{base64.URLEncoding, false},
		{base64.StdEncoding, false},
		{base64.RawStdEncoding, false},
	}
	for _, candidate := range encodings {
		decoded, decodeErr := candidate.encoding.DecodeString(raw)
		if decodeErr != nil {
			continue
		}
		if len(decoded) != ed25519.PublicKeySize {
			return nil, false, fmt.Errorf("public key decodes to %d bytes, want %d", len(decoded), ed25519.PublicKeySize)
		}
		return ed25519.PublicKey(decoded), candidate.canonical, nil
	}
	return nil, false, errors.New("public key is not valid base64 or base64url")
}
