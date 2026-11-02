package demo

import (
	"context"
	"errors"
	"fmt"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/rawstore"
	"reflect"
	"testing"
	"time"
)

type paginatedMatchStore struct {
	pagedNotices
	revision int64
	reads    int
}

func (s *paginatedMatchStore) CatalogueGeneration(context.Context) (int64, error) {
	return s.revision, nil
}
func (s *paginatedMatchStore) CataloguePage(ctx context.Context, before int64, limit int) ([]rawstore.CatalogueDocument, error) {
	s.reads++
	return s.pagedNotices.CataloguePage(ctx, before, limit)
}
func pagingFixture(t *testing.T) (*Service, *paginatedMatchStore, Request) {
	t.Helper()
	docs := []rawstore.Document{}
	for i := 0; i < 130; i++ {
		title := fmt.Sprintf("%03d services", 129-i)
		if i == 129 {
			title += " Needle"
		}
		docs = append(docs, doc(fmt.Sprintf("2026-%06d", 100000+i), eformstest.ContractNotice(eformstest.Notice{Title: title, CPV: "72000000", Deadline: in(2)})))
	}
	store := &paginatedMatchStore{pagedNotices: pagedNotices{fakeNotices{docs: docs}}, revision: 1}
	return newService(t, store, nil), store, Request{Org: "org-a", User: "user-a", Profile: match.Profile{CPV: []string{"72000000"}}, Paginate: true, Limit: 50}
}
func TestMatchingPagesReachEveryResultWithoutAnotherMatchingScan(t *testing.T) {
	s, source, r := pagingFixture(t)
	first, err := s.Matches(context.Background(), r)
	if err != nil {
		t.Fatal(err)
	}
	if first.Page != 1 || first.PageSize != 50 || first.TotalMatched != 130 || len(first.Results) != 50 || first.NextCursor == "" || first.PreviousCursor != "" {
		t.Fatalf("first page: %+v", first)
	}
	scanned := source.reads
	seen := map[string]bool{}
	page := first
	for {
		for _, row := range page.Results {
			if seen[row.NoticeID] {
				t.Fatal("duplicate across pages")
			}
			seen[row.NoticeID] = true
		}
		if page.NextCursor == "" {
			break
		}
		r.Cursor = page.NextCursor
		page, err = s.Matches(context.Background(), r)
		if err != nil {
			t.Fatal(err)
		}
	}
	if len(seen) != 130 || page.Page != 3 || len(page.Results) != 30 || source.reads != scanned {
		t.Fatalf("seen=%d page=%d scans=%d/%d", len(seen), page.Page, source.reads, scanned)
	}
	r.Cursor = page.PreviousCursor
	previous, err := s.Matches(context.Background(), r)
	if err != nil || previous.Page != 2 {
		t.Fatalf("previous: %+v %v", previous, err)
	}
	r.Cursor = previous.PreviousCursor
	again, err := s.Matches(context.Background(), r)
	if err != nil || !reflect.DeepEqual(again.Results, first.Results) {
		t.Fatal("return to first page changed results")
	}
}
func TestMatchingPaginationIsBoundToCompanyUserAndSearch(t *testing.T) {
	s, _, r := pagingFixture(t)
	first, err := s.Matches(context.Background(), r)
	if err != nil {
		t.Fatal(err)
	}
	r.Cursor = first.NextCursor
	for _, change := range []func(*Request){func(r *Request) { r.Org = "org-b" }, func(r *Request) { r.User = "user-b" }, func(r *Request) { r.Profile.CPV = []string{"03000000"} }, func(r *Request) { r.Filters.Query = "different" }, func(r *Request) { r.Sort = "title" }, func(r *Request) { r.Limit = 20 }} {
		changed := r
		change(&changed)
		got, err := s.Matches(context.Background(), changed)
		if !errors.Is(err, ErrMatchCursor) || len(got.Results) != 0 {
			t.Fatalf("cursor escaped request scope: %v", err)
		}
	}
}
func TestMatchingFiltersAndSortApplyBeforePagination(t *testing.T) {
	s, _, r := pagingFixture(t)
	r.Filters.Query = "Needle"
	got, err := s.Matches(context.Background(), r)
	if err != nil || got.TotalMatched != 1 || got.Results[0].NoticeID != "2026-100129" {
		t.Fatalf("global search missed later page: %+v %v", got, err)
	}
	r.Filters = MatchFilters{}
	r.Sort = "title"
	got, err = s.Matches(context.Background(), r)
	if err != nil || got.Results[0].NoticeID != "2026-100129" {
		t.Fatalf("global title sort: %+v %v", got, err)
	}
	r.Filters.Region = "NO09"
	got, err = s.Matches(context.Background(), r)
	if err != nil || got.TotalMatched != 0 || len(got.Results) != 0 || got.NextCursor != "" {
		t.Fatalf("empty global filter: %+v %v", got, err)
	}
}
func TestTrackedWorkDoesNotConsumeInboxPagesAndMembershipChangesInvalidate(t *testing.T) {
	s, _, r := pagingFixture(t)
	tracked := trackedByOrg{"org-a": {"2026-100000"}}
	s.tracking = tracked
	first, err := s.Matches(context.Background(), r)
	if err != nil {
		t.Fatal(err)
	}
	if first.TotalMatched != 129 || len(first.Results) != 50 || len(first.TrackedResults) != 1 || first.Results[0].NoticeID == "2026-100000" {
		t.Fatal("tracked work consumed inbox slots")
	}
	r.Cursor = first.NextCursor
	next, err := s.Matches(context.Background(), r)
	if err != nil || len(next.TrackedResults) != 1 {
		t.Fatal("tracked work disappeared on page two")
	}
	tracked["org-a"] = append(tracked["org-a"], "2026-100050")
	if _, err = s.Matches(context.Background(), r); !errors.Is(err, ErrMatchSnapshotExpired) {
		t.Fatalf("pipeline membership drift did not reset paging: %v", err)
	}
}
func TestMatchingSnapshotsInvalidateOnSourceEvidenceOrClockChanges(t *testing.T) {
	t.Run("source redaction or import", func(t *testing.T) {
		s, source, r := pagingFixture(t)
		first, err := s.Matches(context.Background(), r)
		if err != nil {
			t.Fatal(err)
		}
		r.Cursor = first.NextCursor
		source.revision++
		if _, err := s.Matches(context.Background(), r); !errors.Is(err, ErrMatchSnapshotExpired) {
			t.Fatal(err)
		}
	})
	t.Run("uploaded document erasure", func(t *testing.T) {
		s, _, r := pagingFixture(t)
		e := &fakeEvidence{byNotice: map[string]match.Evidence{"2026-100001": {Documents: 1}}}
		s.evidence = e
		first, err := s.Matches(context.Background(), r)
		if err != nil {
			t.Fatal(err)
		}
		r.Cursor = first.NextCursor
		delete(e.byNotice, "2026-100001")
		if _, err := s.Matches(context.Background(), r); !errors.Is(err, ErrMatchSnapshotExpired) {
			t.Fatal(err)
		}
	})
	t.Run("expiry", func(t *testing.T) {
		s, _, r := pagingFixture(t)
		first, err := s.Matches(context.Background(), r)
		if err != nil {
			t.Fatal(err)
		}
		r.Cursor = first.NextCursor
		s.now = func() time.Time { return now.Add(11 * time.Minute) }
		if _, err := s.Matches(context.Background(), r); !errors.Is(err, ErrMatchSnapshotExpired) {
			t.Fatal(err)
		}
	})
}
func TestMatchingSnapshotCacheIsBounded(t *testing.T) {
	s, _, r := pagingFixture(t)
	for i := 0; i < maxMatchSnapshots+3; i++ {
		r.Filters.Query = fmt.Sprint(i)
		if _, err := s.Matches(context.Background(), r); err != nil {
			t.Fatal(err)
		}
	}
	if len(s.matchPages) > maxMatchSnapshots {
		t.Fatal("unbounded match snapshots")
	}
}
