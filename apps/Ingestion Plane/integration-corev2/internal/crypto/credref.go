package crypto

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/hkdf"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"strconv"
	"strings"
	"time"
)

// Credential references (W4-CC-4, ADR-NEXT-sandbox-credential-injection section 2).
//
// A reference is the handle the injecting proxy holds in place of a bound secret. Its text form is
// `crf1.<base64url(nonce || AES-GCM ciphertext)>`, sealed under a key derived from the vault key
// with the purpose label credential-ref-v1. The ciphertext carries the scope it names (binding,
// organisation, connection, call, slot, host and expiry), so a holder cannot change that scope,
// and it holds no secret: a redemption opens the binding's own ciphertext through the vault.
//
// The scope is the GCM plaintext and the purpose label is the GCM additional data. Open receives
// only the reference, the host and the call id, so it cannot rebuild scope-shaped additional data
// before it has authenticated the scope. The tag covers the same fields either way.

const (
	credentialRefPrefix       = "crf1."
	credentialRefPurpose      = "credential-ref-v1"
	credentialRefScopeVersion = "v1"
	credentialRefScopeFields  = 8
	credentialRefFieldMax     = 256
	credentialRefMaxLength    = 8192
)

var (
	// ErrCredentialRefInvalid is any reference that does not parse or does not authenticate. Such a
	// reference names no organisation or binding.
	ErrCredentialRefInvalid = errors.New("credential reference is invalid")
	// ErrCredentialRefHostMismatch is an authentic reference presented for another host.
	ErrCredentialRefHostMismatch = errors.New("credential reference names another host")
	// ErrCredentialRefCallMismatch is an authentic reference presented for another call.
	ErrCredentialRefCallMismatch = errors.New("credential reference names another call")

	errCredentialRefScope = errors.New("credential reference scope is invalid")
)

// CredentialRefClaims is the scope a reference names. Open returns it only for an authentic
// reference.
type CredentialRefClaims struct {
	BindingID    string
	OrgID        string
	ConnectionID string
	CallID       string
	Slot         string
	Host         string
	ExpiresAt    time.Time
}

// CredentialRefs seals and opens credential references.
type CredentialRefs struct {
	gcm cipher.AEAD
}

// NewCredentialRefs derives the reference key from the vault key. Pass the vault's own encryption
// key, so references and vault ciphertexts rotate together.
func NewCredentialRefs(vaultKey []byte) (*CredentialRefs, error) {
	if len(vaultKey) == 0 {
		return nil, errors.New("credential reference key material is required")
	}
	key, err := hkdf.Key(sha256.New, vaultKey, nil, credentialRefPurpose, 32)
	if err != nil {
		return nil, fmt.Errorf("derive credential reference key: %w", err)
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("create cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("create gcm: %w", err)
	}
	return &CredentialRefs{gcm: gcm}, nil
}

// Seal returns the reference for one binding, call and slot. The connection id may be empty; the
// other fields may not. The expiry is stored to the second.
func (r *CredentialRefs) Seal(bindingID, orgID, connectionID, callID, slot, host string, expiresAt time.Time) (string, error) {
	if !credentialRefField(bindingID, false) || !credentialRefField(orgID, false) ||
		!credentialRefField(connectionID, true) || !credentialRefField(callID, false) ||
		!credentialRefField(slot, false) || !credentialRefField(host, false) {
		return "", errCredentialRefScope
	}
	if expiresAt.IsZero() {
		return "", errCredentialRefScope
	}
	scope := strings.Join([]string{
		credentialRefScopeVersion, bindingID, orgID, connectionID, callID, slot, host,
		strconv.FormatInt(expiresAt.Unix(), 10),
	}, "|")
	nonce := make([]byte, r.gcm.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return "", fmt.Errorf("generate credential reference nonce: %w", err)
	}
	sealed := r.gcm.Seal(nil, nonce, []byte(scope), []byte(credentialRefPurpose))
	payload := append(nonce, sealed...)
	return credentialRefPrefix + base64.RawURLEncoding.EncodeToString(payload), nil
}

// Open authenticates a reference and checks the host and call it is presented for. An authentic
// reference presented for another host or call returns its claims together with the mismatch
// error, so the caller can audit the refusal by id. Anything that does not authenticate is
// ErrCredentialRefInvalid with no claims. Open does not check the expiry; the caller does.
func (r *CredentialRefs) Open(ref, host, callID string) (CredentialRefClaims, error) {
	if len(ref) > credentialRefMaxLength || !strings.HasPrefix(ref, credentialRefPrefix) {
		return CredentialRefClaims{}, ErrCredentialRefInvalid
	}
	payload, err := base64.RawURLEncoding.DecodeString(strings.TrimPrefix(ref, credentialRefPrefix))
	if err != nil {
		return CredentialRefClaims{}, ErrCredentialRefInvalid
	}
	nonceSize := r.gcm.NonceSize()
	if len(payload) <= nonceSize+r.gcm.Overhead() {
		return CredentialRefClaims{}, ErrCredentialRefInvalid
	}
	scope, err := r.gcm.Open(nil, payload[:nonceSize], payload[nonceSize:], []byte(credentialRefPurpose))
	if err != nil {
		return CredentialRefClaims{}, ErrCredentialRefInvalid
	}
	fields := strings.Split(string(scope), "|")
	if len(fields) != credentialRefScopeFields || fields[0] != credentialRefScopeVersion {
		return CredentialRefClaims{}, ErrCredentialRefInvalid
	}
	expiresUnix, err := strconv.ParseInt(fields[7], 10, 64)
	if err != nil {
		return CredentialRefClaims{}, ErrCredentialRefInvalid
	}
	claims := CredentialRefClaims{
		BindingID:    fields[1],
		OrgID:        fields[2],
		ConnectionID: fields[3],
		CallID:       fields[4],
		Slot:         fields[5],
		Host:         fields[6],
		ExpiresAt:    time.Unix(expiresUnix, 0).UTC(),
	}
	if claims.Host != host {
		return claims, ErrCredentialRefHostMismatch
	}
	if claims.CallID != callID {
		return claims, ErrCredentialRefCallMismatch
	}
	return claims, nil
}

// credentialRefField admits one scope field: printable ASCII, bounded, and never the separator.
// Only the connection id may be empty.
func credentialRefField(value string, allowEmpty bool) bool {
	if value == "" {
		return allowEmpty
	}
	if len(value) > credentialRefFieldMax {
		return false
	}
	for index := 0; index < len(value); index++ {
		if value[index] <= 0x20 || value[index] >= 0x7f || value[index] == '|' {
			return false
		}
	}
	return true
}
