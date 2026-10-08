package ledger

import (
	"context"
	"time"
)

// ReportingStartReader exposes a display baseline, never a billing/budget reset.
type ReportingStartReader interface {
	ReportingStart(context.Context, string) (*time.Time, error)
}
