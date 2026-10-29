package demo

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/rawstore"
)

func planningCorpus() []rawstore.Document {
	return []rawstore.Document{
		doc("2026-100010", eformstest.PriorInformationNotice(eformstest.Notice{
			ID: "00000000-0000-4000-8000-0000000000p1", Title: "Ny barnehage", PlannedNotice: time.Date(2027, 4, 13, 0, 0, 0, 0, time.UTC),
		})),
		doc("2026-100011", eformstest.ContractNotice(eformstest.Notice{
			ID: "00000000-0000-4000-8000-0000000000c1", Title: "Drift av fagsystem", Deadline: now.AddDate(0, 0, 20),
		})),
	}
}

// A prior information notice is listed as planning, with the day its notice is planned for, and it is
// not judged: the gate is for tenders that are open to bid on.
func TestAPriorInformationNoticeIsListedAsPlanning(t *testing.T) {
	s := newService(t, &pagedNotices{fakeNotices{docs: planningCorpus()}}, nil)
	got, err := s.Catalogue(context.Background(), CatalogueRequest{NoticeType: "planning"})
	if err != nil {
		t.Fatalf("Catalogue: %v", err)
	}
	if len(got.Results) != 1 || got.Results[0].NoticeID != "2026-100010" || got.Results[0].Title != "Ny barnehage" || got.Results[0].PlannedNotice != "2027-04-13" {
		t.Errorf("planning results %+v; want the prior information notice with its planned day", got.Results)
	}
}

func TestMatchesCountAPriorInformationNoticeAndDoNotGateIt(t *testing.T) {
	req, problems := decode(t, `{"profile":{"cpv":["72212000"]}}`)
	if len(problems) > 0 {
		t.Fatalf("decode: %v", problems)
	}
	got, err := newService(t, &fakeNotices{docs: planningCorpus()}, nil).Matches(ctx(), req)
	if err != nil {
		t.Fatalf("Matches: %v", err)
	}
	if got.Funnel.Planning != 1 || got.Funnel.Opportunities != 1 || got.Funnel.Unsupported != 0 {
		t.Errorf("funnel %+v; want one planning notice, one opportunity, nothing unsupported", got.Funnel)
	}
	for _, r := range got.Results {
		if r.NoticeID == "2026-100010" {
			t.Error("the prior information notice was judged as a tender")
		}
	}
}

func TestTheAuditSheetSaysAPriorInformationNoticeIsNothingToBidOn(t *testing.T) {
	req, _ := decode(t, `{"profile":{"cpv":["72212000"]}}`)
	_, err := newService(t, &fakeNotices{docs: planningCorpus()}, nil).Audit(ctx(), "2026-100010", req)
	if !errors.Is(err, ErrNotAnOpportunity) {
		t.Errorf("err %v, want ErrNotAnOpportunity", err)
	}
}
