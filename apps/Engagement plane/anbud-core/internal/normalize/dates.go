package normalize

import (
	"time"
	_ "time/tzdata" // the container image has no zone database; Europe/Oslo must not depend on it
)

// oslo is the zone Norwegian buyers set deadlines in. It is loaded from the embedded zone
// database, so a missing system database cannot silently turn it into UTC.
var oslo = mustLoad("Europe/Oslo")

func mustLoad(name string) *time.Location {
	loc, err := time.LoadLocation(name)
	if err != nil {
		panic("normalize: zone " + name + " is missing from the embedded zone database: " + err.Error())
	}
	return loc
}

// OsloDay is the calendar day of t in Oslo, as YYYY-MM-DD. A deadline at 23:30 UTC is the next
// day in Oslo, and that is the day a Norwegian reader sees.
func OsloDay(t time.Time) string { return t.In(oslo).Format(time.DateOnly) }

// DaysLeft counts Oslo calendar days from now to the deadline: 0 on the deadline's own day, and
// negative once the day has passed. It counts days, not 24-hour spans, so a change to or from
// summer time does not shift it.
func DaysLeft(deadline, now time.Time) int {
	d := deadline.In(oslo)
	n := now.In(oslo)
	dd := time.Date(d.Year(), d.Month(), d.Day(), 0, 0, 0, 0, time.UTC)
	nd := time.Date(n.Year(), n.Month(), n.Day(), 0, 0, 0, 0, time.UTC)
	return int(dd.Sub(nd).Hours() / 24)
}
