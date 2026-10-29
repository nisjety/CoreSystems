package demo

import (
	"context"
	"fmt"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/rawstore"
	"strings"
	"testing"
)

type pagedNotices struct{ fakeNotices }

func (f *pagedNotices) CataloguePage(_ context.Context, before int64, limit int) ([]rawstore.CatalogueDocument, error) {
	out := []rawstore.CatalogueDocument{}
	for i, d := range f.docs {
		seq := int64(len(f.docs) - i)
		if before != 0 && seq >= before {
			continue
		}
		out = append(out, rawstore.CatalogueDocument{Sequence: seq, Document: d})
		if len(out) == limit {
			break
		}
	}
	return out, nil
}
func TestCatalogueNeedsNoProfileAndIncludesExpiredAndAwards(t *testing.T) {
	s := newService(t, &pagedNotices{fakeNotices{docs: corpus()}}, nil)
	got, err := s.Catalogue(context.Background(), CatalogueRequest{})
	if err != nil {
		t.Fatal(err)
	}
	if len(got.Results) != 6 {
		t.Fatalf("results=%d, want all 6 stored notices", len(got.Results))
	}
	for _, n := range got.Results {
		if n.Verdict != "unknown" {
			t.Fatalf("catalogue fabricated an assessment: %s", n.Verdict)
		}
	}
	expired, err := s.Catalogue(context.Background(), CatalogueRequest{Status: "expired"})
	if err != nil || len(expired.Results) != 1 || expired.Results[0].Title != "Stengt drift" {
		t.Fatalf("expired=%+v err=%v", expired, err)
	}
	filtered, err := s.Catalogue(context.Background(), CatalogueRequest{Query: "Jordbruk", CPV: "03", Region: "NO08"})
	if err != nil || len(filtered.Results) != 1 {
		t.Fatalf("filtered=%+v err=%v", filtered, err)
	}
}
func TestCatalogueCursorTraversesBeyondMatchingScanLimit(t *testing.T) {
	docs := make([]rawstore.Document, 1100)
	for i := range docs {
		docs[i] = doc(fmt.Sprintf("2026-%06d", 100000+i), eformstest.ContractNotice(eformstest.Notice{Title: "Tjenester", CPV: "72000000"}))
	}
	s := newService(t, &pagedNotices{fakeNotices{docs: docs}}, nil)
	seen := map[string]bool{}
	cursor := ""
	for page := 0; page < 23; page++ {
		got, err := s.Catalogue(context.Background(), CatalogueRequest{Cursor: cursor})
		if err != nil {
			t.Fatal(err)
		}
		for _, n := range got.Results {
			if seen[n.NoticeID] {
				t.Fatal("duplicate across pages")
			}
			seen[n.NoticeID] = true
		}
		cursor = got.NextCursor
		if cursor == "" {
			break
		}
	}
	if len(seen) != 1100 || cursor != "" {
		t.Fatalf("seen=%d cursor=%q", len(seen), cursor)
	}
	if _, err := s.Catalogue(context.Background(), CatalogueRequest{Cursor: "-1"}); err != ErrCatalogueRequest {
		t.Fatalf("invalid cursor accepted: %v", err)
	}
}

func TestCatalogueDetailReturnsSourceDescriptionDocumentsAndProvenanceWithoutProfile(t *testing.T) {
	raw := eformstest.ContractNotice(eformstest.Notice{Title: "Services", CPV: "72000000", Deadline: in(20)})
	s := newService(t, &fakeNotices{docs: []rawstore.Document{doc("2026-100001", raw)}}, nil)
	detail, err := s.CatalogueDetail(context.Background(), "2026-100001")
	if err != nil {
		t.Fatal(err)
	}
	if detail.Result.Title != "Services" || detail.Result.Verdict != "unknown" || detail.Evidence.NoticeID != "2026-100001" || detail.Evidence.Bytes != len(raw) {
		t.Fatalf("detail=%+v", detail)
	}
	if detail.Notice.Description == "" {
		t.Fatal("source description missing")
	}
	if _, err := s.CatalogueDetail(context.Background(), "2026-999999"); err != ErrNotFound {
		t.Fatalf("missing notice: %v", err)
	}
	if _, err := s.CatalogueDetail(context.Background(), "../source"); err != ErrInvalidNoticeID {
		t.Fatalf("invalid ID: %v", err)
	}
}

func TestCatalogueArrangementComesFromCodesNotTitle(t *testing.T) {
	raw := eformstest.ContractNotice(eformstest.Notice{Title: "Rammeavtale uten kodet ordning", CPV: "72000000"})
	plain, _ := catalogueNotice(doc("2026-100001", raw), now)
	if len(plain.ArrangementTypes) != 0 {
		t.Fatal("arrangement was guessed from title")
	}
	coded := strings.Replace(string(raw), "<cac:TenderingProcess>", `<cac:TenderingProcess><cac:ContractingSystem><cbc:ContractingSystemTypeCode listName="framework-agreement">fa-mix</cbc:ContractingSystemTypeCode></cac:ContractingSystem>`, 1)
	result, _ := catalogueNotice(doc("2026-100001", []byte(coded)), now)
	if len(result.ArrangementTypes) != 1 || result.ArrangementTypes[0] != "framework" {
		t.Fatalf("arrangements=%v", result.ArrangementTypes)
	}
}

