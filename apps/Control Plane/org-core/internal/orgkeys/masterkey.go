package orgkeys

import (
	"bytes"
	"context"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"sort"
	"strings"
)

const (
	// EnvEnabled mounts the key routes (default off: the image ships dark).
	EnvEnabled = "ORG_CORE_KEYS_ENABLED"
	// EnvMasterKeys holds the master keys as JSON {"v1":"<base64 of 32 bytes>"}.
	// X-8 interim: the key comes from the environment (the secrets ledger) now, a
	// KMS later behind MasterKeyProvider. Losing it destroys every wrapped KEK.
	EnvMasterKeys = "ORG_CORE_KEY_MASTER_KEYS"
	// EnvMasterActive names the master key new KEKs are wrapped under.
	EnvMasterActive = "ORG_CORE_KEY_MASTER_ACTIVE"

	maxMasterKeys = 16
	// canaryPlaintext is the known plaintext stored sealed under each master id.
	// Opening it proves the configured key is the one the KEKs were wrapped with
	// before any route serves.
	canaryPlaintext = "org-core.orgkeys.canary.v1"
)

var masterKeyIDPattern = regexp.MustCompile(`^[a-z0-9][a-z0-9._-]{0,31}$`)

// Enabled reports whether ORG_CORE_KEYS_ENABLED is on ("1" or "true").
func Enabled(getenv func(string) string) bool {
	value := strings.TrimSpace(getenv(EnvEnabled))
	return value == "1" || strings.EqualFold(value, "true")
}

// MasterKeyProvider is the root of the hierarchy. EnvMasterKeyProvider is the interim
// implementation; a KMS-backed provider replaces it without touching the store
// or the routes.
type MasterKeyProvider interface {
	// ActiveID names the key new KEKs are wrapped under.
	ActiveID() string
	// Key returns a copy of the key with this id, or false when it is not
	// configured. The caller zeroes the copy.
	Key(id string) ([]byte, bool)
	// IDs lists every configured id, sorted.
	IDs() []string
}

// EnvMasterKeyProvider reads the master keys from the environment.
type EnvMasterKeyProvider struct {
	active string
	keys   map[string][]byte
}

// LoadEnvMasterKeys parses ORG_CORE_KEY_MASTER_KEYS and ORG_CORE_KEY_MASTER_ACTIVE.
// No error it returns contains a key value.
func LoadEnvMasterKeys(getenv func(string) string) (*EnvMasterKeyProvider, error) {
	raw := strings.TrimSpace(getenv(EnvMasterKeys))
	if raw == "" {
		return nil, fmt.Errorf("%s is not set", EnvMasterKeys)
	}
	var encoded map[string]string
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&encoded); err != nil {
		return nil, fmt.Errorf("%s must be a JSON object of id to base64 key", EnvMasterKeys)
	}
	if len(encoded) == 0 || len(encoded) > maxMasterKeys {
		return nil, fmt.Errorf("%s must hold between 1 and %d keys", EnvMasterKeys, maxMasterKeys)
	}
	keys := make(map[string][]byte, len(encoded))
	for id, value := range encoded {
		if !masterKeyIDPattern.MatchString(id) {
			return nil, fmt.Errorf("%s has an invalid key id", EnvMasterKeys)
		}
		key, err := decodeKey(value)
		if err != nil {
			return nil, fmt.Errorf("%s key %s: %w", EnvMasterKeys, id, err)
		}
		for otherID, other := range keys {
			if subtle.ConstantTimeCompare(other, key) == 1 {
				return nil, fmt.Errorf("%s keys %s and %s are identical", EnvMasterKeys, otherID, id)
			}
		}
		keys[id] = key
	}
	active := strings.TrimSpace(getenv(EnvMasterActive))
	if active == "" {
		return nil, fmt.Errorf("%s is not set", EnvMasterActive)
	}
	if _, ok := keys[active]; !ok {
		return nil, fmt.Errorf("%s names a key that %s does not hold", EnvMasterActive, EnvMasterKeys)
	}
	return &EnvMasterKeyProvider{active: active, keys: keys}, nil
}

