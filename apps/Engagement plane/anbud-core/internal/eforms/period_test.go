package eforms

import (
	"strings"
	"testing"
	"time"
)

// periodPlaced puts a lot's contract period, and its renewals, into the first lot of the sample award.
func periodPlaced(period string) []byte {
	return award("<cbc:Name>Delkontrakt 1</cbc:Name>", "<cbc:Name>Delkontrakt 1</cbc:Name>"+period)
}

func day(y int, m time.Month, d int) time.Time { return time.Date(y, m, d, 0, 0, 0, 0, time.UTC) }

// A lot's contract period is read as the notice states it: start and end as calendar days, or a
// duration with its unit. The renewals are the most the buyer may make (BT-58).
func TestParseReadsTheContractPeriodOfALot(t *testing.T) {
	t.Parallel()

	n, err := Parse(periodPlaced(`<cac:PlannedPeriod><cbc:StartDate>2026-10-01+02:00</cbc:StartDate><cbc:EndDate>2028-09-30Z</cbc:EndDate></cac:PlannedPeriod>` +
		`<cac:ContractExtension><cbc:MaximumNumberNumeric>2</cbc:MaximumNumberNumeric></cac:ContractExtension>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	p := n.Lots[0].Period
	if p == nil || p.Start == nil || !p.Start.Equal(day(2026, time.October, 1)) || p.End == nil || !p.End.Equal(day(2028, time.September, 30)) || p.Duration != nil {
		t.Errorf("period = %+v, want 2026-10-01 to 2028-09-30 and no duration", p)
	}
	if r := n.Lots[0].Renewals; r == nil || *r != 2 {
		t.Errorf("renewals = %v, want 2", r)
	}
	if n.Lots[1].Period != nil || n.Lots[1].Renewals != nil {
		t.Errorf("a lot that states no period has one: %+v", n.Lots[1])
	}
	for _, issue := range n.Issues {
		t.Errorf("issue for a well-formed period: %s", issue)
	}
}

func TestParseReadsADurationWithItsUnit(t *testing.T) {
	t.Parallel()

	n, err := Parse(periodPlaced(`<cac:PlannedPeriod><cbc:DurationMeasure unitCode="MONTH">24</cbc:DurationMeasure></cac:PlannedPeriod>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	p := n.Lots[0].Period
	if p == nil || p.Duration == nil || *p.Duration != (Duration{Value: 24, Unit: "MONTH"}) || p.Start != nil || p.End != nil {
		t.Errorf("period = %+v, want a duration of 24 MONTH", p)
	}
}

// A period the notice marks as without end is open, not unknown.
func TestParseReadsAnOpenEndedPeriod(t *testing.T) {
	t.Parallel()

	n, err := Parse(periodPlaced(`<cac:PlannedPeriod><cbc:DescriptionCode listName="timeperiod">UNLIMITED</cbc:DescriptionCode></cac:PlannedPeriod>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if p := n.Lots[0].Period; p == nil || !p.Unlimited {
		t.Errorf("period = %+v, want unlimited", p)
	}
}

// A duration is not guessed at: a unit outside the code list, or a value that is not a whole
// number, is an issue and gives no duration. The issue quotes neither.
func TestParseDoesNotGuessADuration(t *testing.T) {
	t.Parallel()

	for name, period := range map[string]string{
		"an unknown unit":        `<cac:PlannedPeriod><cbc:DurationMeasure unitCode="FORTNIGHT">3</cbc:DurationMeasure></cac:PlannedPeriod>`,
		"no unit":                `<cac:PlannedPeriod><cbc:DurationMeasure>3</cbc:DurationMeasure></cac:PlannedPeriod>`,
		"a fraction":             `<cac:PlannedPeriod><cbc:DurationMeasure unitCode="YEAR">1.5</cbc:DurationMeasure></cac:PlannedPeriod>`,
		"renewals not a count":   `<cac:ContractExtension><cbc:MaximumNumberNumeric>two</cbc:MaximumNumberNumeric></cac:ContractExtension>`,
		"an end that is no date": `<cac:PlannedPeriod><cbc:EndDate>soon</cbc:EndDate></cac:PlannedPeriod>`,
	} {
		n, err := Parse(periodPlaced(period))
		if err != nil {
			t.Fatalf("%s: Parse: %v", name, err)
		}
		lot := n.Lots[0]
		if lot.Renewals != nil || (lot.Period != nil && (lot.Period.Duration != nil || lot.Period.End != nil)) {
			t.Errorf("%s: read as %+v / renewals %v", name, lot.Period, lot.Renewals)
		}
		if len(n.Issues) == 0 {
			t.Errorf("%s: no issue recorded", name)
		}
		for _, issue := range n.Issues {
			for _, value := range []string{"FORTNIGHT", "1.5", "two", "soon"} {
				if strings.Contains(issue, value) {
					t.Errorf("%s: the issue quotes the value: %q", name, issue)
				}
			}
		}
	}
}

// A contract notice states the period of each lot the same way, and it is read the same way.
func TestParseReadsThePeriodOfAContractNoticeLot(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample("<cbc:Name>Delkontrakt 1</cbc:Name>", `<cbc:Name>Delkontrakt 1</cbc:Name><cac:PlannedPeriod><cbc:DurationMeasure unitCode="YEAR">3</cbc:DurationMeasure></cac:PlannedPeriod>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if p := n.Lots[0].Period; p == nil || p.Duration == nil || *p.Duration != (Duration{Value: 3, Unit: "YEAR"}) {
		t.Errorf("period = %+v, want 3 YEAR", p)
	}
}

// The day a contract was concluded (BT-145) is read for the lot result whose contract it is, through
// the contract's identifier.
func TestParseReadsWhenTheContractWasSigned(t *testing.T) {
	t.Parallel()

	n, err := Parse(award())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if s := n.Results[0].ContractSigned; s == nil || !s.Equal(day(2026, time.September, 1)) {
		t.Errorf("contract signed = %v, want 2026-09-01", s)
	}
	if n.Results[1].ContractSigned != nil {
		t.Errorf("a lot with no contract has a signing date: %v", n.Results[1].ContractSigned)
	}
}

// A lot result that refers to a contract the notice does not hold is an issue, not a date.
func TestParseReportsAContractItCannotFind(t *testing.T) {
	t.Parallel()

	n, err := Parse(award(`<efac:SettledContract><cbc:ID schemeName="contract">CON-0001</cbc:ID></efac:SettledContract>
        <efac:TenderLot>`, `<efac:SettledContract><cbc:ID schemeName="contract">CON-9999</cbc:ID></efac:SettledContract>
        <efac:TenderLot>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Results[0].ContractSigned != nil {
		t.Errorf("contract signed = %v for a contract the notice does not hold", n.Results[0].ContractSigned)
	}
	found := false
	for _, issue := range n.Issues {
		if strings.Contains(issue, "contract") {
			found = true
		}
	}
	if !found {
		t.Errorf("no issue for an unknown contract: %v", n.Issues)
	}
}
