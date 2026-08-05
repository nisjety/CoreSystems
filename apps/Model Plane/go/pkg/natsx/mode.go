// Package natsx — mode.go defines compatibility modes that control how the
// Publisher and Subscriber wrappers bridge between the legacy subject tree
// (verevon.*, aqencia.*) and the new mp.v1.* namespace.
//
// Modes are selected via the MP_COMPAT_MODE environment variable:
//
//	v1_only     (default) — publish/subscribe only mp.v1.*
//	dual_write            — publish to both v1 and legacy; subscribe v1
//	dual_read             — publish v1; subscribe both and dedup by envelope ID
//	legacy_only           — publish/subscribe only legacy (emergency rollback)
package natsx

import "os"

// CompatMode controls NATS publish/subscribe bridging behavior.
type CompatMode int

const (
	// ModeV1Only publishes and subscribes only to mp.v1.* subjects.
	ModeV1Only CompatMode = iota
	// ModeDualWrite publishes to both v1 and legacy; subscribes to v1 only.
	ModeDualWrite
	// ModeDualRead publishes v1 only; subscribes to both v1 and legacy,
	// deduplicating by envelope EventID.
	ModeDualRead
	// ModeLegacyOnly publishes and subscribes only to legacy subjects
	// (emergency rollback path).
	ModeLegacyOnly
)

// String returns the canonical string form of the mode.
func (m CompatMode) String() string {
	switch m {
	case ModeV1Only:
		return "v1_only"
	case ModeDualWrite:
		return "dual_write"
	case ModeDualRead:
		return "dual_read"
	case ModeLegacyOnly:
		return "legacy_only"
	default:
		return "v1_only"
	}
}

// ParseCompatMode parses a string value (as set in MP_COMPAT_MODE) into a
// CompatMode. Unknown or empty values fall back to ModeV1Only.
func ParseCompatMode(s string) CompatMode {
	switch s {
	case "dual_write":
		return ModeDualWrite
	case "dual_read":
		return ModeDualRead
	case "legacy_only":
		return ModeLegacyOnly
	case "v1_only", "":
		return ModeV1Only
	default:
		return ModeV1Only
	}
}

// ReadCompatModeFromEnv reads MP_COMPAT_MODE from the environment.
// Defaults to ModeV1Only when unset or invalid.
func ReadCompatModeFromEnv() CompatMode {
	return ParseCompatMode(os.Getenv("MP_COMPAT_MODE"))
}
