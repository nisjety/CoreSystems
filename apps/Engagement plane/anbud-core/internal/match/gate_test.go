package match

import (
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/orgcore"
)

// now is 09:00 Oslo time on Tuesday 10 November 2026.
var now = time.Date(2026, time.November, 10, 8, 0, 0, 0, time.UTC)

func at(days int) *time.Time {
	t := now.Add(time.Duration(days) * 24 * time.Hour)
	return &t
}

func nok(major int64) *eforms.Amount { return &eforms.Amount{Minor: major * 100, Currency: "NOK"} }

// tender is a parsed notice for IT services in Oslo, worth 1 MNOK, open for 20 more days.
func tender() eforms.Notice {
	return eforms.Notice{
		Title:          "Drift av fagsystem",
		Description:    "Kommunen skal kjøpe drift og forvaltning av et fagsystem.",
		CPVMain:        []string{"72212000"},
		NUTS:           []string{"NO081"},
		EstimatedValue: nok(1_000_000),
		Lots:           []eforms.Lot{{ID: "LOT-0001", Title: "Drift", Deadline: at(20)}},
	}
}

func profile() Profile {
	return Profile{
		CPV:      []string{"72000000"},
		NUTS:     []string{"NO08"},
		MinValue: nok(200_000),
		MaxValue: nok(5_000_000),
		Keywords: []string{"fagsystem"},
	}
}

func assess(n eforms.Notice, p Profile, s Strictness) Assessment {
	return Assess(n, normalize.Notice(n), p, s, now)
}

func check(t *testing.T, a Assessment, rule string) Check {
	t.Helper()
	for _, c := range append(a.Gate, a.Fit...) {
		if c.Rule == rule {
			return c
		}
	}
	t.Fatalf("no check %q in %+v", rule, a)
	return Check{}
}

func TestAStrongMatchPassesEveryGate(t *testing.T) {
	t.Parallel()

	a := assess(tender(), profile(), Balanced)
	if a.Verdict != Pass || a.Band != Strong {
		t.Errorf("verdict %v band %v, want pass/strong; checks %+v %+v", a.Verdict, a.Band, a.Gate, a.Fit)
	}
	if c := check(t, a, RuleDeadline); c.Outcome != Pass || c.DaysLeft == nil || *c.DaysLeft != 20 {
		t.Errorf("deadline check = %+v", c)
	}
}

func TestAPassedDeadlineIsAHardFailNotALowScore(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots[0].Deadline = at(-1)
	a := assess(n, profile(), Wide)
	if a.Verdict != Fail {
		t.Errorf("verdict = %v, want fail", a.Verdict)
	}
	if a.Band != Strong {
		t.Errorf("band = %v: the fit is still shown, the gate decides", a.Band)
	}
}

func TestOneOpenLotKeepsTheNoticeOpen(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots = append(n.Lots, eforms.Lot{ID: "LOT-0002", Deadline: at(-3)})
	if c := check(t, assess(n, profile(), Balanced), RuleDeadline); c.Outcome != Pass || *c.DaysLeft != 20 {
		t.Errorf("deadline = %+v, want pass with 20 days to the open lot", c)
	}
}

func TestAMissingDeadlineIsUnknownNotPassed(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots[0].Deadline = nil
	a := assess(n, profile(), Balanced)
	if c := check(t, a, RuleDeadline); c.Outcome != Unknown {
		t.Errorf("deadline = %+v, want unknown", c)
	}
	if a.Verdict != Conditional {
		t.Errorf("verdict = %v: a notice whose deadline nobody knows is not a clean pass", a.Verdict)
	}
}

func TestACompanyTheRegisterBlocksFailsTheGate(t *testing.T) {
	t.Parallel()

	p := ProfileFromOrganisation(orgcore.Organisation{OrgNumber: "974760673", Bankrupt: true})
	p.CPV = []string{"72000000"}
	if a := assess(tender(), p, Balanced); a.Verdict != Fail || check(t, a, RuleCompany).Outcome != Fail {
		t.Errorf("verdict = %v", a.Verdict)
	}
	if p := ProfileFromOrganisation(orgcore.Organisation{OrgNumber: "974760673"}); p.Blocked || p.OrgNumber != "974760673" {
		t.Errorf("a healthy company's profile = %+v", p)
	}
}

