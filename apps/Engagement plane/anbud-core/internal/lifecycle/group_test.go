package lifecycle

import (
	"slices"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

func at(day int, hour int) *time.Time {
	t := time.Date(2026, time.October, day, hour, 0, 0, 0, time.UTC)
	return &t
}

// version builds a stored contract notice. Every value is invented.
func version(id, procedure, notice, ver string, issued *time.Time) Version {
	return Version{
		ID:        id,
		FetchedAt: time.Date(2026, time.October, 20, 0, 0, 0, 0, time.UTC),
		Notice: eforms.Notice{
			Header:   eforms.Header{Root: "ContractNotice", ProcedureID: procedure, NoticeID: notice, VersionID: ver},
			IssuedAt: issued,
		},
	}
}

func ids(vs []Version) []string {
	var out []string
	for _, v := range vs {
		out = append(out, v.ID)
	}
	return out
}

// Contract notices of one procedure are one procedure, oldest first, whatever order they come in.
func TestGroupPutsTheNoticesOfAProcedureTogetherOldestFirst(t *testing.T) {
	t.Parallel()

	got := Group([]Version{
		version("2026-300002", "proc-1", "n2", "01", at(8, 9)),
		version("2026-300001", "proc-1", "n1", "01", at(1, 9)),
		version("2026-300009", "proc-2", "n9", "01", at(2, 9)),
	})
	if len(got) != 2 {
		t.Fatalf("procedures = %d, want 2", len(got))
	}
	var one Procedure
	for _, p := range got {
		if p.ID == "proc-1" {
			one = p
		}
	}
	if !slices.Equal(ids(one.Versions), []string{"2026-300001", "2026-300002"}) {
		t.Errorf("versions = %v, want the earlier issue first", ids(one.Versions))
	}
	if one.Original().ID != "2026-300001" || one.Current().ID != "2026-300002" || !one.Amended() {
		t.Errorf("original %q current %q amended %v", one.Original().ID, one.Current().ID, one.Amended())
	}
}

// With the same issue time the version number orders them, then when they were stored, then the id,
// so the order never depends on the order the store returned them in.
func TestGroupOrdersByIssueTimeThenVersionThenStoredThenID(t *testing.T) {
	t.Parallel()

	same := at(5, 9)
	a := version("2026-300001", "p", "n", "01", same)
	b := version("2026-300002", "p", "n", "02", same)
	c := version("2026-300003", "p", "m", "02", same)
	c.FetchedAt = c.FetchedAt.Add(time.Hour)
	d := version("2026-300004", "p", "k", "02", same)
	d.FetchedAt = c.FetchedAt
	want := []string{"2026-300001", "2026-300002", "2026-300003", "2026-300004"}
	for _, in := range [][]Version{{d, c, b, a}, {a, b, c, d}, {c, a, d, b}} {
		got := Group(in)
		if len(got) != 1 || !slices.Equal(ids(got[0].Versions), want) {
			t.Errorf("order = %v, want %v", ids(got[0].Versions), want)
		}
	}
}

// A notice with no issue time is ordered by what is left, and does not break the order of the rest.
func TestGroupOrdersANoticeWithNoIssueTimeFirst(t *testing.T) {
	t.Parallel()

	got := Group([]Version{
		version("2026-300002", "p", "n2", "02", at(8, 9)),
		version("2026-300001", "p", "n1", "01", nil),
		version("2026-300003", "p", "n3", "03", at(9, 9)),
	})
	if len(got) != 1 || !slices.Equal(ids(got[0].Versions), []string{"2026-300001", "2026-300002", "2026-300003"}) {
		t.Errorf("order = %v", got)
	}
}

// Only contract notices of a procedure are grouped. A notice with no procedure identifier, or a result
// notice, stands alone: a result is not an amendment of the competition.
func TestGroupLeavesOtherNoticesAlone(t *testing.T) {
	t.Parallel()

	planning := version("2026-300001", "", "n1", "01", at(1, 9))
	planning2 := version("2026-300002", "", "n2", "01", at(2, 9))
	award := version("2026-300003", "proc-1", "n3", "01", at(3, 9))
	award.Notice.Root = "ContractAwardNotice"
	competition := version("2026-300004", "proc-1", "n4", "01", at(1, 9))
	got := Group([]Version{planning, planning2, award, competition})
	if len(got) != 4 {
		t.Fatalf("procedures = %d, want each of the four alone", len(got))
	}
	for _, p := range got {
		if p.Amended() {
			t.Errorf("%q is amended: %v", p.ID, ids(p.Versions))
		}
	}
}

// The same version stored twice is one version: it is not an amendment of itself.
func TestGroupKeepsASingleCopyOfAVersion(t *testing.T) {
	t.Parallel()

	got := Group([]Version{
		version("2026-300001", "p", "n", "01", at(1, 9)),
		version("2026-300002", "p", "n", "01", at(1, 9)),
	})
	if len(got) != 1 || len(got[0].Versions) != 1 || got[0].Amended() {
		t.Errorf("procedures = %+v, want one version", got)
	}
}

// A change confirms its predecessor when it names it by notice identifier and version.
func TestConfirms(t *testing.T) {
	t.Parallel()

	prev := version("2026-300001", "p", "0cdb6ab9-aaaa-4bbb-8ccc-0123456789ab", "01", at(1, 9))
	next := version("2026-300002", "p", "n2", "01", at(2, 9))
	next.Notice.Change = &eforms.ChangeInfo{Replaces: "0cdb6ab9-aaaa-4bbb-8ccc-0123456789ab-01"}
	if !Confirms(prev, next) {
		t.Error("a change naming the predecessor by identifier and version does not confirm it")
	}
	next.Notice.Change.Replaces = "0cdb6ab9-aaaa-4bbb-8ccc-0123456789ab-02"
	if Confirms(prev, next) {
		t.Error("a change naming another version confirms the predecessor")
	}
	next.Notice.Change.Replaces = "123456-2026"
	if Confirms(prev, next) {
		t.Error("a publication number confirms the predecessor: it cannot be matched")
	}
	next.Notice.Change = nil
	if Confirms(prev, next) {
		t.Error("a notice with no change section confirms its predecessor")
	}
}

// The order is by when a notice was issued, even where its version number and its Doffin id say the
// other way: a notice issued later is the later version of the tender, whatever number it carries.
func TestGroupOrdersByIssueTimeBeforeVersionNumberAndId(t *testing.T) {
	t.Parallel()

	got := Group([]Version{
		version("2026-300001", "p", "n1", "01", at(8, 9)),
		version("2026-300002", "p", "n2", "02", at(1, 9)),
	})
	if len(got) != 1 || !slices.Equal(ids(got[0].Versions), []string{"2026-300002", "2026-300001"}) {
		t.Errorf("order = %v, want the one issued on the first day first", ids(got[0].Versions))
	}
	if got[0].Current().ID != "2026-300001" {
		t.Errorf("current = %q, want the one issued last", got[0].Current().ID)
	}
}
