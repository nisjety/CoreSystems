package canvas

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"strings"
	"unicode/utf8"
)

// Limits for the room canvas. Each bound is checked in this package before any row is touched. The HTTP layer applies
// MaxRequestBodyBytes with http.MaxBytesReader.
const (
	// MinContentBytes and MaxContentBytes bound one version's Markdown. The cap keeps a whole read inside one tool result.
	MinContentBytes = 1
	MaxContentBytes = 65536

	// MaxSummaryChars bounds the display-only summary, counted in characters.
	MaxSummaryChars = 200

	// MaxRequestBodyBytes is the HTTP body cap (128 KiB).
	MaxRequestBodyBytes = 128 * 1024

	// DefaultListLimit and MaxListLimit bound one page of version history.
	DefaultListLimit = 50
	MaxListLimit     = 50

	// MaxWritesPerAuthorPerHour bounds the writes one author (a user, or an agent run) makes to one canvas in a
	// rolling hour. MaxWritesPerAgentRun bounds the writes one agent run makes to one canvas.
	MaxWritesPerAuthorPerHour = 60
	MaxWritesPerAgentRun      = 20

	// MaxIdempotencyKeyBytes, MaxVersionIDBytes and MaxSpaceRefBytes bound request identifiers.
	MaxIdempotencyKeyBytes = 256
	MaxVersionIDBytes      = 128
	MaxSpaceRefBytes       = 256
)

// ValidateContent accepts 1 to MaxContentBytes bytes of valid UTF-8 with no NUL byte. Oversize is CodeTooLarge (413);
// every other failure is CodeInvalidRequest (400).
func ValidateContent(content string) error {
	switch {
	case len(content) < MinContentBytes:
		return invalidRequest("content must not be empty")
	case len(content) > MaxContentBytes:
		return &Error{Code: CodeTooLarge, Message: "content is larger than the canvas limit", MaxBytes: MaxContentBytes}
	case !utf8.ValidString(content):
		return invalidRequest("content must be valid UTF-8")
	case strings.IndexByte(content, 0) >= 0:
		return invalidRequest("content must not contain NUL bytes")
	}
	return nil
}

// NormalizeSummary trims a summary and returns nil for an empty one. It rejects invalid UTF-8, NUL bytes and more than
// MaxSummaryChars characters.
func NormalizeSummary(summary *string) (*string, error) {
	if summary == nil {
		return nil, nil
	}
	if !utf8.ValidString(*summary) || strings.IndexByte(*summary, 0) >= 0 {
		return nil, invalidRequest("summary must be valid UTF-8 without NUL bytes")
	}
	trimmed := strings.TrimSpace(*summary)
	if trimmed == "" {
		return nil, nil
	}
	if utf8.RuneCountInString(trimmed) > MaxSummaryChars {
		return nil, invalidRequest("summary must be at most 200 characters")
	}
	return &trimmed, nil
}

// ValidateIdempotencyKey requires a non-blank key of at most MaxIdempotencyKeyBytes bytes, valid UTF-8, no NUL.
func ValidateIdempotencyKey(key string) error {
	if strings.TrimSpace(key) == "" || len(key) > MaxIdempotencyKeyBytes ||
		!utf8.ValidString(key) || strings.IndexByte(key, 0) >= 0 {
		return invalidRequest("idempotency_key is required and must be at most 256 bytes")
	}
	return nil
}

// ValidateVersionID bounds a version id taken from a request body or path. Version ids are opaque text.
func ValidateVersionID(id string) error {
	if id == "" || len(id) > MaxVersionIDBytes || strings.TrimSpace(id) != id {
		return invalidRequest("version id is not valid")
	}
	return nil
}

// ClampListLimit maps a requested page size into [1, MaxListLimit]. An unset size gets DefaultListLimit.
func ClampListLimit(limit int) int {
	switch {
	case limit <= 0:
		return DefaultListLimit
	case limit > MaxListLimit:
		return MaxListLimit
	default:
		return limit
	}
}

// ContentDigest is the digest stored with a version and bound into the Control decision: "sha256:" followed by 64
// lowercase hex digits of the content bytes. The same string is produced by Control's canvas issuer.
func ContentDigest(content string) string {
	sum := sha256.Sum256([]byte(content))
	return "sha256:" + hex.EncodeToString(sum[:])
}

// idempotencyHash binds one idempotency key to its org, canvas, operation and author. Each field is length-prefixed so
// two different tuples never share an encoding. The raw key is never stored. The operation is part of the binding, so
// one key cannot replay a save as a restore or the other way round.
func idempotencyHash(orgID, canvasID, op string, author Author, key string) string {
	h := sha256.New()
	fields := []string{"space-canvas.idempotency.v1", orgID, canvasID, op, author.Kind, author.ID, key}
	for _, field := range fields {
		var size [8]byte
		binary.BigEndian.PutUint64(size[:], uint64(len(field)))
		h.Write(size[:])
		h.Write([]byte(field))
	}
	return hex.EncodeToString(h.Sum(nil))
}

// invalidRequest is the typed 400 for a request the caller got wrong.
func invalidRequest(message string) *Error {
	return &Error{Code: CodeInvalidRequest, Message: message}
}