func TestAnExcludedCPVAreaFailsTheGate(t *testing.T) {
	t.Parallel()

	p := profile()
	p.ExcludedCPV = []string{"72200000"} // the notice's 72212000 is inside it
	if a := assess(tender(), p, Wide); a.Verdict != Fail || check(t, a, RuleExcluded).Outcome != Fail {
		t.Errorf("verdict = %v", a.Verdict)
	}
	p.ExcludedCPV = []string{"72300000"} // a sibling area: no effect
	if a := assess(tender(), p, Wide); a.Verdict != Pass {
		t.Errorf("a sibling exclusion changed the verdict to %v", a.Verdict)
	}
}

func requirement(text string) eforms.Notice {
	n := tender()
	n.SelectionCriteria = []eforms.SelectionCriterion{{Lot: "LOT-0001", Type: "other", Name: "Krav", Description: text}}
	return n
}

func TestRequirementsAreJudgedByLeadTime(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		text string
		held []Capability
		days int
		want Outcome
	}{
		"held":                           {"Leverandøren skal være registrert i StartBANK.", []Capability{CapStartBank}, 20, Pass},
		"obtainable before the deadline": {"Leverandøren skal være registrert i StartBANK.", nil, 20, Conditional},
		"not obtainable in time":         {"Leverandøren skal være registrert i StartBANK.", nil, 5, Fail},
		"declaration takes hours":        {"Det kreves HMS-egenerklæring.", nil, 1, Conditional},
		"ISO not in time but equivalent": {"Leverandøren skal ha ISO 9001 eller tilsvarende.", nil, 20, Conditional},
		"not recognised":                 {"Leverandøren skal ha relevant erfaring fra tilsvarende oppdrag.", nil, 20, Unknown},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			n := requirement(tc.text)
			n.Lots[0].Deadline = at(tc.days)
			p := profile()
			p.Held = map[Capability]bool{}
			for _, c := range tc.held {
				p.Held[c] = true
			}
			a := assess(n, p, Balanced)
			c := check(t, a, RuleRequirement)
			if c.Outcome != tc.want {
				t.Errorf("requirement = %+v, want %v", c, tc.want)
			}
			if c.Cite != (Cite{Field: "selection criterion", Lot: "LOT-0001", Index: 0}) {
				t.Errorf("cite = %+v", c.Cite)
			}
			if tc.want == Fail && a.Verdict != Fail {
				t.Errorf("verdict = %v, want fail", a.Verdict)
			}
		})
	}
}

func TestAProfileLeadTimeOverridesTheDefault(t *testing.T) {
	t.Parallel()

	n := requirement("Leverandøren skal være registrert i StartBANK.")
	n.Lots[0].Deadline = at(5)
	p := profile()
	p.LeadTimes = map[Capability]LeadTime{CapStartBank: {Days: 2}}
	if c := check(t, assess(n, p, Balanced), RuleRequirement); c.Outcome != Conditional {
		t.Errorf("requirement = %+v, want conditional with a 2-day lead time", c)
	}
}

func TestStrictnessWidensCPVAndRegion(t *testing.T) {
	t.Parallel()

	n := tender()
	n.CPVMain = []string{"72310000"} // a sibling of the profile's 72212000 under 72000000
	n.NUTS = []string{"NO0A2"}       // outside the profile's NO081, inside Norway
	p := profile()
	p.CPV = []string{"72212000"}
	p.NUTS = []string{"NO081"}
	p.Keywords = nil

	if a := assess(n, p, Strict); a.Band != None {
		t.Errorf("strict band = %v, want none", a.Band)
	}
	if a := assess(n, p, Wide); a.Band == None {
		t.Errorf("wide band = none; checks %+v", a.Fit)
	}
}

func TestANationwideNoticeIsInsideEveryRegion(t *testing.T) {
	t.Parallel()

	n := tender()
	n.NUTS = []string{"NO0"}
	p := profile()
	p.NUTS = []string{"NO081"}
	if c := check(t, assess(n, p, Strict), RuleRegion); c.Outcome != Pass {
		t.Errorf("region = %+v, want pass", c)
	}
}

func TestValueOutsideTheRangeLowersTheBand(t *testing.T) {
	t.Parallel()

	n := tender()
	n.EstimatedValue = nok(50_000_000)
	a := assess(n, profile(), Balanced)
	if c := check(t, a, RuleValue); c.Outcome != Fail {
		t.Errorf("value = %+v", c)
	}
	if a.Band != Stretch {
		t.Errorf("band = %v, want stretch", a.Band)
	}
	n.EstimatedValue = nil
	if c := check(t, assess(n, profile(), Balanced), RuleValue); c.Outcome != Unknown {
		t.Errorf("no value: %+v, want unknown", c)
	}
}

