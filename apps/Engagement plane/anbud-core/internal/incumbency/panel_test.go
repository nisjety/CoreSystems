package incumbency

import (
	"encoding/json"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

const buyer = "974760673"

var (
	beta = eforms.Winner{Name: "Annen Fiktiv AS", OrgNumber: "222222222"}
	// solo is a sole trader: its name is a person's name.
	solo    = eforms.Winner{Name: "Ola Fiktivsen", OrgNumber: "333333333"}
	foreign = eforms.Winner{Name: "Fiktiv GmbH", OrgNumber: "DE123456789"}
)

func one(t *testing.T, r result) Award {
	t.Helper()
	awards := FromNotice(r.notice, r.notice_())
	if len(awards) != 1 {
		t.Fatalf("fixture %s gave %d awards", r.notice, len(awards))
	}
	return awards[0]
}

var classes = map[string]Class{"111111111": Organisation, "222222222": Organisation, "333333333": SoleTrader}

func TestRelationNeedsTheSameBuyerAndACloseCPV(t *testing.T) {
	t.Parallel()

	target := Target{ProcedureID: "proc-now", BuyerOrgNumbers: []string{buyer}, CPV: []string{"72212100"}}
	cases := []struct {
		name string
		r    result
		want Relation
		ok   bool
	}{
		{"the same code", result{notice: "2025-1", procedure: "p1", buyer: buyer, cpv: "72212100", winners: []eforms.Winner{acme}}, SameCPV, true},
		{"the same class", result{notice: "2025-2", procedure: "p2", buyer: buyer, cpv: "72215000", winners: []eforms.Winner{acme}}, SameClass, true},
		{"the same group", result{notice: "2025-3", procedure: "p3", buyer: buyer, cpv: "72250000", winners: []eforms.Winner{acme}}, SameGroup, true},
		{"only the division", result{notice: "2025-4", procedure: "p4", buyer: buyer, cpv: "72800000", winners: []eforms.Winner{acme}}, "", false},
		{"another buyer", result{notice: "2025-5", procedure: "p5", buyer: "922222222", cpv: "72212100", winners: []eforms.Winner{acme}}, "", false},
		{"the same procedure", result{notice: "2025-6", procedure: "proc-now", buyer: buyer, cpv: "72212100", winners: []eforms.Winner{acme}}, "", false},
	}
	for _, c := range cases {
		got, ok := Relate(target, one(t, c.r))
		if ok != c.ok || got != c.want {
			t.Errorf("%s: Relate = %q %v, want %q %v", c.name, got, ok, c.want, c.ok)
		}
	}
}

// The panel names a winner only when the register says it is an organisation that is not a sole
// trader. A sole trader's name is a person's name and is never shown; a winner that could not be
// checked (no lookup, a foreign identifier) is not shown either. Only organisations are ranked.
func TestPanelNamesOnlyVerifiedOrganisations(t *testing.T) {
	t.Parallel()

	target := Target{ProcedureID: "proc-now", BuyerOrgNumbers: []string{buyer}, CPV: []string{"72212000"}}
	awards := []Award{
		one(t, result{notice: "2024-1", procedure: "p1", buyer: buyer, cpv: "72212000", awarded: day(2024, 3, 1), winners: []eforms.Winner{acme}, tenders: intp(5)}),
		one(t, result{notice: "2023-1", procedure: "p2", buyer: buyer, cpv: "72212000", awarded: day(2023, 3, 1), winners: []eforms.Winner{acme}, tenders: intp(3)}),
		one(t, result{notice: "2022-1", procedure: "p3", buyer: buyer, cpv: "72212000", awarded: day(2022, 3, 1), winners: []eforms.Winner{beta}, tenders: intp(7)}),
		one(t, result{notice: "2021-1", procedure: "p4", buyer: buyer, cpv: "72212000", awarded: day(2021, 3, 1), winners: []eforms.Winner{solo}}),
		one(t, result{notice: "2020-1", procedure: "p5", buyer: buyer, cpv: "72212000", awarded: day(2020, 3, 1), winners: []eforms.Winner{foreign}}),
	}
	p := Build(target, awards, classes)

	if p.Related != 5 || len(p.Entries) != 5 {
		t.Fatalf("related %d entries %d, want 5 and 5", p.Related, len(p.Entries))
	}
	if got := []string{p.Entries[0].NoticeID, p.Entries[4].NoticeID}; !slices.Equal(got, []string{"2024-1", "2020-1"}) {
		t.Errorf("entries are not newest first: %v", got)
	}
	if w := p.Entries[0].Winners; len(w) != 1 || w[0] != (WinnerView{Name: acme.Name, OrgNumber: acme.OrgNumber, Class: Organisation}) {
		t.Errorf("a verified organisation is not named: %+v", w)
	}
	if w := p.Entries[3].Winners; len(w) != 1 || w[0] != (WinnerView{Class: SoleTrader}) {
		t.Errorf("a sole trader is not withheld: %+v", w)
	}
	if w := p.Entries[4].Winners; len(w) != 1 || w[0] != (WinnerView{Class: Unverified}) {
		t.Errorf("an unverified winner is not withheld: %+v", w)
	}
	if want := []WinnerCount{{Name: acme.Name, OrgNumber: acme.OrgNumber, Wins: 2}, {Name: beta.Name, OrgNumber: beta.OrgNumber, Wins: 1}}; !slices.Equal(p.TopWinners, want) {
		t.Errorf("top winners = %+v, want %+v", p.TopWinners, want)
	}
	if p.WithheldWinners != 2 {
		t.Errorf("withheld winners = %d, want 2 (the sole trader and the foreign winner)", p.WithheldWinners)
	}
	if p.MedianTendersReceived == nil || *p.MedianTendersReceived != 5 {
		t.Errorf("median tenders = %v, want 5 of 3, 5 and 7", p.MedianTendersReceived)
	}
	out, _ := json.Marshal(p)
	for _, personal := range []string{solo.Name, solo.OrgNumber, foreign.Name, foreign.OrgNumber} {
		if strings.Contains(string(out), personal) {
			t.Errorf("the panel carries %q", personal)
		}
	}
}

// The likely incumbent is the latest award of the closest relation: a closer CPV beats a newer award.
func TestIncumbentIsTheLatestAwardOfTheClosestRelation(t *testing.T) {
	t.Parallel()

	target := Target{ProcedureID: "proc-now", BuyerOrgNumbers: []string{buyer}, CPV: []string{"72212000"}}
	awards := []Award{
		one(t, result{notice: "2025-1", procedure: "p1", buyer: buyer, cpv: "72250000", awarded: day(2025, 3, 1), winners: []eforms.Winner{beta}}),
		one(t, result{notice: "2022-1", procedure: "p2", buyer: buyer, cpv: "72212000", awarded: day(2022, 3, 1), winners: []eforms.Winner{acme}}),
		one(t, result{notice: "2023-1", procedure: "p3", buyer: buyer, cpv: "72212000", awarded: day(2023, 3, 1), winners: []eforms.Winner{acme}}),
	}
	p := Build(target, awards, classes)
	if p.Incumbent == nil || p.Incumbent.NoticeID != "2023-1" || p.Incumbent.Relation != SameCPV {
		t.Errorf("incumbent = %+v, want the 2023 award of the same code", p.Incumbent)
	}
}

func TestPanelOfNothing(t *testing.T) {
	t.Parallel()

	p := Build(Target{BuyerOrgNumbers: []string{buyer}, CPV: []string{"72212000"}}, nil, nil)
	if p.Related != 0 || p.Incumbent != nil || len(p.Entries) != 0 || p.MedianTendersReceived != nil {
		t.Errorf("empty panel = %+v", p)
	}
}

// The entries are capped, the newest kept; the counts still cover every related award.
func TestPanelCapsItsEntries(t *testing.T) {
	t.Parallel()

	var awards []Award
	for i := 0; i < MaxEntries+5; i++ {
		awards = append(awards, one(t, result{notice: "n" + string(rune('a'+i)), procedure: "p" + string(rune('a'+i)), buyer: buyer, cpv: "72212000",
			awarded: day(2000+i, 1, 1), winners: []eforms.Winner{acme}}))
	}
	p := Build(Target{BuyerOrgNumbers: []string{buyer}, CPV: []string{"72212000"}}, awards, classes)
	if p.Related != MaxEntries+5 || len(p.Entries) != MaxEntries || p.TopWinners[0].Wins != MaxEntries+5 {
		t.Errorf("related %d entries %d wins %d", p.Related, len(p.Entries), p.TopWinners[0].Wins)
	}
	if p.Entries[0].AwardDate.Year() != 2000+MaxEntries+4 {
		t.Errorf("the newest entry is %v", p.Entries[0].AwardDate)
	}
}

// WinnersToCheck lists the Norwegian organisation numbers of the related awards' winners, each once,
// for a register lookup. A foreign identifier cannot be checked there and is not listed.
func TestWinnersToCheck(t *testing.T) {
	t.Parallel()

	target := Target{BuyerOrgNumbers: []string{buyer}, CPV: []string{"72212000"}}
	awards := []Award{
		one(t, result{notice: "1", procedure: "p1", buyer: buyer, cpv: "72212000", winners: []eforms.Winner{acme, beta}}),
		one(t, result{notice: "2", procedure: "p2", buyer: buyer, cpv: "72212000", winners: []eforms.Winner{acme, foreign}}),
		one(t, result{notice: "3", procedure: "p3", buyer: "922222222", cpv: "72212000", winners: []eforms.Winner{solo}}),
	}
	got := WinnersToCheck(target, awards, 10)
	slices.Sort(got)
	if want := []string{"111111111", "222222222"}; !slices.Equal(got, want) {
		t.Errorf("to check = %v, want %v", got, want)
	}
	if got := WinnersToCheck(target, awards, 1); len(got) != 1 {
		t.Errorf("the limit is not kept: %v", got)
	}
}

var _ = time.UTC
