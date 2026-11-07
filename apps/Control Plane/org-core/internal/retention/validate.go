package retention

import (
	"fmt"
	"sort"
	"strings"
	"unicode/utf8"
)

// Error codes a validation failure can carry. They are stable: the admin view
// maps them to text.
const (
	CodeExpectedRevisionRequired = "expected_revision_required"
	CodeWindowOutOfRange         = "thread_days_out_of_range"
	CodeInvalidScope             = "invalid_scope"
	CodeInvalidThreadID          = "invalid_thread_id"
	CodeThreadIDNotAllowed       = "thread_id_not_allowed"
	CodeInvalidHoldRef           = "invalid_hold_ref"
	CodeTooManyThreadIDs         = "too_many_thread_ids"
)

// FieldError is one problem with a request. Path is a JSON path such as
// "thread_ids[3]".
type FieldError struct {
	Path    string `json:"path"`
	Code    string `json:"code"`
	Message string `json:"message"`
}

// ValidationError carries every problem found, so an admin sees them all at once.
type ValidationError struct {
	Errors []FieldError
}

func (e *ValidationError) Error() string {
	if len(e.Errors) == 0 {
		return "invalid retention request"
	}
	return fmt.Sprintf("invalid retention request: %s: %s", e.Errors[0].Path, e.Errors[0].Message)
}

func (e *ValidationError) add(path, code, message string) {
	e.Errors = append(e.Errors, FieldError{Path: path, Code: code, Message: message})
}

func (e *ValidationError) orNil() error {
	if len(e.Errors) == 0 {
		return nil
	}
	return e
}

// ValidatePut checks a policy write. A window is either null (keep) or a whole
// number of days inside MinThreadDays..MaxThreadDays; the bounds stop a typo from
// emptying an organisation (a window of 1) or reading as "keep" while deleting
// (a negative number).
func ValidatePut(req PutRequest) error {
	invalid := &ValidationError{}
	if req.ExpectedRevision == nil || *req.ExpectedRevision < 0 {
		invalid.add("expected_revision", CodeExpectedRevisionRequired, "expected_revision is required and must be zero or greater (0 for the first write)")
	}
	if req.ThreadDays != nil && (*req.ThreadDays < MinThreadDays || *req.ThreadDays > MaxThreadDays) {
		invalid.add("thread_days", CodeWindowOutOfRange, fmt.Sprintf("thread_days must be null (keep) or between %d and %d", MinThreadDays, MaxThreadDays))
	}
	return invalid.orNil()
}

// NormalizePlace checks a hold placement and returns it with the target resolved:
// an org hold targets orgID, a thread hold its thread id. The hold reference is
// trimmed; it names a matter, so it must be present and short.
func NormalizePlace(orgID string, req PlaceHoldRequest) (scope, target, holdRef string, err error) {
	invalid := &ValidationError{}
	scope = strings.TrimSpace(req.Scope)
	threadID := strings.TrimSpace(req.ThreadID)
	switch scope {
	case ScopeOrg:
		if threadID != "" {
			invalid.add("thread_id", CodeThreadIDNotAllowed, "an org hold covers the whole organisation and takes no thread_id")
		}
		target = orgID
	case ScopeThread:
		if !ValidThreadID(threadID) {
			invalid.add("thread_id", CodeInvalidThreadID, threadIDRule())
		}
		target = threadID
	default:
		invalid.add("scope", CodeInvalidScope, `scope must be "org" or "thread"`)
	}
	holdRef = strings.TrimSpace(req.HoldRef)
	if holdRef == "" || utf8.RuneCountInString(holdRef) > MaxHoldRefLength || strings.ContainsAny(holdRef, "\r\n\t") {
		invalid.add("hold_ref", CodeInvalidHoldRef, fmt.Sprintf("hold_ref is required: a matter or case reference of at most %d characters on one line", MaxHoldRefLength))
	}
	if err := invalid.orNil(); err != nil {
		return "", "", "", err
	}
	return scope, target, holdRef, nil
}

// NormalizeCheck checks the sweeper's question and returns the ids deduplicated
// in bytewise order. An empty list is allowed: it asks for the window and the org
// hold alone. The cap counts raw entries, so a caller cannot pass it by repeating.
func NormalizeCheck(req CheckRequest) ([]string, error) {
	invalid := &ValidationError{}
	if len(req.ThreadIDs) > MaxCheckThreadIDs {
		invalid.add("thread_ids", CodeTooManyThreadIDs, fmt.Sprintf("at most %d thread ids per call", MaxCheckThreadIDs))
		return nil, invalid
	}
	seen := make(map[string]struct{}, len(req.ThreadIDs))
	ids := make([]string, 0, len(req.ThreadIDs))
	for i, id := range req.ThreadIDs {
		// A check id is compared as given: trimming here would let " t1" match a
		// hold on "t1" for a caller that never sent "t1".
		if !ValidThreadID(id) {
			invalid.add(fmt.Sprintf("thread_ids[%d]", i), CodeInvalidThreadID, threadIDRule())
			continue
		}
		if _, dup := seen[id]; dup {
			continue
		}
		seen[id] = struct{}{}
		ids = append(ids, id)
	}
	if err := invalid.orNil(); err != nil {
		return nil, err
	}
	sort.Strings(ids)
	return ids, nil
}

// ValidThreadID accepts the ids session-core issues (UUIDs and prefixed ids):
// 1..MaxThreadIDLength ASCII letters, digits and "._:-", starting with a letter or
// digit. Anything else is refused rather than stored as a target nothing matches.
func ValidThreadID(id string) bool {
	if len(id) == 0 || len(id) > MaxThreadIDLength {
		return false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9':
		case i > 0 && (c == '.' || c == '_' || c == ':' || c == '-'):
		default:
			return false
		}
	}
	return true
}

func threadIDRule() string {
	return fmt.Sprintf("thread_id must be 1 to %d letters, digits or ._:- starting with a letter or digit", MaxThreadIDLength)
}