func TestKeywordsAloneMakeAGoodMatch(t *testing.T) {
	t.Parallel()

	n := tender()
	n.CPVMain = []string{"48000000"}
	a := assess(n, profile(), Balanced)
	if c := check(t, a, RuleKeywords); c.Outcome != Pass {
		t.Errorf("keywords = %+v", c)
	}
	if a.Band != Good {
		t.Errorf("band = %v, want good", a.Band)
	}
}

func TestGainedCountsWhatWideningAdds(t *testing.T) {
	t.Parallel()

	near := tender()
	near.CPVMain = []string{"72310000"}
	far := tender()
	far.CPVMain = []string{"03000000"}
	p := profile()
	p.CPV = []string{"72212000"}
	p.Keywords = nil

	candidates := []Candidate{{Notice: tender()}, {Notice: near}, {Notice: far}}
	for i := range candidates {
		candidates[i].Norm = normalize.Notice(candidates[i].Notice)
	}
	if got := Gained(candidates, p, Strict, Wide, now); got != 1 {
		t.Errorf("Gained = %d, want 1 (the sibling CPV)", got)
	}
}

func TestALeadTimeEqualToTheDaysLeftIsStillInTime(t *testing.T) {
	t.Parallel()

	n := requirement("Leverandøren skal være registrert i StartBANK.")
	n.Lots[0].Deadline = at(DefaultLeadTimes[CapStartBank].Days)
	if c := check(t, assess(n, profile(), Balanced), RuleRequirement); c.Outcome != Conditional {
		t.Errorf("requirement = %+v, want conditional on the boundary", c)
	}
}

func TestEachStrictnessLevelReachesExactlyItsOwnDistance(t *testing.T) {
	t.Parallel()

	p := profile()
	p.CPV = []string{"72212000"}
	p.NUTS = []string{"NO081"}
	for name, tc := range map[string]struct {
		rule, code string
		s          Strictness
		want       Outcome
	}{
		"cpv same group, strict":           {RuleCPV, "72250000", Strict, Fail},
		"cpv same group, balanced":         {RuleCPV, "72250000", Balanced, Conditional},
		"cpv same division, balanced":      {RuleCPV, "72310000", Balanced, Fail},
		"cpv same division, wide":          {RuleCPV, "72310000", Wide, Conditional},
		"cpv other division, wide":         {RuleCPV, "48000000", Wide, Fail},
		"region same NUTS 2, strict":       {RuleRegion, "NO082", Strict, Fail},
		"region same NUTS 2, balanced":     {RuleRegion, "NO082", Balanced, Conditional},
		"region other NUTS 2, balanced":    {RuleRegion, "NO0A2", Balanced, Fail},
		"region other NUTS 2, wide":        {RuleRegion, "NO0A2", Wide, Conditional},
		"region other country, wide":       {RuleRegion, "SE110", Wide, Fail},
		"region inside profile, strict":    {RuleRegion, "NO081", Strict, Pass},
		"cpv inside the profile, strict":   {RuleCPV, "72212100", Strict, Pass},
		"cpv covering the profile, strict": {RuleCPV, "72200000", Strict, Pass},
	} {
		n := tender()
		if tc.rule == RuleCPV {
			n.CPVMain = []string{tc.code}
		} else {
			n.NUTS = []string{tc.code}
		}
		if c := check(t, assess(n, p, tc.s), tc.rule); c.Outcome != tc.want {
			t.Errorf("%s: %+v, want %v", name, c, tc.want)
		}
	}
}

func TestTheValueRangeWidensWithStrictness(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		value int64
		s     Strictness
		want  Outcome
	}{
		"just under the floor, strict":     {150_000, Strict, Fail},
		"just under the floor, balanced":   {150_000, Balanced, Pass},
		"under half the floor, balanced":   {90_000, Balanced, Fail},
		"under half the floor, wide":       {90_000, Wide, Pass},
		"over the ceiling, strict":         {8_000_000, Strict, Fail},
		"over the ceiling, balanced":       {8_000_000, Balanced, Pass},
		"over twice the ceiling, balanced": {11_000_000, Balanced, Fail},
		"far over the ceiling, wide":       {500_000_000, Wide, Pass},
	} {
		n := tender()
		n.EstimatedValue = nok(tc.value)
		if c := check(t, assess(n, profile(), tc.s), RuleValue); c.Outcome != tc.want {
			t.Errorf("%s: %+v, want %v", name, c, tc.want)
		}
	}
}

