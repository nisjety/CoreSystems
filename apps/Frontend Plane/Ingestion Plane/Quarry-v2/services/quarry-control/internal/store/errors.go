package store

import "errors"

var (
	ErrNotFound = errors.New("not found")
	ErrConflict = errors.New("conflict")
	// ErrQuotaExceeded: a per-org cap (webhook subscriptions) is full.
	ErrQuotaExceeded = errors.New("quota exceeded")
)
