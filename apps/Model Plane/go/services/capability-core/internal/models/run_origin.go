package models

// Run origin (build plan WS20 S2, DEC-17): who started a run. The vocabulary is
// closed, and the only value a run may be stamped with under `origin` in its
// metadata. This is the Go twin of mp-contracts::run_origin; both are pinned by
// the same known answers, so a token means the same thing to the writer and to
// every reader. A run with no stamp predates the field and reads as chat.
const (
	RunOriginChat     = "chat"
	RunOriginSchedule = "schedule"
	RunOriginTrigger  = "trigger"
	RunOriginAPI      = "api"
)

// RunOriginMetadataKey is the runs.metadata key the origin is stored under.
const RunOriginMetadataKey = "origin"

// ParseRunOrigin accepts exactly one of the closed tokens. Exact match only, so
// a look-alike is refused rather than read as a chat.
func ParseRunOrigin(token string) (string, bool) {
	switch token {
	case RunOriginChat, RunOriginSchedule, RunOriginTrigger, RunOriginAPI:
		return token, true
	}
	return "", false
}

// RunOriginFromMetadata returns the origin a run's stored metadata names. An
// absent or null stamp reads as chat. A stamp that is not a closed token is
// refused (ok false), never mapped to chat.
func RunOriginFromMetadata(metadata map[string]any) (string, bool) {
	raw, present := metadata[RunOriginMetadataKey]
	if !present || raw == nil {
		return RunOriginChat, true
	}
	token, isString := raw.(string)
	if !isString {
		return "", false
	}
	return ParseRunOrigin(token)
}
