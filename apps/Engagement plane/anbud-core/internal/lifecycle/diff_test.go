package lifecycle

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

func notice() eforms.Notice {
	d := time.Date(2026, time.November, 2, 11, 0, 0, 0, time.UTC)
	return eforms.Notice{
		Header:         eforms.Header{Root: "ContractNotice"},
		Title:          "Drift av fagsystem",
		Description:    "Fiktiv beskrivelse.",
		ProcedureType:  "open",
		EstimatedValue: &eforms.Amount{Minor: 150_000_000, Currency: "NOK"},
		CPVMain:        []string{"72212000"},
		NUTS:           []string{"NO081"},
		Deadline:       &d,
		Lots: []eforms.Lot{{
			ID: "LOT-0001", Title: "Delkontrakt 1", Deadline: &d,
			AwardCriteria: []eforms.AwardCriterion{{Type: "quality", Name: "Kvalitet", Weight: ip(60)}, {Type: "price", Name: "Pris", Weight: ip(40)}},
		}},
		Documents:         []eforms.DocumentRef{{Lot: "LOT-0001", URI: "https://example.invalid/docs/1"}},
		SelectionCriteria: []eforms.SelectionCriterion{{Lot: "LOT-0001", Type: "other", Name: "Krav", Description: "Tre referanser."}},
		Requirements:      []eforms.Requirement{{Lot: "LOT-0001", Type: "CERTIFICATE", Text: "ISO 9001"}},
	}
}

func ip(n int) *int { return &n }

func find(cs []Change, field, lot, op string) *Change {
	for i := range cs {
		if cs[i].Field == field && cs[i].Lot == lot && cs[i].Op == op {
			return &cs[i]
		}
	}
	return nil
}

func TestDiffOfAnUnchangedNoticeIsEmpty(t *testing.T) {
	t.Parallel()

	if got, total := Diff(notice(), notice()); len(got) != 0 || total != 0 {
		t.Errorf("changes = %+v (total %d), want none", got, total)
	}
}

// A deadline that moves is reported with the days it moved, counted in Oslo calendar days, later being
// positive. It is material. The same is said of a deadline of a lot.
func TestDiffReportsAMovedDeadlineInDays(t *testing.T) {
	t.Parallel()

	prev, next := notice(), notice()
	later := time.Date(2026, time.November, 16, 11, 0, 0, 0, time.UTC)
	next.Deadline = &later
	next.Lots[0].Deadline = &later
	got, _ := Diff(prev, next)
	for _, lot := range []string{"", "LOT-0001"} {
		c := find(got, "deadline", lot, "changed")
		if c == nil || c.DaysMoved == nil || *c.DaysMoved != 14 || !c.Material {
			t.Fatalf("deadline of %q = %+v, want 14 days later and material", lot, c)
		}
		if c.Before != "2026-11-02T11:00:00Z" || c.After != "2026-11-16T11:00:00Z" {
			t.Errorf("before %q after %q", c.Before, c.After)
		}
	}
	earlier := time.Date(2026, time.October, 30, 11, 0, 0, 0, time.UTC)
	next.Deadline = &earlier
	got, _ = Diff(prev, next)
	if c := find(got, "deadline", "", "changed"); c == nil || *c.DaysMoved != -3 {
		t.Errorf("an earlier deadline = %+v, want 3 days earlier", c)
	}
	sameDay := time.Date(2026, time.November, 2, 13, 0, 0, 0, time.UTC)
	next.Deadline = &sameDay
	got, _ = Diff(prev, next)
	if c := find(got, "deadline", "", "changed"); c == nil || *c.DaysMoved != 0 {
		t.Errorf("a later hour on the same day = %+v, want 0 days", c)
	}
}

