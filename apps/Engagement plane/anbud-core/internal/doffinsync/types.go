// Package doffinsync is the sync engine for the public Doffin corpus. It decides
// which days to search and which notices to download, and it records what
// happened through four small consumer-side store interfaces. It imports no
// database package: internal/rawstore implements the interfaces.
//
// The engine is sequential. One subscription key is one budget, so one run
// makes one call at a time.
package doffinsync

import "time"

// SourceDoffin is the source label on every row this package writes.
const SourceDoffin = "doffin"

// Mode selects what a run does with each day it visits.
type Mode string

const (
	// ModeSync searches and downloads every day that is not done.
	ModeSync Mode = "sync"
	// ModeCensus searches each day that is not done and downloads nothing.
	ModeCensus Mode = "census"
)

// DayState is the sync_days.state vocabulary of migration 0002.
type DayState string

const (
	StateRunning     DayState = "running"
	StateSearched    DayState = "searched"
	StateDone        DayState = "done"
	StateCapped      DayState = "capped"
	StatePartial     DayState = "partial"
	StateFailed      DayState = "failed"
	StatePaused      DayState = "paused"
	StateInterrupted DayState = "interrupted"
)

// ErrorCode is one of the sync_days.error_code values of migration 0002.
type ErrorCode string

const (
	CodeSaturated         ErrorCode = "saturated"
	CodeCountMismatch     ErrorCode = "count_mismatch"
	CodeCountChanged      ErrorCode = "count_changed"
	CodeDownloadLimit     ErrorCode = "download_limit"
	CodeNotFoundGrace     ErrorCode = "not_found_grace"
	CodeItemRejected      ErrorCode = "item_rejected"
	CodeConsecutiveErrors ErrorCode = "consecutive_errors"
	CodeRateLimited       ErrorCode = "rate_limited"
	CodeUnauthorized      ErrorCode = "unauthorized"
	CodeInvalidParams     ErrorCode = "invalid_params"
	CodeSearchFailed      ErrorCode = "search_failed"
	CodeDecodeError       ErrorCode = "decode_error"
	CodeStoreFailed       ErrorCode = "store_failed"
	CodeCancelled         ErrorCode = "cancelled"
)

// ResolutionKind is the verdict of the notice store on one id.
type ResolutionKind int

const (
	// ResolutionHeld means a raw row for the id exists. It is never downloaded again.
	ResolutionHeld ResolutionKind = iota + 1
	// ResolutionNotFound means the id's most recent definitive attempt was a 404.
	ResolutionNotFound
	// ResolutionRejected means the id's most recent definitive attempt was refused
	// for its content (too large, or an empty body). Whether Doffin is asked again
	// is the engine's decision, from Resolution.Rejections and LastRejection.
	ResolutionRejected
)

// Resolution is what the notice store knows about an id: facts only, no policy.
// The engine applies the retry policy with its own clock (RejectCooloff and
// MaxRejections).
type Resolution struct {
	Kind ResolutionKind
	// Rejections is the number of rejected attempts (too_large or empty_body) the
	// id has ever had. LastRejection is the attempted_at of the latest one. Both
	// are only meaningful for ResolutionRejected.
	Rejections    int
	LastRejection time.Time
}

// NotFoundGrace is how old a day must be before a 404 on one of its notices is
// terminal. Younger days leave the notice unresolved and retry it next run.
const NotFoundGrace = 7 * 24 * time.Hour

// RejectCooloff is how old an id's latest rejection must be before the engine asks
// Doffin for that id again. A younger rejection leaves the id cooling: unresolved,
// not downloaded, and no attempt is recorded.
const RejectCooloff = 24 * time.Hour

// MaxRejections is the rejection count after which an id is terminal: never
// downloaded again, and counted as rejected in its own bucket, so its day can be
// done. An id refused now whose total reaches it is terminal in the same run.
const MaxRejections = 3

// Reason is the outcome of a run.
type Reason string

const (
	ReasonDone        Reason = "done"
	ReasonPartial     Reason = "partial"
	ReasonPaused      Reason = "paused"
	ReasonKeyRejected Reason = "key_rejected"
	ReasonCancelled   Reason = "cancelled"
	ReasonFailed      Reason = "failed"
)

// Attempt outcomes, exactly the download_attempts.outcome list of migration 0002.
const (
	OutcomeStored         = "stored"
	OutcomeDuplicate      = "duplicate"
	OutcomeNotFound       = "not_found"
	OutcomeInvalidID      = "invalid_id"
	OutcomeHTTPError      = "http_error"
	OutcomeEmptyBody      = "empty_body"
	OutcomeTooLarge       = "too_large"
	OutcomeTransportError = "transport_error"
	OutcomeRateLimited    = "rate_limited"
	OutcomeUnauthorized   = "unauthorized"
)

// SearchPage is one search response, kept verbatim. Body is never empty.
type SearchPage struct {
	QueryKey    string // "issueDate=2026-10-07;perPage=100;sort=PUBLICATION_DATE_ASC"
	Page        int
	ContentType string
	Body        []byte
	FetchedAt   time.Time
}

// Attempt is one download outcome that the store does not record itself. A
// stored or duplicate outcome is recorded inside StoreNotice.
type Attempt struct {
	DoffinID   string
	Outcome    string // exactly the download_attempts.outcome list
	HTTPStatus int    // 0 when none
	Message    string // fixed text, or the client's sanitised message; at most 256 characters
	At         time.Time
}

// RunStart opens a run. From and To are UTC midnights.
type RunStart struct {
	Mode Mode
	From time.Time
	To   time.Time
}

// Counters are the run totals. They are written once, when the run finishes.
type Counters struct {
	Searches, Downloads, Stored, Duplicates, HeldSkipped, NotFound int
}

// RunOutcome is written when a run finishes. LastError is fixed text only.
type RunOutcome struct {
	Reason          Reason
	LastError       string
	RetryAfterUntil *time.Time
	Counters        Counters
}

// DayRecord is the state of one day as written by FinishDay.
type DayRecord struct {
	Day        time.Time // UTC midnight
	RunID      int64     // the run that began the day; a ledger keeps the write only for that run
	State      DayState
	ErrorCode  ErrorCode // empty unless the state needs one
	Total      *int64    // nil until a search succeeded
	Accessible *int64
	Held       int
	Stored     int
	NotFound   int
	Rejected   int // terminal rejections: refused for their content MaxRejections times
	Unresolved int
}

// DayOutcome is the report line for one day. It carries no body, no id list and
// no contact field.
type DayOutcome struct {
	Day        time.Time
	State      DayState
	ErrorCode  ErrorCode
	Total      int64
	Accessible int64
	Held       int
	Stored     int
	NotFound   int
	Rejected   int
	Unresolved int
	Searches   int // the searches this day took, a census reads the sweeps from it
}

// Summary is what Run reports. It carries counts, states and ids only.
type Summary struct {
	RunID           int64
	Reason          Reason
	Counters        Counters
	Days            []DayOutcome
	RetryAfterUntil *time.Time
}
