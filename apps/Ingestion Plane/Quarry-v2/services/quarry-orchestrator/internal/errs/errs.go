// Package errs defines the error taxonomy used by Quarry orchestrator
// activities and workflows. Errors are classified into retryable and
// non-retryable categories and, when raised from activities, wrapped as
// Temporal application errors so workflow retry policies honour the
// classification without additional logic.
package errs

import (
	"errors"
	"fmt"

	"go.temporal.io/sdk/temporal"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

// Category classifies an error for retry and event-mapping purposes.
type Category int

const (
	CategoryUnknown Category = iota
	// Retryable.
	CategoryNetwork
	CategoryTimeout
	CategoryServer5xx
	// Non-retryable.
	CategoryClient4xx
	CategoryValidation
	CategoryBlocklist
	CategoryAuth
)

// String returns a stable identifier used as the Temporal application-error
// type and as the error-category payload field in events.
func (c Category) String() string {
	switch c {
	case CategoryNetwork:
		return "network"
	case CategoryTimeout:
		return "timeout"
	case CategoryServer5xx:
		return "server_5xx"
	case CategoryClient4xx:
		return "client_4xx"
	case CategoryValidation:
		return "validation"
	case CategoryBlocklist:
		return "blocklist"
	case CategoryAuth:
		return "auth"
	default:
		return "unknown"
	}
}

// Retryable reports whether workflows should retry activities that return
// this category. Non-retryable categories propagate to the workflow as
// terminal failures.
func (c Category) Retryable() bool {
	switch c {
	case CategoryNetwork, CategoryTimeout, CategoryServer5xx:
		return true
	default:
		return false
	}
}

// Event returns the page-level event type associated with this category.
// Retryable categories map to EvtPageRetried while the workflow is still
// retrying; once attempts are exhausted the caller is responsible for
// emitting EvtPageEscalated.
func (c Category) Event() quarrycontracts.EventType {
	switch c {
	case CategoryBlocklist:
		return quarrycontracts.EvtPageBlocked
	case CategoryClient4xx, CategoryValidation, CategoryAuth:
		return quarrycontracts.EvtPageFailed
	case CategoryNetwork, CategoryTimeout, CategoryServer5xx:
		return quarrycontracts.EvtPageRetried
	default:
		return quarrycontracts.EvtPageFailed
	}
}

// Error is the orchestrator's canonical error type. It carries enough
// context to surface a page-level event payload and to drive Temporal
// retry decisions.
type Error struct {
	Category Category
	Op       string // activity/operation name, e.g. "RunPage"
	Status   int    // HTTP status when applicable; 0 otherwise
	Body     string // truncated response body for diagnostics
	Err      error
}

func (e *Error) Error() string {
	if e == nil {
		return ""
	}
	switch {
	case e.Err != nil && e.Status > 0:
		return fmt.Sprintf("%s: %s (status=%d): %v", e.Op, e.Category, e.Status, e.Err)
	case e.Err != nil:
		return fmt.Sprintf("%s: %s: %v", e.Op, e.Category, e.Err)
	case e.Status > 0:
		return fmt.Sprintf("%s: %s (status=%d)", e.Op, e.Category, e.Status)
	default:
		return fmt.Sprintf("%s: %s", e.Op, e.Category)
	}
}

func (e *Error) Unwrap() error { return e.Err }

// Temporal converts this error into a Temporal-aware error. Non-retryable
// categories are returned as NonRetryableApplicationError so retry
// policies terminate immediately; retryable categories are returned as
// plain ApplicationError with a stable Type that activities and workers
// can match on.
func (e *Error) Temporal() error {
	if e == nil {
		return nil
	}
	if !e.Category.Retryable() {
		return temporal.NewNonRetryableApplicationError(e.Error(), e.Category.String(), e.Err, map[string]any{
			"status": e.Status,
			"op":     e.Op,
		})
	}
	// Preserve the underlying cause chain. The variadic arg on
	// `NewApplicationError` is `details`, not `cause`; using the
	// `WithCause` variant keeps `errors.Is/As` working on `e.Err`
	// when callers unwrap the Temporal error.
	return temporal.NewApplicationErrorWithCause(e.Error(), e.Category.String(), e.Err, map[string]any{
		"status": e.Status,
		"op":     e.Op,
	})
}

// New builds a categorised error.
func New(cat Category, op string, err error) *Error {
	return &Error{Category: cat, Op: op, Err: err}
}

// FromHTTPStatus classifies an HTTP response. status==0 is treated as a
// network failure. 401/403 map to Auth; 429 is retryable (server-side
// throttling). Any other 4xx is non-retryable client error. 5xx is
// retryable server error. Values >= 600 fall back to Network.
func FromHTTPStatus(op string, status int, body []byte) *Error {
	const maxBody = 512
	b := string(body)
	if len(b) > maxBody {
		b = b[:maxBody]
	}
	switch {
	case status == 0:
		return &Error{Category: CategoryNetwork, Op: op, Status: 0, Body: b,
			Err: errors.New("no response")}
	case status == 401 || status == 403:
		return &Error{Category: CategoryAuth, Op: op, Status: status, Body: b,
			Err: fmt.Errorf("unauthorized (%d)", status)}
	case status == 429:
		return &Error{Category: CategoryServer5xx, Op: op, Status: status, Body: b,
			Err: fmt.Errorf("rate limited (%d)", status)}
	case status >= 400 && status < 500:
		return &Error{Category: CategoryClient4xx, Op: op, Status: status, Body: b,
			Err: fmt.Errorf("client error (%d)", status)}
	case status >= 500 && status < 600:
		return &Error{Category: CategoryServer5xx, Op: op, Status: status, Body: b,
			Err: fmt.Errorf("server error (%d)", status)}
	default:
		return &Error{Category: CategoryNetwork, Op: op, Status: status, Body: b,
			Err: fmt.Errorf("unexpected status (%d)", status)}
	}
}

// Classify extracts an *Error from an error chain, returning (nil,false)
// if the error is not a classified orchestrator error.
func Classify(err error) (*Error, bool) {
	var e *Error
	if errors.As(err, &e) {
		return e, true
	}
	return nil, false
}

// IsNonRetryable reports whether a Temporal-returned error was produced
// from a non-retryable category. Useful in workflows that receive
// application errors from activity futures.
func IsNonRetryable(err error) bool {
	if err == nil {
		return false
	}
	var appErr *temporal.ApplicationError
	if errors.As(err, &appErr) {
		return appErr.NonRetryable()
	}
	if e, ok := Classify(err); ok {
		return !e.Category.Retryable()
	}
	return false
}