// decodeKey accepts standard or URL-safe base64, padded or not, and requires
// exactly KeySize bytes that are not all zero.
func decodeKey(value string) ([]byte, error) {
	value = strings.TrimSpace(value)
	var key []byte
	var err error
	for _, encoding := range []*base64.Encoding{base64.StdEncoding, base64.RawStdEncoding, base64.URLEncoding, base64.RawURLEncoding} {
		key, err = encoding.DecodeString(value)
		if err == nil {
			break
		}
	}
	if err != nil {
		return nil, errors.New("is not base64")
	}
	if len(key) != KeySize {
		zero(key)
		return nil, fmt.Errorf("must decode to %d bytes", KeySize)
	}
	if bytes.Equal(key, make([]byte, KeySize)) {
		return nil, errors.New("must not be all zero")
	}
	return key, nil
}

func (m *EnvMasterKeyProvider) ActiveID() string { return m.active }

func (m *EnvMasterKeyProvider) Key(id string) ([]byte, bool) {
	key, ok := m.keys[id]
	if !ok {
		return nil, false
	}
	return append([]byte(nil), key...), true
}

func (m *EnvMasterKeyProvider) IDs() []string {
	ids := make([]string, 0, len(m.keys))
	for id := range m.keys {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	return ids
}

// ValidateConfig is the boot assertion: when the key routes are on, the master
// keys must parse, or org-core refuses to start rather than serve 503 on every
// call. With the routes off it checks nothing.
func ValidateConfig(getenv func(string) string) error {
	if !Enabled(getenv) {
		return nil
	}
	if _, err := LoadEnvMasterKeys(getenv); err != nil {
		return fmt.Errorf("%s is on but the master keys are not usable: %w", EnvEnabled, err)
	}
	return nil
}

func canaryAAD(masterID string) []byte {
	return []byte("org-core.canary|master=" + masterID)
}

// checkCanaries proves the configured master keys before any route serves. Every
// stored canary must open under the key of its id (a missing or wrong key is
// ErrUnavailable, never a decrypt error on a caller's request), and a configured
// key with no canary yet gets one.
func checkCanaries(ctx context.Context, provider MasterKeyProvider, store Store) error {
	stored, err := store.Canaries(ctx)
	if err != nil {
		return fmt.Errorf("%w: read canaries: %v", ErrUnavailable, err)
	}
	for id, sealed := range stored {
		if err := verifyCanary(provider, id, sealed); err != nil {
			return err
		}
	}
	for _, id := range provider.IDs() {
		if _, ok := stored[id]; ok {
			continue
		}
		key, ok := provider.Key(id)
		if !ok {
			return fmt.Errorf("%w: master key %s vanished", ErrUnavailable, id)
		}
		sealed, sealErr := seal(key, []byte(canaryPlaintext), canaryAAD(id))
		zero(key)
		if sealErr != nil {
			return fmt.Errorf("%w: seal canary: %v", ErrUnavailable, sealErr)
		}
		// Insert-if-absent: a concurrent instance may have written its own first;
		// whichever row won must open under this key.
		winner, err := store.PutCanary(ctx, id, sealed)
		if err != nil {
			return fmt.Errorf("%w: store canary: %v", ErrUnavailable, err)
		}
		if err := verifyCanary(provider, id, winner); err != nil {
			return err
		}
	}
	return nil
}

func verifyCanary(provider MasterKeyProvider, id string, sealed []byte) error {
	key, ok := provider.Key(id)
	if !ok {
		return fmt.Errorf("%w: master key %s has a stored canary but is not configured", ErrUnavailable, id)
	}
	defer zero(key)
	plaintext, err := open(key, sealed, canaryAAD(id))
	if err != nil || string(plaintext) != canaryPlaintext {
		return fmt.Errorf("%w: master key %s does not open its canary", ErrUnavailable, id)
	}
	return nil
}