// A deadline that appears or goes is added or removed, not moved.
func TestDiffReportsADeadlineThatAppearsOrGoes(t *testing.T) {
	t.Parallel()

	prev, next := notice(), notice()
	next.Deadline = nil
	got, _ := Diff(prev, next)
	if c := find(got, "deadline", "", "removed"); c == nil || c.DaysMoved != nil {
		t.Errorf("removed deadline = %+v", c)
	}
	got, _ = Diff(next, prev)
	if c := find(got, "deadline", "", "added"); c == nil || c.After == "" {
		t.Errorf("added deadline = %+v", c)
	}
}

func TestDiffReportsTheValueOfTheNoticeAndOfALot(t *testing.T) {
	t.Parallel()

	prev, next := notice(), notice()
	next.EstimatedValue = &eforms.Amount{Minor: 200_050_000, Currency: "NOK"}
	prev.Lots[0].Value = &eforms.Amount{Minor: 50_000_000, Currency: "NOK"}
	next.Lots[0].Value = &eforms.Amount{Minor: 60_000_000, Currency: "NOK"}
	got, _ := Diff(prev, next)
	if c := find(got, "value", "", "changed"); c == nil || c.Before != "1500000.00 NOK" || c.After != "2000500.00 NOK" || !c.Material {
		t.Errorf("value = %+v", c)
	}
	if c := find(got, "value", "LOT-0001", "changed"); c == nil || c.Before != "500000.00 NOK" || c.After != "600000.00 NOK" {
		t.Errorf("lot value = %+v", c)
	}
}

// Codes, places, lots and links are compared as sets: what was added and what was removed.
func TestDiffReportsWhatWasAddedAndRemoved(t *testing.T) {
	t.Parallel()

	prev, next := notice(), notice()
	next.CPVMain = []string{"72212000", "72000000"}
	next.NUTS = []string{"NO082"}
	next.Lots = append(next.Lots, eforms.Lot{ID: "LOT-0002", Title: "Delkontrakt 2"})
	next.Documents = []eforms.DocumentRef{{Lot: "LOT-0001", URI: "https://example.invalid/docs/2"}}
	got, _ := Diff(prev, next)
	for _, want := range []struct{ field, op, text string }{
		{"cpv", "added", "72000000"}, {"nuts", "added", "NO082"}, {"nuts", "removed", "NO081"},
		{"lots", "added", "Delkontrakt 2"},
		{"documents", "added", "https://example.invalid/docs/2"}, {"documents", "removed", "https://example.invalid/docs/1"},
	} {
		found := false
		for _, c := range got {
			if c.Field == want.field && c.Op == want.op && (strings.Contains(c.After, want.text) || strings.Contains(c.Before, want.text)) {
				found = true
				if !c.Material {
					t.Errorf("%s %s is not material", want.field, want.op)
				}
			}
		}
		if !found {
			t.Errorf("no %s %s of %q in %+v", want.field, want.op, want.text, got)
		}
	}
	if find(got, "cpv", "", "removed") != nil {
		t.Errorf("a code that is still there is reported removed")
	}
}

// A CPV code is compared without its check digit, so a notice that writes 72212000-7 where it wrote
// 72212000 has not changed.
func TestDiffComparesCPVCodesWithoutTheCheckDigit(t *testing.T) {
	t.Parallel()

	prev, next := notice(), notice()
	next.CPVMain = []string{"72212000-7"}
	if got, _ := Diff(prev, next); len(got) != 0 {
		t.Errorf("changes = %+v, want none", got)
	}
}

// Title and description changes are reported and are not material. A description change carries no
// text, only its length, so a diff never repeats free text.
func TestDiffReportsWordingAsNotMaterial(t *testing.T) {
	t.Parallel()

	prev, next := notice(), notice()
	next.Title = "Drift og vedlikehold av fagsystem"
	next.Description = strings.Repeat("Ny beskrivelse. ", 20)
	got, _ := Diff(prev, next)
	title := find(got, "title", "", "changed")
	if title == nil || title.Material || title.Before != "Drift av fagsystem" {
		t.Errorf("title = %+v", title)
	}
	desc := find(got, "description", "", "changed")
	if desc == nil || desc.Material || strings.Contains(desc.After, "Ny beskrivelse") || !strings.Contains(desc.After, "320") {
		t.Errorf("description = %+v, want its length and none of its text", desc)
	}
}

