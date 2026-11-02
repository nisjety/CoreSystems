package doffinsync

import (
	"errors"
	"fmt"
	"time"
)

// ErrInvalidWindow reports a window the engine refuses before any write.
var ErrInvalidWindow = errors.New("doffinsync: invalid window")

// earliestDay is the first day of the public corpus (SOURCES.md:54).
var earliestDay = time.Date(2017, time.January, 2, 0, 0, 0, 0, time.UTC)

const dayLayout = "2006-01-02"

// Window is an inclusive range of UTC days. Both bounds are UTC midnights.
type Window struct{ From, To time.Time }

// Validate rejects a bound that is not a UTC midnight, a From before 2017-01-02,
// a From after To, and a To later than UTC yesterday. Only closed days are
// synced: a day still in progress grows while it is paged (D7).
func (w Window) Validate(now time.Time) error {
	switch {
	case !isUTCMidnight(w.From) || !isUTCMidnight(w.To):
		return fmt.Errorf("%w: bounds must be UTC midnights", ErrInvalidWindow)
	case w.From.Before(earliestDay):
		return fmt.Errorf("%w: from is before 2017-01-02", ErrInvalidWindow)
	case w.From.After(w.To):
		return fmt.Errorf("%w: from is after to", ErrInvalidWindow)
	case w.To.After(yesterdayUTC(now)):
		return fmt.Errorf("%w: to is not a closed day", ErrInvalidWindow)
	}
	return nil
}

// Days returns every UTC day in the window in order. It is empty when From is
// after To.
func (w Window) Days() []time.Time {
	var days []time.Time
	last := w.To.UTC()
	for d := w.From.UTC(); !d.After(last); d = d.AddDate(0, 0, 1) {
		days = append(days, d)
	}
	return days
}

func isUTCMidnight(t time.Time) bool {
	return t.Location() == time.UTC && t.Hour() == 0 && t.Minute() == 0 && t.Second() == 0 && t.Nanosecond() == 0
}

// yesterdayUTC is the latest closed day: the UTC calendar day before now's.
func yesterdayUTC(now time.Time) time.Time {
	n := now.UTC()
	today := time.Date(n.Year(), n.Month(), n.Day(), 0, 0, 0, 0, time.UTC)
	return today.AddDate(0, 0, -1)
}

// dayKey is the "2006-01-02" label of a day, as DayStates keys it.
func dayKey(d time.Time) string { return d.UTC().Format(dayLayout) }
