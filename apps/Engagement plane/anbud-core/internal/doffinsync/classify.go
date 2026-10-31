package doffinsync

import (
	"errors"
	"fmt"
	"net/http"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// Fixed texts. A stored or reported field never carries an error's own text,
// which can embed request URLs and keys (T41). Only the client's sanitised
// message is used, and only for an http_error attempt.
const (
	msgCancelled      = "run cancelled"
	msgKeyRejected    = "subscription key rejected"
	msgRateLimited    = "upstream rate limit reached"
	msgSearchRejected = "search request rejected by upstream"
	msgConsecutive    = "consecutive upstream errors"
	msgDownloadLimit  = "download limit reached"
	msgGaps           = "days remain incomplete"
	msgStore          = "storing a response failed"
	msgLedger         = "writing the sync ledger failed"

	msgEmptyBody = "empty response body"
	msgTooLarge  = "response exceeds the size cap"
	msgTransport = "transport error"
	msgHTTPError = "upstream returned an error status"
	msgNotFound  = "notice not found"
)

// maxAttemptMessage is the download_attempts.message bound of migration 0002.
const maxAttemptMessage = 256

// attemptFor maps a download error to its download_attempts outcome, HTTP status
// and fixed or sanitised message (table D). It never returns err.Error().
// doffin.ErrInvalidParams has no mapping: downloadFailed quarantines a refused
// id before it reaches here, because no attempt happened (D-A).
func attemptFor(err error) (outcome string, status int, message string) {
	var rl *doffin.RateLimitedError
	var he *doffin.HTTPError
	errors.As(err, &he)
	switch {
	case errors.As(err, &rl):
		return OutcomeRateLimited, http.StatusTooManyRequests, msgRateLimited
	case errors.Is(err, doffin.ErrUnauthorized):
		return OutcomeUnauthorized, statusOf(he), msgKeyRejected
	case errors.Is(err, doffin.ErrNotFound):
		return OutcomeNotFound, http.StatusNotFound, msgNotFound
	case errors.Is(err, doffin.ErrEmptyResponse):
		return OutcomeEmptyBody, 0, msgEmptyBody
	case errors.Is(err, doffin.ErrResponseTooLarge):
		return OutcomeTooLarge, 0, msgTooLarge
	case he != nil:
		return OutcomeHTTPError, he.Status, clipMessage(he.Message)
	default:
		return OutcomeTransportError, 0, msgTransport
	}
}

// statusOf is the HTTP status of an HTTPError, or 0 when there is none.
func statusOf(he *doffin.HTTPError) int {
	if he == nil {
		return 0
	}
	return he.Status
}

// clipMessage bounds the client's sanitised message to the stored length, and
// falls back to a fixed text when the client sent none.
func clipMessage(s string) string {
	if s == "" {
		return msgHTTPError
	}
	if runes := []rune(s); len(runes) > maxAttemptMessage {
		return string(runes[:maxAttemptMessage])
	}
	return s
}

// queryKey labels a search page for the raw store: the day, the page size and the sort.
func queryKey(d time.Time, size int) string {
	return fmt.Sprintf("issueDate=%s;perPage=%d;sort=%s", dayKey(d), size, sortBy)
}