type catalogueEvidence struct {
	pagedNotices
	dates map[string]string
}

func (f *catalogueEvidence) PublicationDates(context.Context) (map[string]string, error) {
	return f.dates, nil
}

func TestCatalogueSortAndFacetCountsCoverEveryPage(t *testing.T) {
	docs := make([]rawstore.Document, 120)
	for i := range docs {
		deadline := in(5)
		if i%3 == 0 {
			deadline = in(-2)
		}
		docs[i] = doc(fmt.Sprintf("2026-%06d", 100000+i), eformstest.ContractNotice(eformstest.Notice{Title: fmt.Sprintf("Tender %03d", 119-i), Buyer: fmt.Sprintf("Buyer %d", i%2), CPV: "72000000", Deadline: deadline}))
	}
	source := &catalogueEvidence{pagedNotices: pagedNotices{fakeNotices{docs: docs}}, dates: map[string]string{"2026-100119": "2026-10-28"}}
	s := newService(t, source, nil)
	got, err := s.Catalogue(context.Background(), CatalogueRequest{Sort: "title", Direction: "asc"})
	if err != nil {
		t.Fatal(err)
	}
	if got.Results[0].Title != "Tender 000" || got.Results[49].Title != "Tender 049" || got.TotalMatched != 120 || got.Facets["status"]["active"] != 80 || got.Facets["status"]["expired"] != 40 {
		t.Fatalf("global sort/count wrong: first=%s matched=%d facets=%v", got.Results[0].Title, got.TotalMatched, got.Facets)
	}
	next, err := s.Catalogue(context.Background(), CatalogueRequest{Sort: "title", Direction: "asc", Cursor: got.NextCursor})
	if err != nil || next.Results[0].Title != "Tender 050" {
		t.Fatalf("next page wrong: %v", err)
	}
	desc, err := s.Catalogue(context.Background(), CatalogueRequest{Sort: "title", Direction: "desc"})
	if err != nil || desc.Results[0].Title != "Tender 119" {
		t.Fatalf("descending sort wrong: %v", err)
	}
	filtered, err := s.Catalogue(context.Background(), CatalogueRequest{Status: "active"})
	if err != nil || filtered.TotalMatched != 80 || filtered.Facets["status"]["expired"] != 40 {
		t.Fatalf("self-excluding facet wrong: %+v %v", filtered.Facets, err)
	}
	if got.Results[0].PublishedDate != "2026-10-28" {
		t.Fatal("publisher date missing")
	}
}
func TestCatalogueContractTermsAndWinnersKeepVerificationPolicy(t *testing.T) {
	raw := eformstest.AwardWithResult(eformstest.Notice{Title: "Award", CPV: "72000000"}, eformstest.Award{WinnerName: "Verified Supplier", WinnerOrg: "974760673", AwardDate: now, DurationMonths: 48})
	for _, form := range []string{"AS", "ENK"} {
		source := &pagedNotices{fakeNotices{docs: []rawstore.Document{doc("2026-100001", raw)}}}
		s := newService(t, source, &fakeCompanies{org: orgcore.Organisation{OrgNumber: "974760673", OrgForm: form}})
		got, err := s.CatalogueDetail(context.Background(), "2026-100001")
		if err != nil {
			t.Fatal(err)
		}
		if len(got.Result.Contracts) != 1 || got.Result.Contracts[0].Duration.Value != 48 || got.Result.Contracts[0].EndDate != "2030-11-10" {
			t.Fatalf("contract terms=%+v", got.Result.Contracts)
		}
		for _, winner := range got.Result.Winners {
			if form == "ENK" && winner.Name != "" {
				t.Fatal("sole trader named")
			}
		}
		if form == "AS" && (len(got.Result.Winners) != 1 || got.Result.Winners[0].Name != "Verified Supplier") {
			t.Fatalf("verified winner missing: %+v", got.Result.Winners)
		}
	}
}

func TestCatalogueSortUsesNordicAlphabetOrder(t *testing.T) {
	rows := []CatalogueNotice{{Result: Result{NoticeID: "1", Buyer: "Ålesund"}}, {Result: Result{NoticeID: "2", Buyer: "Arbeid"}}, {Result: Result{NoticeID: "3", Buyer: "Øst"}}, {Result: Result{NoticeID: "4", Buyer: "Æra"}}, {Result: Result{NoticeID: "5", Buyer: "Zebra"}}}
	sortCatalogue(rows, "buyer", "asc")
	for i, want := range []string{"Arbeid", "Zebra", "Æra", "Øst", "Ålesund"} {
		if rows[i].Buyer != want {
			t.Fatalf("row %d=%s, want %s", i, rows[i].Buyer, want)
		}
	}
}
