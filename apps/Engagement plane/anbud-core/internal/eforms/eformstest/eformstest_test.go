package eformstest_test

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
)

func TestContractNoticeParsesWithTheFieldsItWasGiven(t *testing.T) {
	t.Parallel()

	deadline := time.Date(2026, time.November, 20, 11, 0, 0, 0, time.UTC)
	n, err := eforms.Parse(eformstest.ContractNotice(eformstest.Notice{
		ID: "00000000-0000-4000-8000-0000000000a1", Title: "Drift & forvaltning <test>", CPV: "72212000", NUTS: "NO081",
		ValueNOK: 1_500_000, Buyer: "Eksempelkommune", BuyerOrg: "974760673", Deadline: deadline,
		Criteria: []string{"Leverandøren skal være registrert i StartBANK."}, PointToDocuments: true,
	}))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Root != "ContractNotice" || n.SDKVersion != "1.14" || n.NoticeID != "00000000-0000-4000-8000-0000000000a1" || n.Title != "Drift & forvaltning <test>" {
		t.Errorf("header or title: root %q sdk %q id %q title %q", n.Root, n.SDKVersion, n.NoticeID, n.Title)
	}
	if !slices.Equal(n.CPVMain, []string{"72212000"}) || !slices.Equal(n.NUTS, []string{"NO081"}) || !slices.Equal(n.BuyerOrgNumbers, []string{"974760673"}) {
		t.Errorf("cpv %v nuts %v buyer %v", n.CPVMain, n.NUTS, n.BuyerOrgNumbers)
	}
	if n.EstimatedValue == nil || n.EstimatedValue.Minor != 150_000_000 {
		t.Errorf("value = %+v", n.EstimatedValue)
	}
	if len(n.Lots) != 1 || n.Lots[0].Deadline == nil || !n.Lots[0].Deadline.Equal(deadline) {
		t.Errorf("lots = %+v", n.Lots)
	}
	if len(n.SelectionCriteria) != 1 || len(n.Issues) != 0 || n.ContactPoints != 1 {
		t.Errorf("criteria %v issues %v contacts %d", n.SelectionCriteria, n.Issues, n.ContactPoints)
	}
	pointers := 0
	for _, r := range n.Requirements {
		if r.Type == "epo-procurement-document" {
			pointers++
		}
	}
	if pointers != 1 {
		t.Errorf("requirements = %+v, want one pointer to the tender documents", n.Requirements)
	}
}

func TestOptionsOmitWhatIsNotGiven(t *testing.T) {
	t.Parallel()

	n, err := eforms.Parse(eformstest.ContractNotice(eformstest.Notice{}))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.EstimatedValue != nil || n.Lots[0].Deadline != nil || n.Lots[0].ParticipationDeadline != nil || len(n.SelectionCriteria) != 0 {
		t.Errorf("value %v deadline %v participation %v criteria %d: nothing was asked for", n.EstimatedValue, n.Lots[0].Deadline, n.Lots[0].ParticipationDeadline, len(n.SelectionCriteria))
	}
	if n.Title == "" || len(n.CPVMain) != 1 || len(n.NUTS) != 1 {
		t.Errorf("the defaults are missing: %+v", n)
	}
}

func TestAShapeThatHasNoFieldMapIsRefused(t *testing.T) {
	t.Parallel()

	if _, err := eforms.Parse(eformstest.ContractNotice(eformstest.Notice{SDK: "1.7"})); err == nil {
		t.Errorf("SDK 1.7 parsed: the fixture is meant to stand for a shape with no field map")
	}
	if n, err := eforms.Parse(eformstest.ContractNotice(eformstest.Notice{SDK: "1.13"})); err != nil || n.SDKVersion != "1.13" {
		t.Errorf("SDK 1.13: %v %q", err, n.SDKVersion)
	}
}

func TestAwardNoticeParsesAsAResultNotice(t *testing.T) {
	t.Parallel()

	n, err := eforms.Parse(eformstest.AwardNotice(eformstest.Notice{Title: "Tildelt"}))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Root != "ContractAwardNotice" || n.Title != "Tildelt" {
		t.Errorf("root %q title %q", n.Root, n.Title)
	}
}

