package demo

import (
	"slices"
	"testing"
)

func TestOneNoticeHeldFromDoffinAndTEDIsShownOnce(t *testing.T) {
	for _, tc := range []struct {
		name  string
		items []twin
		want  []bool
	}{
		{"the same version: Doffin's copy, whichever came first",
			[]twin{{"u1", "01", "ted"}, {"u1", "01", "doffin"}},
			[]bool{false, true}},
		{"the same version, Doffin first",
			[]twin{{"u1", "01", "doffin"}, {"u1", "01", "ted"}},
			[]bool{true, false}},
		{"a newer version on TED wins over Doffin's older one",
			[]twin{{"u1", "01", "doffin"}, {"u1", "02", "ted"}},
			[]bool{false, true}},
		{"different notices are both kept",
			[]twin{{"u1", "01", "doffin"}, {"u2", "01", "ted"}},
			[]bool{true, true}},
		{"an unreadable body is always kept",
			[]twin{{"", "", "ted"}, {"", "", "doffin"}, {"u1", "01", "ted"}},
			[]bool{true, true, true}},
		{"the best copy's source wins; that source's own versions are the amendments' to group",
			[]twin{{"u1", "01", "ted"}, {"u1", "02", "doffin"}, {"u1", "01", "doffin"}},
			[]bool{false, true, true}},
		{"copies within one source are never merged by this rule",
			[]twin{{"u1", "01", "doffin"}, {"u1", "01", "doffin"}, {"u2", "01", "ted"}, {"u2", "02", "ted"}},
			[]bool{true, true, true, true}},
	} {
		if got := keepOnePerNotice(tc.items); !slices.Equal(got, tc.want) {
			t.Errorf("%s: keep %v, want %v", tc.name, got, tc.want)
		}
	}
}