func TestExclusionGroundsAreOneSelfDeclaredCheck(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Requirements = []eforms.Requirement{
		{Type: "exg-crim-fraud"}, {Type: "exg-sitn-bankr"}, {Type: "exg-pmt-bre-tax"}, {Type: "epo-notice"}, {Type: "none"},
	}
	a := assess(n, profile(), Balanced)
	c := check(t, a, RuleExclusion)
	if c.Outcome != Pass || c.Detail != "3 exclusion grounds; the company declares them in the ESPD" {
		t.Errorf("exclusion = %+v", c)
	}
	if a.Verdict != Pass {
		t.Errorf("verdict = %v: exclusion grounds and pointers to the notice alone do not hold a notice back", a.Verdict)
	}
	for _, g := range a.Gate {
		if g.Rule == RuleRequirement {
			t.Errorf("an exclusion ground or pointer became a requirement check: %+v", g)
		}
	}
}

func TestCriteriaInTheProcurementDocumentsAreUnknown(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Requirements = []eforms.Requirement{{Lot: "LOT-0001", Type: "epo-procurement-document"}}
	a := assess(n, profile(), Balanced)
	c := check(t, a, RuleDocuments)
	if c.Outcome != Unknown || c.Cite != (Cite{Field: "requirement", Lot: "LOT-0001", Index: 0}) {
		t.Errorf("documents = %+v", c)
	}
	if a.Verdict != Conditional {
		t.Errorf("verdict = %v, want conditional until the documents are read", a.Verdict)
	}
}

func TestTheCriterionNameIsReadToo(t *testing.T) {
	t.Parallel()

	n := tender()
	n.SelectionCriteria = []eforms.SelectionCriterion{{Lot: "LOT-0001", Name: "StartBANK", Description: "Registrering kreves."}}
	if c := check(t, assess(n, profile(), Balanced), RuleRequirement); c.Outcome != Conditional {
		t.Errorf("requirement = %+v, want the name to be recognised", c)
	}
}

func TestAParticipationDeadlineIsTheDeadlineThatCounts(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots[0].Deadline = nil
	n.Lots[0].ParticipationDeadline = at(20)
	a := assess(n, profile(), Balanced)
	c := check(t, a, RuleDeadline)
	if c.Outcome != Pass || c.DaysLeft == nil || *c.DaysLeft != 20 {
		t.Errorf("deadline = %+v, want pass with 20 days left", c)
	}
	if c.Cite != (Cite{Field: "participation deadline", Lot: "LOT-0001"}) || !strings.Contains(c.Detail, "request to participate") {
		t.Errorf("cite %+v detail %q: the deadline must say it is for requests to participate", c.Cite, c.Detail)
	}
}

// Tenders are invited only from those who asked to take part, so a passed participation deadline
// closes the lot even when a later tender deadline is stated.
func TestAPassedParticipationDeadlineClosesTheLot(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots[0].Deadline = at(20)
	n.Lots[0].ParticipationDeadline = at(-1)
	if a := assess(n, profile(), Balanced); a.Verdict != Fail || check(t, a, RuleDeadline).Outcome != Fail {
		t.Errorf("verdict = %v, want fail", a.Verdict)
	}
}

func TestTheDeadlineCheckNamesTheLotWhoseDeadlineIsNearest(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots = []eforms.Lot{{ID: "LOT-A", Deadline: at(30)}, {ID: "LOT-B", Deadline: at(7)}, {ID: "LOT-C", Deadline: at(-2)}}
	c := check(t, assess(n, profile(), Balanced), RuleDeadline)
	if c.Cite.Lot != "LOT-B" || c.DaysLeft == nil || *c.DaysLeft != 7 {
		t.Errorf("deadline = %+v, want the lot due in 7 days", c)
	}
}

// "+N if you widen" counts tenders a company could still bid on. A notice whose deadline has
// passed is not one, however well it would fit.
func TestGainedDoesNotCountNoticesTheGateFails(t *testing.T) {
	t.Parallel()

	near := tender()
	near.CPVMain = []string{"72310000"}
	closed := near
	closed.Lots = []eforms.Lot{{ID: "LOT-0001", Deadline: at(-5)}}
	p := profile()
	p.CPV = []string{"72212000"}
	p.Keywords = nil

	candidates := []Candidate{{Notice: near}, {Notice: closed}}
	for i := range candidates {
		candidates[i].Norm = normalize.Notice(candidates[i].Notice)
	}
	if got := Gained(candidates, p, Strict, Wide, now); got != 1 {
		t.Errorf("Gained = %d, want 1: the notice that is closed does not count", got)
	}
}