func TestTheContactPersonIsInTheBodyForTestsToLookFor(t *testing.T) {
	t.Parallel()

	body := string(eformstest.ContractNotice(eformstest.Notice{}))
	for _, marker := range eformstest.PersonalMarkers {
		if !strings.Contains(body, marker) {
			t.Errorf("the fixture does not carry %q, so a test for its absence proves nothing", marker)
		}
	}
}

// An award with a result parses with its winner, tender count, award day and contract period, under
// the procedure the fixture names.
func TestAwardWithResultParsesWithItsWinner(t *testing.T) {
	t.Parallel()

	awarded := time.Date(2025, time.March, 3, 0, 0, 0, 0, time.UTC)
	n, err := eforms.Parse(eformstest.AwardWithResult(
		eformstest.Notice{ProcedureID: "00000000-0000-4000-8000-0000000000a1", CPV: "72212000"},
		eformstest.Award{WinnerName: "Fiktiv Leverandør AS", WinnerOrg: "111111111", Tenders: 4, AwardDate: awarded, DurationMonths: 24},
	))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.ProcedureID != "00000000-0000-4000-8000-0000000000a1" || n.AwardDate == nil || !n.AwardDate.Equal(awarded) {
		t.Errorf("procedure %q award date %v", n.ProcedureID, n.AwardDate)
	}
	if len(n.Results) != 1 || len(n.Results[0].Winners) != 1 || n.Results[0].Winners[0] != (eforms.Winner{Name: "Fiktiv Leverandør AS", OrgNumber: "111111111"}) {
		t.Fatalf("results = %+v", n.Results)
	}
	if c := n.Results[0].TendersReceived; c == nil || *c != 4 {
		t.Errorf("tenders = %v", c)
	}
	if p := n.Lots[0].Period; p == nil || p.Duration == nil || *p.Duration != (eforms.Duration{Value: 24, Unit: "MONTH"}) {
		t.Errorf("period = %+v", p)
	}
	if !slices.Equal(n.BuyerOrgNumbers, []string{"974760673"}) {
		t.Errorf("the winner is taken for the buyer: %v", n.BuyerOrgNumbers)
	}
	if len(n.Issues) != 0 {
		t.Errorf("issues: %v", n.Issues)
	}
}

// A fixture can be issued at a given time and can be a change notice, laid out as the captured change
// notices are, so that what consumes the parser can be tested with versions of one procedure.
func TestChangeNoticeFixtureParsesWithItsIssueTimeAndChange(t *testing.T) {
	t.Parallel()

	issued := time.Date(2026, time.October, 8, 9, 30, 0, 0, time.UTC)
	n, err := eforms.Parse(eformstest.ContractNotice(eformstest.Notice{
		ProcedureID: "00000000-0000-4000-8000-0000000000b1", IssuedAt: issued,
		Change: &eformstest.Change{Replaces: "0cdb6ab9-aaaa-4bbb-8ccc-0123456789ab-01", Reason: "Fristen er utsatt.", Sections: []string{"LOT-0001"}},
	}))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.IssuedAt == nil || !n.IssuedAt.Equal(issued) {
		t.Errorf("issued at = %v, want %v", n.IssuedAt, issued)
	}
	c := n.Change
	if c == nil || c.Replaces != "0cdb6ab9-aaaa-4bbb-8ccc-0123456789ab-01" || c.ReasonCode != "update-add" || c.Reason != "Fristen er utsatt." || !slices.Equal(c.Sections, []string{"LOT-0001"}) {
		t.Errorf("change = %+v", c)
	}
	if len(n.Issues) != 0 {
		t.Errorf("issues: %v", n.Issues)
	}
	plain, err := eforms.Parse(eformstest.ContractNotice(eformstest.Notice{}))
	if err != nil || plain.Change != nil || plain.IssuedAt != nil {
		t.Errorf("a fixture with neither has change %+v issued %v (err %v)", plain.Change, plain.IssuedAt, err)
	}
}
