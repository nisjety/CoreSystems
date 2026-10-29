package sync

import (
	"context"
	"errors"
	"io"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
)

type fakeSitePagesLister struct {
	pages        []sharepoint.SitePage
	err          error
	receivedOrg  string
	receivedSite string
}

func (f *fakeSitePagesLister) ListSitePages(_ context.Context, organizationID, siteID string) ([]sharepoint.SitePage, error) {
	f.receivedOrg = organizationID
	f.receivedSite = siteID
	if f.err != nil {
		return nil, f.err
	}
	return f.pages, nil
}

type recordingPageSink struct {
	ingested []sharepoint.SitePage
	err      error
}

func (s *recordingPageSink) IngestSitePage(_ context.Context, _ store.Source, _ store.Item, page sharepoint.SitePage) error {
	s.ingested = append(s.ingested, page)
	return s.err
}

func newSitePagesEngine(src store.Source, lister SitePagesLister, sink PageContentSink, items *fakeItemStore, cursors *fakeCursorStore, pub *recordingPublisher) *Engine {
	return NewEngine(Config{
		Sources:     &fakeSourceStore{src: src},
		Items:       items,
		Cursors:     cursors,
		Publisher:   pub,
		SitePages:   lister,
		PageContent: sink,
		Subjects:    events.NewSubjects("finspo-test"),
		Logger:      zerolog.New(io.Discard),
	})
}

func sitePagesSource() store.Source {
	return store.Source{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		Kind:           store.SourceKindSitePages,
		SiteID:         "site-1",
	}
}

func TestSyncDriveDispatchesSitePagesKind(t *testing.T) {
	t.Parallel()

	modified := time.Date(2026, time.June, 1, 9, 0, 0, 0, time.UTC)
	src := sitePagesSource()
	lister := &fakeSitePagesLister{pages: []sharepoint.SitePage{
		{ID: "page-1", Name: "Home.aspx", Title: "Home", WebURL: "https://sp/SitePages/Home.aspx", LastModifiedDateTime: &modified},
		{ID: "page-2", Name: "News.aspx", Title: "News"},
		{ID: "", Name: "ghost.aspx"}, // no id → skipped
	}}
	sink := &recordingPageSink{}
	items := &fakeItemStore{}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	pub := &recordingPublisher{}
	engine := newSitePagesEngine(src, lister, sink, items, cursors, pub)

	res, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive(): %v", err)
	}

	if lister.receivedOrg != "org-1" || lister.receivedSite != "site-1" {
		t.Fatalf("lister received (%q, %q), want (org-1, site-1)", lister.receivedOrg, lister.receivedSite)
	}
	if res.ItemsUpserted != 2 {
		t.Fatalf("ItemsUpserted = %d, want 2", res.ItemsUpserted)
	}
	if len(items.upserts) != 2 {
		t.Fatalf("len(upserts) = %d, want 2", len(items.upserts))
	}
	if got := items.upserts[0]; got.ID != "page-1" || got.MimeType() != "text/html" || got.IsFolder() {
		t.Fatalf("upsert[0] = %+v, want page-1 as a text/html non-folder item", got)
	}
	if len(sink.ingested) != 2 {
		t.Fatalf("pages forwarded to content sink = %d, want 2", len(sink.ingested))
	}

	if len(cursors.saved) != 1 {
		t.Fatalf("cursor saves = %d, want 1", len(cursors.saved))
	}
	saved := cursors.saved[0]
	if saved.status != "ok" || saved.itemsDelta != 2 || saved.deltaLink != "" {
		t.Fatalf("cursor = %+v, want ok/2/empty delta link", saved)
	}

	var itemEvents, syncedEvents int
	for _, e := range pub.emitted {
		switch e.Subject {
		case engine.subjects.ItemUpserted():
			itemEvents++
		case engine.subjects.SourceSynced():
			syncedEvents++
		}
	}
	if itemEvents != 2 || syncedEvents != 1 {
		t.Fatalf("events = %d item.upserted / %d source.synced, want 2 / 1", itemEvents, syncedEvents)
	}
}

func TestSyncSitePagesContentFailureDoesNotAbort(t *testing.T) {
	t.Parallel()

	src := sitePagesSource()
	lister := &fakeSitePagesLister{pages: []sharepoint.SitePage{
		{ID: "page-1", Name: "Home.aspx"},
		{ID: "page-2", Name: "News.aspx"},
	}}
	sink := &recordingPageSink{err: errors.New("canvas fetch 500")}
	items := &fakeItemStore{}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	engine := newSitePagesEngine(src, lister, sink, items, cursors, &recordingPublisher{})

	res, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive(): %v (content ingest must be best-effort)", err)
	}
	if res.ItemsUpserted != 2 {
		t.Fatalf("ItemsUpserted = %d, want 2 despite ingest failures", res.ItemsUpserted)
	}
}

func TestSyncSitePagesListFailureRecordsCursorError(t *testing.T) {
	t.Parallel()

	src := sitePagesSource()
	lister := &fakeSitePagesLister{err: errors.New("graph 403")}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	engine := newSitePagesEngine(src, lister, nil, &fakeItemStore{}, cursors, &recordingPublisher{})

	if _, err := engine.SyncDrive(context.Background(), src.ID); err == nil {
		t.Fatal("expected list failure to surface")
	}
	if len(cursors.saved) != 1 || cursors.saved[0].status != "error" {
		t.Fatalf("cursor saves = %+v, want one error-status save", cursors.saved)
	}
}

func TestSyncSitePagesWithoutListerFailsClosed(t *testing.T) {
	t.Parallel()

	src := sitePagesSource()
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	engine := newSitePagesEngine(src, nil, nil, &fakeItemStore{}, cursors, &recordingPublisher{})

	if _, err := engine.SyncDrive(context.Background(), src.ID); err == nil {
		t.Fatal("expected unconfigured site pages sync to error")
	}
	if len(cursors.saved) != 1 || cursors.saved[0].status != "error" {
		t.Fatalf("cursor saves = %+v, want one error-status save", cursors.saved)
	}
}