func TestNextOpenDeadlineIsTheEarliestOneNotYetPassed(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots = []eforms.Lot{{ID: "LOT-A", Deadline: at(30)}, {ID: "LOT-B", Deadline: at(7)}, {ID: "LOT-C", Deadline: at(-2)}}
	d, ok := NextOpenDeadline(normalize.Notice(n), now)
	if !ok || d.Lot != "LOT-B" || !d.At.Equal(*at(7)) {
		t.Errorf("next = %+v, %v; want LOT-B in 7 days", d, ok)
	}

	// A deadline that falls exactly now is still open; one a second earlier is not.
	edge := tender()
	edge.Lots = []eforms.Lot{{ID: "LOT-A", Deadline: &now}}
	if _, ok := NextOpenDeadline(normalize.Notice(edge), now); !ok {
		t.Errorf("a deadline that falls now is closed")
	}
	past := now.Add(-time.Second)
	edge.Lots = []eforms.Lot{{ID: "LOT-A", Deadline: &past}}
	if _, ok := NextOpenDeadline(normalize.Notice(edge), now); ok {
		t.Errorf("a deadline that passed a second ago is open")
	}
	if _, ok := NextOpenDeadline(normalize.Notice(eforms.Notice{}), now); ok {
		t.Errorf("a notice with no deadline has a next deadline")
	}
}

// ValueOf is the value a notice is judged and shown by: its own estimate, else its largest lot's.
func TestValueOfPrefersTheNoticeEstimateThenTheLargestLot(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots = []eforms.Lot{{ID: "A", Value: nok(100)}, {ID: "B", Value: nok(900)}, {ID: "C", Value: nok(300)}}
	if v := ValueOf(n); v == nil || v.Minor != 100_000_000 {
		t.Errorf("with an estimate: %+v, want the notice's 1 000 000", v)
	}
	n.EstimatedValue = nil
	if v := ValueOf(n); v == nil || v.Minor != 90_000 {
		t.Errorf("without one: %+v, want the largest lot's 900", v)
	}
	n.Lots = []eforms.Lot{{ID: "A"}}
	if v := ValueOf(n); v != nil {
		t.Errorf("with no value anywhere: %+v", v)
	}
}

// A notice that points to the tender documents once for each lot says one thing: the criteria are
// not in the notice. It is one check, not one for each lot, so a person reads it once.
func TestSeveralPointersToTheDocumentsAreOneCheck(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Requirements = []eforms.Requirement{
		{Lot: "", Type: "exg-crim-fraud"},
		{Lot: "LOT-0001", Type: eforms.RequirementInTenderDocuments},
		{Lot: "LOT-0002", Type: eforms.RequirementInTenderDocuments},
		{Lot: "LOT-0003", Type: eforms.RequirementInTenderDocuments},
	}
	a := assess(n, profile(), Balanced)
	var found []Check
	for _, c := range a.Gate {
		if c.Rule == RuleDocuments {
			found = append(found, c)
		}
	}
	if len(found) != 1 {
		t.Fatalf("checks = %+v, want exactly one", found)
	}
	c := found[0]
	want := "the selection criteria are in the tender documents, which are not read yet (3 entries of the notice say so)"
	if c.Outcome != Unknown || c.Detail != want || c.Cite != (Cite{Field: "requirement", Lot: "LOT-0001", Index: 1}) {
		t.Errorf("check = %+v\nwant detail %q and the first pointer cited", c, want)
	}
}

// A notice that lists no exclusion ground has no check for them: "0 exclusion grounds declared" would
// read as an assurance nobody gave.
func TestNoticeWithoutExclusionGroundsHasNoSuchCheck(t *testing.T) {
	t.Parallel()

	for name, requirements := range map[string][]eforms.Requirement{
		"none at all":        nil,
		"only a pointer":     {{Lot: "LOT-0001", Type: eforms.RequirementInTenderDocuments}},
		"only another entry": {{Type: "epo-notice"}, {Type: "none"}},
	} {
		n := tender()
		n.Requirements = requirements
		for _, c := range assess(n, profile(), Balanced).Gate {
			if c.Rule == RuleExclusion {
				t.Errorf("%s: a notice with no exclusion ground got %+v", name, c)
			}
		}
	}
}
