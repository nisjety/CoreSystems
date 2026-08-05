package content

import (
	"context"
	"errors"
	"regexp"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
)

// fakePageText records fetch calls and returns canned text/errors.
type fakePageText struct {
	text  string
	err   error
	calls int
}

func (f *fakePageText) FetchPageText(_ context.Context, _, _, _ string) (string, error) {
	f.calls++
	return f.text, f.err
}

func newPagesIngestor(f *fakePageText, s *fakeSink, class string) *PagesIngestor {
	return NewPagesIngestor(PagesConfig{Pages: f, Docs: s, Logger: zerolog.Nop(), ZDRClassification: class})
}

func pageFixture() (store.Source, store.Item, sharepoint.SitePage) {
	src := store.Source{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		Kind:           store.SourceKindSitePages,
		SiteID:         "contoso.sharepoint.com,11111111,22222222",
		TenantID:       "tenant-1",
		SiteWebURL:     "https://sp/sites/intranet",
	}
	item := store.Item{ItemID: "page-1", Name: "Home.aspx", Path: "/Home.aspx", WebURL: "https://sp/SitePages/Home.aspx"}
	page := sharepoint.SitePage{ID: "page-1", Name: "Home.aspx", Title: "Home", PageLayout: "home", WebURL: "https://sp/SitePages/Home.aspx"}
	return src, item, page
}

func TestPagesIngestor_HappyPathForwardsDocument(t *testing.T) {
	f := &fakePageText{text: "Welcome\n\nFirst & foremost."}
	s := &fakeSink{configured: true}
	ing := newPagesIngestor(f, s, "internal")

	src, item, page := pageFixture()
	if err := ing.IngestSitePage(context.Background(), src, item, page); err != nil {
		t.Fatalf("ingest: %v", err)
	}
	if len(s.created) != 1 {
		t.Fatalf("expected 1 forwarded doc, got %d", len(s.created))
	}
	doc := s.created[0]
	if doc.Source != "sharepoint" || doc.Type != "sharepoint_page" {
		t.Errorf("source/type = %q/%q, want sharepoint/sharepoint_page", doc.Source, doc.Type)
	}
	if doc.Title != "Home" {
		t.Errorf("title = %q, want the page title", doc.Title)
	}
	if doc.Content != "Welcome\n\nFirst & foremost." {
		t.Errorf("content = %q", doc.Content)
	}
	if doc.ZDRClassification != "internal" {
		t.Errorf("zdr = %q, want internal", doc.ZDRClassification)
	}
	if got := doc.Metadata["page_id"]; got != "page-1" {
		t.Errorf("metadata.page_id = %v, want page-1", got)
	}
	if s.orgs[0] != "org-1" {
		t.Errorf("org = %q, want org-1", s.orgs[0])
	}
}

// TestPagesIngestor_PrefersPageModifiedAtOverItem covers P2-3: the page's own
// lastModifiedDateTime must win when both it and the wrapping drive item's
// carry a value.
func TestPagesIngestor_PrefersPageModifiedAtOverItem(t *testing.T) {
	f := &fakePageText{text: "body"}
	s := &fakeSink{configured: true}
	ing := newPagesIngestor(f, s, "internal")

	src, item, page := pageFixture()
	pageModified := time.Date(2024, 3, 1, 12, 0, 0, 0, time.UTC)
	itemModified := time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC)
	page.LastModifiedDateTime = &pageModified
	item.ModifiedAt = &itemModified

	if err := ing.IngestSitePage(context.Background(), src, item, page); err != nil {
		t.Fatalf("ingest: %v", err)
	}
	if got := s.created[0].ModifiedAt; got == nil || !got.Equal(pageModified) {
		t.Errorf("ModifiedAt = %v, want the page's own %v", got, pageModified)
	}
}

// TestPagesIngestor_FallsBackToItemModifiedAt covers the other half: when the
// Pages API returns no lastModifiedDateTime for the page itself, the wrapping
// drive item's still reaches the forwarded document rather than being dropped.
func TestPagesIngestor_FallsBackToItemModifiedAt(t *testing.T) {
	f := &fakePageText{text: "body"}
	s := &fakeSink{configured: true}
	ing := newPagesIngestor(f, s, "internal")

	src, item, page := pageFixture()
	itemModified := time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC)
	item.ModifiedAt = &itemModified
	// page.LastModifiedDateTime deliberately left nil.

	if err := ing.IngestSitePage(context.Background(), src, item, page); err != nil {
		t.Fatalf("ingest: %v", err)
	}
	if got := s.created[0].ModifiedAt; got == nil || !got.Equal(itemModified) {
		t.Errorf("ModifiedAt = %v, want the fallback item time %v", got, itemModified)
	}
}

func TestPagesIngestor_IdempotencyKeyIsStableAndCharsetSafe(t *testing.T) {
	f := &fakePageText{text: "body"}
	s := &fakeSink{configured: true}
	ing := newPagesIngestor(f, s, "")

	src, item, page := pageFixture()
	_ = ing.IngestSitePage(context.Background(), src, item, page)
	_ = ing.IngestSitePage(context.Background(), src, item, page)

	if len(s.created) != 2 {
		t.Fatalf("expected 2 forwards, got %d", len(s.created))
	}
	key := s.created[0].IdempotencyKey
	if key != s.created[1].IdempotencyKey {
		t.Fatalf("idempotency key not stable across re-syncs: %q vs %q", key, s.created[1].IdempotencyKey)
	}
	// The raw site id contains commas — the documents-api charset would 400 on
	// it, so the key must be hash-derived.
	if !regexp.MustCompile(`^[A-Za-z0-9._:-]+$`).MatchString(key) {
		t.Fatalf("idempotency key %q violates the documents-api charset", key)
	}
}

func TestPagesIngestor_EmptyTextSkipsCleanly(t *testing.T) {
	f := &fakePageText{text: "   \n "}
	s := &fakeSink{configured: true}
	ing := newPagesIngestor(f, s, "")

	src, item, page := pageFixture()
	if err := ing.IngestSitePage(context.Background(), src, item, page); err != nil {
		t.Fatalf("ingest: %v", err)
	}
	if len(s.created) != 0 {
		t.Fatalf("expected no forwards for empty canvas text, got %d", len(s.created))
	}
}

func TestPagesIngestor_FetchErrorSurfaces(t *testing.T) {
	f := &fakePageText{err: errors.New("graph 500")}
	s := &fakeSink{configured: true}
	ing := newPagesIngestor(f, s, "")

	src, item, page := pageFixture()
	if err := ing.IngestSitePage(context.Background(), src, item, page); err == nil {
		t.Fatal("expected fetch error to surface")
	}
	if len(s.created) != 0 {
		t.Fatalf("expected no forwards after a fetch error, got %d", len(s.created))
	}
}

func TestPagesIngestor_UnconfiguredSinkIsNoOp(t *testing.T) {
	f := &fakePageText{text: "body"}
	s := &fakeSink{configured: false}
	ing := newPagesIngestor(f, s, "")

	src, item, page := pageFixture()
	if err := ing.IngestSitePage(context.Background(), src, item, page); err != nil {
		t.Fatalf("ingest: %v", err)
	}
	if f.calls != 0 {
		t.Fatalf("expected no fetch when the sink is unconfigured, got %d calls", f.calls)
	}
}
