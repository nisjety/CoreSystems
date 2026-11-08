package cron

import (
	"fmt"
	"strconv"
	"strings"
	"time"
)

// horizon bounds the search for one fire, in minutes (five years). A valid
// expression that never fires (30 February) is an error, not a hang.
const horizon = 5 * 366 * 24 * 60

// spec is a compiled expression: a membership table per field, indexed by
// value, and the Vixie day rule. It is built only from a Validate-d expression.
type spec struct {
	minute, hour, dom, month, dow []bool
	// domStar and dowStar record a field that starts with '*'. When either is
	// set the day matches when both tables do; when both are restricted it
	// matches when either does (the classic cron rule).
	domStar, dowStar bool
}

func compile(expr string) (*spec, error) {
	if err := Validate(expr); err != nil {
		return nil, err
	}
	parts := strings.Fields(expr)
	var (
		sp  spec
		err error
	)
	if sp.minute, err = expandField(parts[0], fields[0]); err != nil {
		return nil, err
	}
	if sp.hour, err = expandField(parts[1], fields[1]); err != nil {
		return nil, err
	}
	if sp.dom, err = expandField(parts[2], fields[2]); err != nil {
		return nil, err
	}
	if sp.month, err = expandField(parts[3], fields[3]); err != nil {
		return nil, err
	}
	if sp.dow, err = expandField(parts[4], fields[4]); err != nil {
		return nil, err
	}
	sp.domStar = strings.HasPrefix(parts[2], "*")
	sp.dowStar = strings.HasPrefix(parts[4], "*")
	return &sp, nil
}

// expandField turns one validated field into its membership table.
func expandField(f string, spec fieldSpec) ([]bool, error) {
	set := make([]bool, spec.max+1)
	for _, tok := range strings.Split(f, ",") {
		base, step := tok, 1
		if idx := strings.Index(tok, "/"); idx >= 0 {
			base = tok[:idx]
			n, err := strconv.Atoi(tok[idx+1:])
			if err != nil || n <= 0 {
				return nil, fmt.Errorf("cron: %s: invalid step %q", spec.name, tok[idx+1:])
			}
			step = n
		}
		lo, hi := spec.min, spec.max
		switch {
		case base == "*":
		case strings.Contains(base, "-"):
			idx := strings.Index(base, "-")
			var err error
			if lo, err = strconv.Atoi(base[:idx]); err != nil {
				return nil, fmt.Errorf("cron: %s: %q", spec.name, base)
			}
			if hi, err = strconv.Atoi(base[idx+1:]); err != nil {
				return nil, fmt.Errorf("cron: %s: %q", spec.name, base)
			}
		default:
			n, err := strconv.Atoi(base)
			if err != nil {
				return nil, fmt.Errorf("cron: %s: %q", spec.name, base)
			}
			lo, hi = n, n
		}
		for v := lo; v <= hi && v <= spec.max; v += step {
			if v >= spec.min {
				set[v] = true
			}
		}
	}
	return set, nil
}

// matches reports whether the wall-clock time t (already in the schedule's
// location) is a fire time.
func (sp *spec) matches(t time.Time) bool {
	if !sp.minute[t.Minute()] || !sp.hour[t.Hour()] || !sp.month[int(t.Month())] {
		return false
	}
	domOK := sp.dom[t.Day()]
	dowOK := sp.dow[int(t.Weekday())]
	if sp.domStar || sp.dowStar {
		return domOK && dowOK
	}
	return domOK || dowOK
}

// next returns the first fire time strictly after 'after', read in loc.
func (sp *spec) next(after time.Time, loc *time.Location) (time.Time, bool) {
	t := after.Truncate(time.Minute).Add(time.Minute)
	for i := 0; i < horizon; i++ {
		if sp.matches(t.In(loc)) {
			return t, true
		}
		t = t.Add(time.Minute)
	}
	return time.Time{}, false
}

// minIntervalSamples are the reference instants MinInterval measures from. They
// are fixed (so the answer is deterministic) and offset from one another by
// an amount that is not a whole day, so they land on different phases of a
// daily, weekly or monthly expression.
var minIntervalSamples = []time.Time{
	time.Date(2026, time.January, 1, 0, 0, 0, 0, time.UTC),
	time.Date(2026, time.January, 2, 13, 0, 0, 0, time.UTC),
	time.Date(2026, time.January, 9, 5, 30, 0, 0, time.UTC),
}

// MinInterval returns the smallest gap between two consecutive fire times of a
// valid 5-field expression read in loc (nil means UTC). It takes the next two
// fire times from each of three sample instants and returns the smallest gap.
// An expression that never fires within five years is an error.
func MinInterval(expr string, loc *time.Location) (time.Duration, error) {
	if loc == nil {
		loc = time.UTC
	}
	sp, err := compile(expr)
	if err != nil {
		return 0, err
	}
	var best time.Duration
	for _, from := range minIntervalSamples {
		first, ok := sp.next(from, loc)
		if !ok {
			return 0, fmt.Errorf("cron: %q never fires", expr)
		}
		second, ok := sp.next(first, loc)
		if !ok {
			return 0, fmt.Errorf("cron: %q never fires", expr)
		}
		if gap := second.Sub(first); best == 0 || gap < best {
			best = gap
		}
	}
	return best, nil
}
