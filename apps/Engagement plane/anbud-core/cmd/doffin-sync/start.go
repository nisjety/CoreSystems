package main

import "time"

// startRefused reports whether a stored retry instant forbids a start, before any
// Doffin call (D19). ok says a run stored an instant, and until is that instant.
// The store does not compare it with any clock (D-B4), so this process's clock
// decides: the start is refused only while now is before until.
func startRefused(until time.Time, ok bool, now time.Time) bool {
	return ok && now.Before(until)
}
