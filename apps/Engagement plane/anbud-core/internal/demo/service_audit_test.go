package demo

import (
	"context"
	"encoding/json"
	"errors"
	"maps"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/rawstore"
)

func auditNotice(extra eformstest.Notice) rawstore.Document {
	extra.Deadline = in(20)
	return doc("2026-100001", eformstest.ContractNotice(extra))
}

func TestAuditShowsEverythingBehindOneAssessment(t *testing.T) {
	t.Parallel()

	d := auditNotice(eformstest.Notice{
		Title: "Drift av fagsystem", CPV: "72212000-7", NUTS: "NO081", ValueNOK: 1_500_000,
		Criteria:         []string{"Leverandøren skal ha ISO 9001 eller tilsvarende.", "Tre referanser fra tilsvarende oppdrag."},
		PointToDocuments: true,
	})
	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{d}}, nil).Audit(context.Background(), "2026-100001", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}

	r := got.Result
	if r.NoticeID != "2026-100001" || r.Title != "Drift av fagsystem" || r.NoticeType != "cn-standard" || r.Verdict != "conditional" || r.Band != "strong" {
		t.Errorf("result = id %q title %q type %q verdict %q band %q", r.NoticeID, r.Title, r.NoticeType, r.Verdict, r.Band)
	}
	if r.EstimatedValue == nil || r.EstimatedValue.Currency != "NOK" || r.EstimatedValue.Amount != "1500000.00" {
		t.Errorf("value = %+v", r.EstimatedValue)
	}
	if r.Deadline == nil || r.Deadline.DaysLeft != 20 || r.Deadline.Kind != "tender" || r.Deadline.Lot != "LOT-0001" || r.Deadline.OsloDay != "2026-11-30" {
		t.Errorf("deadline = %+v", r.Deadline)
	}

	// The audit sheet is for a person to check the work: the evidence, what was read, what was changed.
	if got.Evidence.Source != "doffin" || got.Evidence.NoticeID != "2026-100001" || got.Evidence.SHA256 != d.SHA256 ||
		!got.Evidence.FetchedAt.Equal(fetched) || got.Evidence.Bytes != len(d.Body) {
		t.Errorf("evidence = %+v", got.Evidence)
	}
	n := got.Notice
	if len(n.Lots) != 1 || n.Lots[0].ID != "LOT-0001" || n.Lots[0].TenderDeadline == nil {
		t.Errorf("lots = %+v", n.Lots)
	}
	if len(n.SelectionCriteria) != 2 || n.SelectionCriteria[0].Description != "Leverandøren skal ha ISO 9001 eller tilsvarende." || n.SelectionCriteria[0].Lot != "LOT-0001" {
		t.Errorf("criteria = %+v", n.SelectionCriteria)
	}
	if n.ExclusionGrounds != 3 || !n.CriteriaInDocuments || len(n.Documents) != 1 || n.Documents[0].URL != "https://example.invalid/docs/1" {
		t.Errorf("exclusion grounds %d, criteria in documents %v, documents %+v", n.ExclusionGrounds, n.CriteriaInDocuments, n.Documents)
	}
	if len(got.Normalisation.Changes) != 1 || got.Normalisation.Changes[0].Field != "cpv" || got.Normalisation.Changes[0].From != "72212000-7" || got.Normalisation.Changes[0].To != "72212000" {
		t.Errorf("changes = %+v", got.Normalisation.Changes)
	}
	if got.Parse.Root != "ContractNotice" || got.Parse.SDKVersion != "1.14" || got.Parse.Flavour != "eu" || got.Parse.ContactPoints != 1 {
		t.Errorf("parse = %+v", got.Parse)
	}
	if got.Parse.IssuesTotal != 0 || got.Parse.UnreadTotal == 0 || len(got.Parse.Unread) == 0 || got.Parse.Unread[0] != "ContractNotice/RegulatoryDomain" {
		t.Errorf("issues %d unread %d %v: the unread elements must be listed so a person can see what was not read", got.Parse.IssuesTotal, got.Parse.UnreadTotal, got.Parse.Unread)
	}

	var rules []string
	for _, c := range r.Gate {
		rules = append(rules, c.Rule+"="+c.Outcome)
	}
	if want := map[string]int{"pass": 2, "conditional": 1, "unknown": 2, "fail": 0}; !maps.Equal(r.GateCounts, want) {
		t.Errorf("gate counts = %v, want %v", r.GateCounts, want)
	}
	wantRules := []string{"deadline=pass", "exclusion_grounds=pass", "criteria_in_documents=unknown", "requirement=conditional", "requirement=unknown"}
	if strings.Join(rules, " ") != strings.Join(wantRules, " ") {
		t.Errorf("gate = %v, want %v", rules, wantRules)
	}
}

