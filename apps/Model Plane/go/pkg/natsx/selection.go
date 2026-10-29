package natsx

import (
	"fmt"
	"strings"
)

// SubjectSelectionError indicates a canonical subject has no legacy mapping
// and cannot be served in LegacyOnly mode.
type SubjectSelectionError struct {
	Subject string
}

func (e *SubjectSelectionError) Error() string {
	return fmt.Sprintf("no legacy mapping for subject %q", e.Subject)
}

// SubscriberSubjects returns the subjects a subscriber should listen on for
// a given canonical subject under the specified compatibility mode.
//
// Subjects outside the mp.v1 prefix are passed through unchanged.
// Parity contract: matches Rust mp_events::subjects::subscriber_subjects.
func SubscriberSubjects(canonical string, mode CompatMode) ([]string, error) {
	if !strings.HasPrefix(canonical, Prefix) {
		return []string{canonical}, nil
	}

	switch mode {
	case ModeV1Only, ModeDualWrite:
		return []string{canonical}, nil
	case ModeDualRead:
		legacy := TranslateNewToLegacy(canonical)
		if legacy == "" {
			return []string{canonical}, nil
		}
		return []string{canonical, legacy}, nil
	case ModeLegacyOnly:
		legacy := TranslateNewToLegacy(canonical)
		if legacy == "" {
			return nil, &SubjectSelectionError{Subject: canonical}
		}
		return []string{legacy}, nil
	}
	return []string{canonical}, nil
}
