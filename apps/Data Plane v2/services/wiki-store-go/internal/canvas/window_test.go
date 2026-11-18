package canvas

import (
	"testing"
	"time"
)

// TestFloorSetWindowsOnlyWhenAFloorIsNamed: the zero floor windows nothing, and a timestamp or a revision makes the floor set.
func TestFloorSetWindowsOnlyWhenAFloorIsNamed(t *testing.T) {
	at := time.Date(2026, time.November, 1, 10, 0, 0, 0, time.UTC)
	cases := map[string]struct {
		floor Floor
		want  bool
	}{
		"zero":            {Floor{}, false},
		"revision zero":   {Floor{Revision: 0}, false},
		"revision":        {Floor{Revision: 4}, true},
		"instant":         {Floor{At: &at}, true},
		"instant and rev": {Floor{At: &at, Revision: 4}, true},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			if got := tc.floor.Set(); got != tc.want {
				t.Fatalf("Set() = %v, want %v", got, tc.want)
			}
		})
	}
}

// TestFloorAdmitsOnlyACanvasWhollyInsideTheWindow checks the earliest version of a canvas against every floor. The boundary is
// inclusive: a canvas created at the floor's instant, or under the floor's revision, is inside the window.
func TestFloorAdmitsOnlyACanvasWhollyInsideTheWindow(t *testing.T) {
	at := time.Date(2026, time.November, 1, 10, 0, 0, 0, time.UTC)
	justBefore := at.Add(-time.Nanosecond)
	later := at.Add(time.Hour)
	cases := map[string]struct {
		floor     Floor
		oldestAt  time.Time
		oldestRev int64
		wantAdmit bool
	}{
		"no floor admits any canvas":                   {Floor{}, justBefore, 1, true},
		"instant: created at the instant":              {Floor{At: &at}, at, 1, true},
		"instant: created after it":                    {Floor{At: &at}, later, 1, true},
		"instant: created a tick before it":            {Floor{At: &at}, justBefore, 9, false},
		"revision: at the revision":                    {Floor{Revision: 4}, later, 4, true},
		"revision: below it":                           {Floor{Revision: 4}, later, 3, false},
		"both: inside both":                            {Floor{At: &at, Revision: 4}, later, 4, true},
		"both: after the instant but below revision":   {Floor{At: &at, Revision: 4}, later, 3, false},
		"both: at the revision but before the instant": {Floor{At: &at, Revision: 4}, justBefore, 4, false},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			if got := tc.floor.admits(tc.oldestAt, tc.oldestRev); got != tc.wantAdmit {
				t.Fatalf("admits(%v, %d) = %v, want %v", tc.oldestAt, tc.oldestRev, got, tc.wantAdmit)
			}
		})
	}
}

// TestHiddenCanvasIsTheFloorRefusal: a canvas the window hides answers the wire code Control's decision refusal uses, so one code names
// the floor on both planes.
func TestHiddenCanvasIsTheFloorRefusal(t *testing.T) {
	err := hiddenCanvasError()
	if err.Code != CodeHistoryFloor || CodeHistoryFloor != "space_history_floor_unsupported" {
		t.Fatalf("hidden canvas code = %q, want space_history_floor_unsupported", err.Code)
	}
}