func TestAuditDoesNotCarryPersonalData(t *testing.T) {
	t.Parallel()

	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{auditNotice(eformstest.Notice{})}}, nil).Audit(context.Background(), "2026-100001", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	out, _ := json.Marshal(got)
	for _, marker := range eformstest.PersonalMarkers {
		if strings.Contains(string(out), marker) {
			t.Errorf("the audit sheet carries %q", marker)
		}
	}
	if got.Parse.ContactPoints != 1 {
		t.Errorf("contact points = %d: they are counted, never shown", got.Parse.ContactPoints)
	}
}

func TestAuditClipsFreeTextAndCapsLists(t *testing.T) {
	t.Parallel()

	long := strings.Repeat("æ", 5000)
	criteria := make([]string, maxListed+20)
	for i := range criteria {
		criteria[i] = "Krav nummer " + long[:4]
	}
	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{auditNotice(eformstest.Notice{Description: long, Criteria: criteria})}}, nil).
		Audit(context.Background(), "2026-100001", mustDecode(t, itProfile))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if n := utf8.RuneCountInString(got.Notice.Description); n != maxTextRunes+1 || !strings.HasSuffix(got.Notice.Description, "…") {
		t.Errorf("description has %d runes, want %d and an ellipsis", n, maxTextRunes+1)
	}
	if len(got.Notice.SelectionCriteria) != maxListed || got.Notice.SelectionCriteriaTotal != maxListed+20 {
		t.Errorf("criteria listed %d of %d, want %d of %d", len(got.Notice.SelectionCriteria), got.Notice.SelectionCriteriaTotal, maxListed, maxListed+20)
	}
}

func TestAuditRefusesWhatItCannotAssess(t *testing.T) {
	t.Parallel()

	docs := []rawstore.Document{
		doc("2026-100001", eformstest.AwardNotice(eformstest.Notice{})),
		doc("2026-100002", []byte("this is not a notice")),
		doc("2026-100003", eformstest.ContractNotice(eformstest.Notice{SDK: "1.7"})),
	}
	store := &fakeNotices{docs: docs}
	s := newService(t, store, nil)
	r := mustDecode(t, itProfile)

	if _, err := s.Audit(context.Background(), "2026-100001", r); !errors.Is(err, ErrNotAnOpportunity) {
		t.Errorf("a result notice: err = %v, want ErrNotAnOpportunity", err)
	}
	if _, err := s.Audit(context.Background(), "2026-100002", r); !errors.Is(err, ErrUnreadable) {
		t.Errorf("a body that is not a notice: err = %v, want ErrUnreadable", err)
	}
	_, err := s.Audit(context.Background(), "2026-100003", r)
	var unsupported *UnsupportedError
	if !errors.As(err, &unsupported) || unsupported.Root != "ContractNotice" || unsupported.SDKVersion != "1.7" || unsupported.Flavour != "eu" {
		t.Errorf("an SDK with no field map: err = %v, want an UnsupportedError naming the shape", err)
	}
	if _, err := s.Audit(context.Background(), "2026-999999", r); !errors.Is(err, ErrNotFound) {
		t.Errorf("a notice that is not held: err = %v, want ErrNotFound", err)
	}
}

func TestAuditNeverAsksTheStoreForAnIdThatIsNotANoticeID(t *testing.T) {
	t.Parallel()

	store := &fakeNotices{}
	for _, id := range []string{"", "x", "2026-1", "2026-104837'; DROP", "../../etc/passwd"} {
		if _, err := newService(t, store, nil).Audit(context.Background(), id, mustDecode(t, itProfile)); !errors.Is(err, ErrInvalidNoticeID) {
			t.Errorf("id %q: err = %v, want ErrInvalidNoticeID", id, err)
		}
	}
	if len(store.fetched) != 0 {
		t.Errorf("the store was asked for %v", store.fetched)
	}
}

func TestAuditReturnsAStoreErrorAsIt(t *testing.T) {
	t.Parallel()

	boom := errors.New("database is down")
	if _, err := newService(t, &fakeNotices{err: boom}, nil).Audit(context.Background(), "2026-100001", mustDecode(t, itProfile)); !errors.Is(err, boom) {
		t.Errorf("err = %v, want the store's error", err)
	}
}

func TestAuditAppliesTheCompanyCheckToo(t *testing.T) {
	t.Parallel()

	companies := &fakeCompanies{org: orgcore.Organisation{OrgNumber: "974760673", Bankrupt: true}}
	got, err := newService(t, &fakeNotices{docs: []rawstore.Document{auditNotice(eformstest.Notice{})}}, companies).
		Audit(context.Background(), "2026-100001", mustDecode(t, `{"profile":{"org_number":"974760673","cpv":["72000000"]}}`))
	if err != nil {
		t.Fatalf("Audit: %v", err)
	}
	if got.Company == nil || !got.Company.Checked || got.Result.Verdict != "fail" {
		t.Errorf("company %+v verdict %q", got.Company, got.Result.Verdict)
	}
}
