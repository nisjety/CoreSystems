// Package decisionkey is the operability layer around the Control Space
// decision verification key (CONTROL_SPACE_DECISION_KEY_ID /
// CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64) -- finding F11.
//
// In the W1 live proof a stale key id in the Model Plane deploy env overrode the
// Control ledger, so every Control-signed decision named a key id this service
// did not trust and was refused with a bare, generic "untrusted ... key"
// error. The refusal is correct and stays exactly as it was; what was missing
// is any signal that explained it. This package adds that signal and nothing
// else:
//
//   - BootCheck logs the configured key id with a short public-key fingerprint
//     (never the key itself) and refuses to start when a key is configured but
//     cannot be parsed.
//   - ReportUnknownKey is called by a verifier when a token names a key id other
//     than the configured one. It error-logs both ids, once a minute per token
//     id, so a stale key is visible instead of silent.
//   - DecodePublicKey decodes URL-safe base64 first. Control emits base64url
//     without padding; a decoder that only understood standard base64 would
//     silently disable the whole decision lane.
//
// capability-core carries a byte-for-byte equivalent of this package (the two
// services are separate Go modules); keep them in step.
package decisionkey

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"time"
)

const (
	// unknownKeyLogInterval is the window in which one token key id logs at most once.
	unknownKeyLogInterval = time.Minute
	// unknownKeyLimiterCapacity bounds the ids remembered by the limiter. Token key
	// ids are caller-controlled, so the map must not grow with them; at the cap,
	// entries older than the window are dropped and, if none are, the new id is not
	// logged.
	unknownKeyLimiterCapacity = 64
	// loggedKeyIDMaxChars is the longest key id echoed into a log line.
	loggedKeyIDMaxChars = 64
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

// Fingerprint is a short, stable, non-reversible tag for a public key: the first
// 8 bytes of sha256(raw public key) as lowercase hex. It matches the fingerprint
// session-core and scripts/check-control-decision-key.sh print, so two
// deployments' keys can be compared by eye.
func Fingerprint(key ed25519.PublicKey) string {
	digest := sha256.Sum256(key)
	return hex.EncodeToString(digest[:8])
}

// SanitizeKeyID makes a caller-controlled key id safe to log: control characters
// become '?' and the length is capped.
func SanitizeKeyID(raw string) string {
	runes := []rune(raw)
	truncated := len(runes) > loggedKeyIDMaxChars
	if truncated {
		runes = runes[:loggedKeyIDMaxChars]
	}
	for i, r := range runes {
		if r < 0x20 || r == 0x7f || (r >= 0x80 && r < 0xa0) {
			runes[i] = '?'
		}
	}
	out := string(runes)
	if truncated {
		out += "..."
	}
	return out
}

// Limiter admits one log line per key id per window.
type Limiter struct {
	window   time.Duration
	capacity int
	mu       sync.Mutex
	last     map[string]time.Time
}

// NewLimiter returns a limiter with the given window and distinct-id capacity.
func NewLimiter(window time.Duration, capacity int) *Limiter {
	return &Limiter{window: window, capacity: capacity, last: make(map[string]time.Time)}
}

// Allow reports whether a line for id may be emitted at now, and records it when
// it may.
func (l *Limiter) Allow(id string, now time.Time) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	if last, ok := l.last[id]; ok {
		if now.Sub(last) < l.window {
			return false
		}
		l.last[id] = now
		return true
	}
	if len(l.last) >= l.capacity {
		for key, last := range l.last {
			if now.Sub(last) >= l.window {
				delete(l.last, key)
			}
		}
		if len(l.last) >= l.capacity {
			return false
		}
	}
	l.last[id] = now
	return true
}

var unknownKeyLimiter = NewLimiter(unknownKeyLogInterval, unknownKeyLimiterCapacity)

// ReportUnknownKey error-logs, rate limited per token key id, that a decision
// named a key id this service does not trust. It does not change the caller's
// refusal. component names the verifier ("cron", "model-action-view", ...).
func ReportUnknownKey(component, tokenKeyID, configuredKeyID string) {
	reportUnknownKey(slog.Default(), unknownKeyLimiter, time.Now(), component, tokenKeyID, configuredKeyID)
}

func reportUnknownKey(logger *slog.Logger, limiter *Limiter, now time.Time, component, tokenKeyID, configuredKeyID string) {
	if !limiter.Allow(tokenKeyID, now) {
		return
	}
	logger.Error("Control Space decision signed with a key id this service does not trust; the decision is refused. "+
		"The deploy env (CONTROL_SPACE_DECISION_KEY_ID / CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64) "+
		"probably carries a stale key that differs from the Control ledger",
		"verifier", component,
		"token_key_id", SanitizeKeyID(tokenKeyID),
		"configured_key_id", SanitizeKeyID(configuredKeyID),
	)
}

// BootCheck validates the Control decision key configuration at start-up. With
// neither value set it warns (every Space decision will be refused) and returns
// nil. With any value set it requires a non-empty key id and a parseable public
// key, logs the id with the key's fingerprint, and otherwise returns an error the
// caller must treat as fatal. The key itself is never logged.
func BootCheck(logger *slog.Logger, component, keyID, publicKeyBase64 string) error {
	keyID = strings.TrimSpace(keyID)
	publicKeyBase64 = strings.TrimSpace(publicKeyBase64)
	if keyID == "" && publicKeyBase64 == "" {
		logger.Warn("no Control Space decision verification key is configured; every Space decision will be refused",
			"component", component)
		return nil
	}
	if keyID == "" {
		return errors.New("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 is set but CONTROL_SPACE_DECISION_KEY_ID is empty")
	}
	if publicKeyBase64 == "" {
		return errors.New("CONTROL_SPACE_DECISION_KEY_ID is set but CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 is empty")
	}
	key, canonical, err := DecodePublicKey(publicKeyBase64)
	if err != nil {
		return fmt.Errorf("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 is unusable: %w", err)
	}
	logger.Info("Control Space decision verification key configured (id=public-key fingerprint)",
		"component", component,
		"decision_key_id", SanitizeKeyID(keyID),
		"decision_key_fingerprint", Fingerprint(key),
	)
	if !canonical {
		logger.Warn("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 is not unpadded base64url; it was accepted, but Control emits unpadded base64url and other planes may refuse this form",
			"component", component)
	}
	return nil
}
