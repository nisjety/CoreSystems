package main

import "github.com/triodelab/anbud/internal/doffinsync"

// Exit codes, from the exit table in plan section 4. They are the contract with
// the operator and with any scheduler that reads the status.
const (
	exitDone        = 0 // every day in the window is done
	exitUsage       = 1 // usage or configuration, nothing written; or a database role too privileged for the host, after the migrations
	exitPartial     = 2 // gaps remain: a day is partial, or -max-downloads was reached
	exitPaused      = 3 // Doffin answered 429; the retry instant is stored
	exitKeyRejected = 4 // Doffin answered 401 or 403
	exitCancelled   = 5 // a signal ended the run
	exitFailed      = 6 // a search, store or ledger failure ended the run
	exitLocked      = 7 // another sync holds the budget lock; nothing was called
)

// exitCodeFor maps a run reason to its exit code. An unknown reason is a failure,
// so a reason the table does not know cannot exit 0 by accident.
func exitCodeFor(r doffinsync.Reason) int {
	switch r {
	case doffinsync.ReasonDone:
		return exitDone
	case doffinsync.ReasonPartial:
		return exitPartial
	case doffinsync.ReasonPaused:
		return exitPaused
	case doffinsync.ReasonKeyRejected:
		return exitKeyRejected
	case doffinsync.ReasonCancelled:
		return exitCancelled
	default:
		return exitFailed
	}
}