func TestDiffReportsCriteriaAndRequirements(t *testing.T) {
	t.Parallel()

	prev, next := notice(), notice()
	next.Lots[0].AwardCriteria = []eforms.AwardCriterion{{Type: "quality", Name: "Kvalitet", Weight: ip(70)}, {Type: "price", Name: "Pris", Weight: ip(30)}}
	next.SelectionCriteria = []eforms.SelectionCriterion{{Lot: "LOT-0001", Type: "other", Name: "Krav", Description: "Fem referanser."}}
	next.Requirements = append(next.Requirements, eforms.Requirement{Lot: "LOT-0001", Type: "TURNOVER", Text: "Omsetning over 10 MNOK"})
	got, _ := Diff(prev, next)
	if c := find(got, "award_criteria", "LOT-0001", "changed"); c == nil || !strings.Contains(c.Before, "60") || !strings.Contains(c.After, "70") || !c.Material {
		t.Errorf("award criterion weight = %+v", c)
	}
	if c := find(got, "selection_criteria", "LOT-0001", "added"); c == nil || !strings.Contains(c.After, "Fem referanser") {
		t.Errorf("added selection criterion = %+v", c)
	}
	if c := find(got, "selection_criteria", "LOT-0001", "removed"); c == nil || !strings.Contains(c.Before, "Tre referanser") {
		t.Errorf("removed selection criterion = %+v", c)
	}
	if c := find(got, "requirements", "LOT-0001", "added"); c == nil || !strings.Contains(c.After, "TURNOVER") {
		t.Errorf("added requirement = %+v", c)
	}
}

// The list is bounded: a notice that changed everywhere gives the first MaxChanges, and says how many
// there were. Each text is clipped.
func TestDiffIsBounded(t *testing.T) {
	t.Parallel()

	prev, next := notice(), notice()
	for i := 0; i < MaxChanges+40; i++ {
		next.Documents = append(next.Documents, eforms.DocumentRef{URI: "https://example.invalid/" + strings.Repeat("x", 1000) + string(rune('a'+i%26)) + strings.Repeat("y", i)})
	}
	got, total := Diff(prev, next)
	if len(got) != MaxChanges || total < MaxChanges+40 {
		t.Errorf("changes %d total %d, want %d and at least %d", len(got), total, MaxChanges, MaxChanges+40)
	}
	for _, c := range got {
		if n := len([]rune(c.After)); n > MaxTextRunes+1 {
			t.Fatalf("a text of %d characters is not clipped", n)
		}
	}
}

// The diff is linear in the size of the notices. A notice may hold tens of thousands of links, codes
// or criteria within the parser's limits, and the demo diffs inside a request, so a comparison that
// scans one list for each element of the other would take minutes. The bound is generous: a
// quadratic comparison of this size takes ten times as long.
func TestDiffIsLinearInTheSizeOfTheNotice(t *testing.T) {
	t.Parallel()

	const n = 60_000
	prev, next := notice(), notice()
	for i := 0; i < n; i++ {
		link := fmt.Sprintf("https://example.invalid/%d", i)
		prev.Documents = append(prev.Documents, eforms.DocumentRef{URI: link})
		next.Documents = append(next.Documents, eforms.DocumentRef{URI: link + "x"})
		prev.CPVMain = append(prev.CPVMain, fmt.Sprintf("%08d", i))
		next.CPVMain = append(next.CPVMain, fmt.Sprintf("%08d", i+1))
	}
	start := time.Now()
	_, total := Diff(prev, next)
	if took := time.Since(start); took > 3*time.Second {
		t.Errorf("a diff of two notices of %d links and %d codes each took %s", n, n, took)
	}
	if total < 2*n {
		t.Errorf("total = %d, want the changes counted all the same", total)
	}
}
