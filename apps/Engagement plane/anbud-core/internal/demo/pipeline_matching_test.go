package demo

import (
	"context"
	"fmt"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/rawstore"
	"testing"
)

type trackedByOrg map[string][]string

func (t trackedByOrg) TrackedNoticeIDs(_ context.Context, org string) ([]string, error) {
	return t[org], nil
}

func TestPipelineMatchesAnActiveNoticeBeyondNewest300(t *testing.T) {
	docs := []rawstore.Document{}
	for i := 0; i < 400; i++ {
		docs = append(docs, doc(fmt.Sprintf("2026-%06d", 100000+i), eformstest.ContractNotice(eformstest.Notice{Title: "Agriculture", CPV: "03000000", Deadline: in(4)})))
	}
	docs = append(docs, doc("2026-999999", eformstest.ContractNotice(eformstest.Notice{Title: "Older active IT service", CPV: "72000000", Deadline: in(2)})))
	got, err := newService(t, &pagedNotices{fakeNotices{docs: docs}}, nil).Matches(context.Background(), Request{Profile: match.Profile{CPV: []string{"72000000"}}, Limit: 1, Scan: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(got.Results) != 1 || got.Results[0].NoticeID != "2026-999999" || got.Funnel.Scanned != 401 || got.Coverage != "all_active" {
		t.Fatalf("incomplete matching: %+v", got)
	}
}

func TestTrackedWorkSurvivesMismatchExpiryLimitAndMissingProfile(t *testing.T) {
	docs := []rawstore.Document{
		doc("2026-100001", eformstest.ContractNotice(eformstest.Notice{Title: "Active IT", CPV: "72000000", Deadline: in(5)})),
		doc("2026-100002", eformstest.ContractNotice(eformstest.Notice{Title: "Expired IT", CPV: "72000000", Deadline: in(-5)})),
		doc("2026-100003", eformstest.ContractNotice(eformstest.Notice{Title: "No longer matches", CPV: "03000000", Deadline: in(5)})),
	}
	s := newService(t, &pagedNotices{fakeNotices{docs: docs}}, nil)
	s.tracking = trackedByOrg{"org-a": {"2026-100002", "2026-100003", "2026-199999"}}
	r := Request{Org: "org-a", Profile: match.Profile{CPV: []string{"72000000"}}, Limit: 1}
	got, err := s.Matches(context.Background(), r)
	if err != nil {
		t.Fatal(err)
	}
	if len(got.Results) != 1 || len(got.TrackedResults) != 3 {
		t.Fatalf("tracked work consumed inbox limit or vanished: %+v", got)
	}
	if got.TrackedResults[0].Verdict != "fail" || got.TrackedResults[0].Deadline == nil {
		t.Fatal("expired tracked tender lost its warning/date")
	}
	if got.TrackedResults[1].Band != "none" || !got.TrackedResults[1].Retained {
		t.Fatal("mismatched tracked tender lost")
	}
	if got.TrackedResults[2].RetentionReason != "source_unavailable" || got.TrackedResults[2].Title != "2026-199999" {
		t.Fatal("unavailable source fabricated content")
	}
	r.Org = "org-b"
	other, err := s.Matches(context.Background(), r)
	if err != nil || len(other.TrackedResults) != 0 {
		t.Fatalf("cross-org tracked work: %+v %v", other, err)
	}
	parsed, problems := DecodeMatchRequest([]byte(`{"tracked_only":true}`))
	if len(problems) > 0 {
		t.Fatal(problems)
	}
	parsed.Org = "org-a"
	noProfile, err := s.Matches(context.Background(), parsed)
	if err != nil || len(noProfile.TrackedResults) != 3 {
		t.Fatalf("tracked work requires matching profile: %+v %v", noProfile, err)
	}
	if noProfile.TrackedResults[0].Verdict != "unknown" {
		t.Fatal("missing profile was assessed as qualified")
	}
}

func TestProfessionalFocusChangesRankingWithinTheCompanyScope(t *testing.T) {
	docs := []rawstore.Document{
		doc("2026-100001", eformstest.ContractNotice(eformstest.Notice{Title: "General IT support", CPV: "72000000", Deadline: in(2)})),
		doc("2026-100002", eformstest.ContractNotice(eformstest.Notice{Title: "Security operations", CPV: "72000000", Deadline: in(20)})),
	}
	s := newService(t, &pagedNotices{fakeNotices{docs: docs}}, nil)
	r := Request{Profile: match.Profile{CPV: []string{"72000000"}}, Limit: 2}
	ordinary, err := s.Matches(context.Background(), r)
	if err != nil {
		t.Fatal(err)
	}
	if ordinary.Results[0].NoticeID != "2026-100001" {
		t.Fatal("baseline deadline order unexpected")
	}
	r.Profile.UserKeywords = []string{"security"}
	personal, err := s.Matches(context.Background(), r)
	if err != nil {
		t.Fatal(err)
	}
	if personal.Results[0].NoticeID != "2026-100002" {
		t.Fatal("professional context did not rank the relevant company tender first")
	}
}
