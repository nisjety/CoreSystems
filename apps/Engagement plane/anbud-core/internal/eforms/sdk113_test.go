package eforms

import (
	"errors"
	"slices"
	"testing"
	"time"
)

// as113 turns the synthetic 1.14 notice into the same notice under SDK 1.13, flavour eu. Every
// path the parser reads is the same in both, which the captured 1.13 notices confirm (counts in
// docs/SOURCES.md), so one fixture serves both.
func as113(pairs ...string) []byte {
	return sample(append([]string{"eforms-sdk-1.14#", "eforms-sdk-1.13#"}, pairs...)...)
}

func TestParseReadsAnSDK113ContractNotice(t *testing.T) {
	t.Parallel()

	n, err := Parse(as113())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.SDKVersion != "1.13" || n.Flavour != FlavourEU || n.Title != "Eksempeltjeneste" {
		t.Errorf("sdk %q flavour %q title %q", n.SDKVersion, n.Flavour, n.Title)
	}
	if !slices.Equal(n.BuyerOrgNumbers, []string{"000000000"}) || len(n.Lots) != 1 || len(n.Issues) != 0 {
		t.Errorf("buyer %v lots %d issues %v", n.BuyerOrgNumbers, len(n.Lots), n.Issues)
	}
}

// Unknown Norwegian extensions remain unsupported.
func TestParseRefusesAnSDK113FlavourNobodyHasChecked(t *testing.T) {
	t.Parallel()

	if _, err := Parse(as113("eforms:eu", "eforms:future")); !errors.Is(err, ErrNoFieldMap) {
		t.Errorf("err = %v, want ErrNoFieldMap", err)
	}
}

// A restricted procedure states the deadline for requests to participate, not one for tenders.
func TestParseReadsTheDeadlineForRequestsToParticipate(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample(
		"<cac:TenderSubmissionDeadlinePeriod><cbc:EndDate>2026-11-15", "<cac:ParticipationRequestReceptionPeriod><cbc:EndDate>2026-11-15",
		"12:00:00+01:00</cbc:EndTime></cac:TenderSubmissionDeadlinePeriod>", "12:00:00+01:00</cbc:EndTime></cac:ParticipationRequestReceptionPeriod>",
		"<cac:TenderSubmissionDeadlinePeriod><cbc:EndDate>2026-11-20", "<cac:ParticipationRequestReceptionPeriod><cbc:EndDate>2026-11-20",
		"23:59:00Z</cbc:EndTime></cac:TenderSubmissionDeadlinePeriod>", "23:59:00Z</cbc:EndTime></cac:ParticipationRequestReceptionPeriod>",
	))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Deadline != nil || n.Lots[0].Deadline != nil {
		t.Errorf("tender deadlines %v %v, want none: only participation periods are stated", n.Deadline, n.Lots[0].Deadline)
	}
	if want := time.Date(2026, time.November, 15, 11, 0, 0, 0, time.UTC); n.ParticipationDeadline == nil || !n.ParticipationDeadline.Equal(want) {
		t.Errorf("participation deadline = %v, want %v", n.ParticipationDeadline, want)
	}
	if want := time.Date(2026, time.November, 20, 23, 59, 0, 0, time.UTC); n.Lots[0].ParticipationDeadline == nil || !n.Lots[0].ParticipationDeadline.Equal(want) {
		t.Errorf("lot participation deadline = %v, want %v", n.Lots[0].ParticipationDeadline, want)
	}
	if len(n.Issues) != 0 {
		t.Errorf("issues = %v", n.Issues)
	}
	if slices.Contains(n.Unmapped, "ContractNotice/TenderingProcess/ParticipationRequestReceptionPeriod") {
		t.Errorf("the participation period is reported unread")
	}
}

func TestParseKeepsBothDeadlinesOfALot(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample("</cac:TenderSubmissionDeadlinePeriod></cac:TenderingProcess>",
		`</cac:TenderSubmissionDeadlinePeriod><cac:ParticipationRequestReceptionPeriod><cbc:EndDate>2026-11-10</cbc:EndDate><cbc:EndTime>10:00:00Z</cbc:EndTime></cac:ParticipationRequestReceptionPeriod></cac:TenderingProcess>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	lot := n.Lots[0]
	if want := time.Date(2026, time.November, 20, 23, 59, 0, 0, time.UTC); lot.Deadline == nil || !lot.Deadline.Equal(want) {
		t.Errorf("lot deadline = %v, want %v", lot.Deadline, want)
	}
	if want := time.Date(2026, time.November, 10, 10, 0, 0, 0, time.UTC); lot.ParticipationDeadline == nil || !lot.ParticipationDeadline.Equal(want) {
		t.Errorf("lot participation deadline = %v, want %v", lot.ParticipationDeadline, want)
	}
}

func TestParseReportsAParticipationDeadlineWithNoZone(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample(
		"<cac:TenderSubmissionDeadlinePeriod><cbc:EndDate>2026-11-20", "<cac:ParticipationRequestReceptionPeriod><cbc:EndDate>2026-11-20",
		"23:59:00Z</cbc:EndTime></cac:TenderSubmissionDeadlinePeriod>", "23:59:00</cbc:EndTime></cac:ParticipationRequestReceptionPeriod>",
	))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Lots[0].ParticipationDeadline != nil || !slices.Contains(n.Issues, "lot 1 participation: deadline time has no zone") {
		t.Errorf("deadline %v issues %v", n.Lots[0].ParticipationDeadline, n.Issues)
	}
}
