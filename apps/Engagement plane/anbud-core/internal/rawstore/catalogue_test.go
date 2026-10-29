package rawstore

import (
	"errors"
	"github.com/triodelab/anbud/internal/doffinsync"
	"testing"
)

func TestCataloguePageTraversesLatestBodiesWithoutDuplicates(t *testing.T) {
	s, _ := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	putNotice(t, s, run, "2026-100001", "old body")
	putNotice(t, s, run, "2026-100002", "second notice")
	putNotice(t, s, run, "2026-100001", "latest body")
	first, err := s.CataloguePage(testCtx(t), 0, 1)
	if err != nil || len(first) != 1 || string(first[0].Document.Body) != "latest body" {
		t.Fatalf("first=%+v err=%v", first, err)
	}
	second, err := s.CataloguePage(testCtx(t), first[0].Sequence, 1)
	if err != nil || len(second) != 1 || second[0].Document.ID != "2026-100002" {
		t.Fatalf("second=%+v err=%v", second, err)
	}
	last, err := s.CataloguePage(testCtx(t), second[0].Sequence, 1)
	if err != nil || len(last) != 0 {
		t.Fatalf("old revision resurfaced: %+v %v", last, err)
	}
	total, err := s.CatalogueTotal(testCtx(t))
	if err != nil || total != 2 {
		t.Fatalf("total=%d err=%v", total, err)
	}
	if _, err := s.CataloguePage(testCtx(t), -1, 50); !errors.Is(err, ErrInvalidQuery) {
		t.Fatal("negative cursor accepted")
	}
}

func TestCataloguePublicationDateComesFromSearchEvidence(t *testing.T) {
	s, _ := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	body := []byte(`{"hits":[{"id":"2026-100001","publicationDate":"2026-10-02","issueDate":"2026-10-01T10:00:00Z"},{"id":"2026-100002","issueDate":"2026-10-01T10:00:00Z"}]}`)
	if err := s.PutSearchPage(testCtx(t), run, doffinsync.SearchPage{QueryKey: "publication-test", Page: 1, Body: body, ContentType: "application/json", FetchedAt: fixedNow}); err != nil {
		t.Fatal(err)
	}
	dates, err := s.PublicationDates(testCtx(t))
	if err != nil {
		t.Fatal(err)
	}
	if dates["2026-100001"] != "2026-10-02" || dates["2026-100002"] != "" {
		t.Fatalf("dates=%v; issue date must not be substituted", dates)
	}
}
