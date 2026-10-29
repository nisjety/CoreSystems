package incumbency

import (
	"slices"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

func day(y int, m time.Month, d int) *time.Time {
	t := time.Date(y, m, d, 0, 0, 0, 0, time.UTC)
	return &t
}

func intp(n int) *int { return &n }

// result builds a parsed award notice with one awarded lot. Every name and number is invented.
type result struct {
	notice, procedure string
	buyer             string
	cpv               string
	lotCPV            string
	awarded, signed   *time.Time
	period            *eforms.ContractPeriod
	renewals          *int
	winners           []eforms.Winner
	tenders           *int
	status            string
}

func (r result) notice_() eforms.Notice {
	status := r.status
	if status == "" {
		status = "selec-w"
	}
	n := eforms.Notice{
		Header:          eforms.Header{Root: "ContractAwardNotice", NoticeID: "uuid-" + r.notice, ProcedureID: r.procedure},
		Title:           "Fiktiv anskaffelse",
		CPVMain:         []string{r.cpv},
		NUTS:            []string{"NO081"},
		BuyerNames:      []string{"Eksempelkommune"},
		BuyerOrgNumbers: []string{r.buyer},
		AwardDate:       r.awarded,
		Lots: []eforms.Lot{{
			ID: "LOT-0001", Title: "Delkontrakt 1", Period: r.period, Renewals: r.renewals,
		}},
		Results: []eforms.LotResult{{
			Lot: "LOT-0001", Status: status, Winners: r.winners, TendersReceived: r.tenders,
			Value: &eforms.Amount{Minor: 100_000_00, Currency: "NOK"}, ContractSigned: r.signed,
		}},
	}
	if r.lotCPV != "" {
		n.Lots[0].CPVMain = []string{r.lotCPV}
	}
	return n
}

var acme = eforms.Winner{Name: "Fiktiv Leverandør AS", OrgNumber: "111111111"}

func TestFromNoticeTakesEachAwardedLot(t *testing.T) {
	t.Parallel()

	r := result{notice: "2025-100001", procedure: "proc-1", buyer: "974760673", cpv: "72000000", lotCPV: "72212000",
		awarded: day(2025, time.March, 3), signed: day(2025, time.March, 20), renewals: intp(2),
		period:  &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 24, Unit: "MONTH"}},
		winners: []eforms.Winner{acme}, tenders: intp(4)}
	awards := FromNotice("2025-100001", r.notice_())
	if len(awards) != 1 {
		t.Fatalf("awards = %d, want 1", len(awards))
	}
	a := awards[0]
	if a.NoticeID != "2025-100001" || a.ProcedureID != "proc-1" || a.LotTitle != "Delkontrakt 1" {
		t.Errorf("identity = %q %q %q", a.NoticeID, a.ProcedureID, a.LotTitle)
	}
	if !slices.Equal(a.CPV, []string{"72212000"}) {
		t.Errorf("CPV = %v, want the lot's own code before the notice's", a.CPV)
	}
	if !slices.Equal(a.BuyerOrgNumbers, []string{"974760673"}) || a.TendersReceived == nil || *a.TendersReceived != 4 {
		t.Errorf("buyer %v tenders %v", a.BuyerOrgNumbers, a.TendersReceived)
	}
	if a.Renewals == nil || *a.Renewals != 2 || a.Period == nil {
		t.Errorf("the lot's period and renewals are not carried: %+v %v", a.Period, a.Renewals)
	}
}

// A lot with no winner chosen, a lot result for a lot the notice does not describe, and a notice
// that is not a result notice give no award.
func TestFromNoticeSkipsWhatIsNotAnAward(t *testing.T) {
	t.Parallel()

	closed := result{notice: "2025-100002", buyer: "974760673", cpv: "72000000", status: "clos-nw"}
	if got := FromNotice("2025-100002", closed.notice_()); len(got) != 0 {
		t.Errorf("a lot closed without a winner gave %d awards", len(got))
	}
	noWinner := result{notice: "2025-100003", buyer: "974760673", cpv: "72000000"}
	if got := FromNotice("2025-100003", noWinner.notice_()); len(got) != 0 {
		t.Errorf("a result with no winner the parser could follow gave %d awards", len(got))
	}
	cn := result{notice: "2025-100004", buyer: "974760673", cpv: "72000000", winners: []eforms.Winner{acme}}.notice_()
	cn.Root = "ContractNotice"
	if got := FromNotice("2025-100004", cn); len(got) != 0 {
		t.Errorf("a contract notice gave %d awards", len(got))
	}
}

// When a contract ends: as stated, or estimated from a start, a signing date or the award date
// plus the duration, in that order of preference; open when the notice says it has no end; and
// unknown otherwise. An estimate is never presented as a stated date.
func TestEndOfAContract(t *testing.T) {
	t.Parallel()

	months := func(n int) *eforms.Duration { return &eforms.Duration{Value: n, Unit: "MONTH"} }
	cases := []struct {
		name  string
		a     Award
		at    *time.Time
		basis EndBasis
	}{
		{"stated", Award{Period: &eforms.ContractPeriod{Start: day(2025, 1, 1), End: day(2026, 12, 31), Duration: months(6)}}, day(2026, 12, 31), EndStated},
		{"start and duration", Award{Period: &eforms.ContractPeriod{Start: day(2025, 10, 1), Duration: months(24)}, Signed: day(2025, 9, 1)}, day(2027, 10, 1), EndFromStart},
		{"signed and duration", Award{Period: &eforms.ContractPeriod{Duration: months(12)}, Signed: day(2025, 9, 1), AwardDate: day(2025, 8, 1)}, day(2026, 9, 1), EndFromSigned},
		{"award and duration", Award{Period: &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 2, Unit: "YEAR"}}, AwardDate: day(2025, 8, 1)}, day(2027, 8, 1), EndFromAward},
		{"weeks", Award{Period: &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 2, Unit: "WEEK"}}, AwardDate: day(2025, 8, 1)}, day(2025, 8, 15), EndFromAward},
		{"days", Award{Period: &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 10, Unit: "DAY"}}, AwardDate: day(2025, 8, 1)}, day(2025, 8, 11), EndFromAward},
		{"open", Award{Period: &eforms.ContractPeriod{Unlimited: true}, AwardDate: day(2025, 8, 1)}, nil, EndOpen},
		{"no period", Award{AwardDate: day(2025, 8, 1)}, nil, EndUnknown},
		{"a duration and nothing to start from", Award{Period: &eforms.ContractPeriod{Duration: months(12)}}, nil, EndUnknown},
	}
	for _, c := range cases {
		got := c.a.End()
		if got.Basis != c.basis || (c.at == nil) != (got.At == nil) || (c.at != nil && !got.At.Equal(*c.at)) {
			t.Errorf("%s: End() = %v %v, want %v %v", c.name, got.At, got.Basis, c.at, c.basis)
		}
	}
}
