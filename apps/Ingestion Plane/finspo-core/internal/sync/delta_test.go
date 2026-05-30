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

// ---------------------------------------------------------------------------
// fakes
// ---------------------------------------------------------------------------

type fakeFetcher struct {
	pages map[string]sharepoint.DeltaPage
	calls []string
}

func (f *fakeFetcher) InitialDeltaURL(driveID string) string {
	return "https://graph/drives/" + driveID + "/root/delta"
}

func (f *fakeFetcher) Fetch(_ context.Context, _ string, url string) (sharepoint.DeltaPage, error) {
	f.calls = append(f.calls, url)
	page, ok := f.pages[url]
	if !ok {
		return sharepoint.DeltaPage{}, errors.New("unexpected url: " + url)
	}
	return page, nil
}

type fakeSourceStore struct{ src store.Source }

func (s *fakeSourceStore) Get(_ context.Context, id uuid.UUID) (store.Source, error) {
	if id != s.src.ID {
		return store.Source{}, store.ErrNotFound
	}
	return s.src, nil
}

type fakeItemStore struct {
	upserts       []sharepoint.DriveItem
	upsertResult  store.UpsertResult
	softDeletes   []string
	knownDeletes  map[string]store.Item
	softDeleteErr error
}

func (s *fakeItemStore) Upsert(_ context.Context, sourceID uuid.UUID, organizationID string, d sharepoint.DriveItem, _ []byte) (store.UpsertResult, error) {
	s.upserts = append(s.upserts, d)
	res := s.upsertResult
	res.Item.ID = uuid.New()
	res.Item.SourceID = sourceID
	res.Item.OrganizationID = organizationID
	res.Item.ItemID = d.ID
	res.Item.Name = d.Name
	res.Item.Path = d.FullPath()
	res.Item.QuickXorHash = d.QuickXorHash()
	res.Item.SHA1Hash = d.SHA1Hash()
	res.Inserted = true
	return res, nil
}

func (s *fakeItemStore) SoftDelete(_ context.Context, sourceID uuid.UUID, itemID string) (store.Item, error) {
	s.softDeletes = append(s.softDeletes, itemID)
	if s.softDeleteErr != nil {
		return store.Item{}, s.softDeleteErr
	}
	known, ok := s.knownDeletes[itemID]
	if !ok {
		return store.Item{}, store.ErrNotFound
	}
	known.SourceID = sourceID
	known.ItemID = itemID
	return known, nil
}

type fakeCursorStore struct {
	loaded  store.Cursor
	loadErr error
	saved   []savedCursor
}

type savedCursor struct {
	sourceID   uuid.UUID
	deltaLink  string
	status     string
	errMsg     string
	itemsDelta int64
}

func (s *fakeCursorStore) Get(_ context.Context, _ uuid.UUID) (store.Cursor, error) {
	if s.loadErr != nil {
		return store.Cursor{}, s.loadErr
	}
	return s.loaded, nil
}

func (s *fakeCursorStore) Save(_ context.Context, sourceID uuid.UUID, deltaLink, status, errMsg string, itemsDelta int64) (store.Cursor, error) {
	s.saved = append(s.saved, savedCursor{sourceID, deltaLink, status, errMsg, itemsDelta})
	return store.Cursor{SourceID: sourceID, DeltaLink: deltaLink, LastStatus: status}, nil
}

type recordingPublisher struct {
	emitted []emitted
}

type emitted struct {
	Subject string
	Payload any
}

func (p *recordingPublisher) Publish(subject string, payload any) error {
	p.emitted = append(p.emitted, emitted{subject, payload})
	return nil
}

type recordingSink struct {
	upserts           []store.Item
	permissionUpserts []permissionUpsert
	deletes           []store.Item
}

func (s *recordingSink) UpsertSourceObject(_ context.Context, _ store.Source, item store.Item) error {
	s.upserts = append(s.upserts, item)
	return nil
}

type permissionUpsert struct {
	item        store.Item
	permissions []store.Permission
}

func (s *recordingSink) UpsertSourceObjectWithPermissions(_ context.Context, _ store.Source, item store.Item, permissions []store.Permission) error {
	s.upserts = append(s.upserts, item)
	s.permissionUpserts = append(s.permissionUpserts, permissionUpsert{
		item:        item,
		permissions: append([]store.Permission(nil), permissions...),
	})
	return nil
}

func (s *recordingSink) DeleteSourceObject(_ context.Context, _ store.Source, item store.Item) error {
	s.deletes = append(s.deletes, item)
	return nil
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

func newTestEngine(fetcher DeltaFetcher, src store.Source, items *fakeItemStore, cursors *fakeCursorStore, pub *recordingPublisher) *Engine {
	return NewEngine(Config{
		Fetcher:   fetcher,
		Sources:   &fakeSourceStore{src: src},
		Items:     items,
		Cursors:   cursors,
		Publisher: pub,
		Subjects:  events.NewSubjects("finspo-test"),
		Logger:    zerolog.New(io.Discard),
	})
}

func newTestEngineWithSink(fetcher DeltaFetcher, src store.Source, items *fakeItemStore, cursors *fakeCursorStore, pub *recordingPublisher, sink SourceObjectSink) *Engine {
	return NewEngine(Config{
		Fetcher:   fetcher,
		Sources:   &fakeSourceStore{src: src},
		Items:     items,
		Cursors:   cursors,
		Publisher: pub,
		Sink:      sink,
		Subjects:  events.NewSubjects("finspo-test"),
		Logger:    zerolog.New(io.Discard),
	})
}

func driveFileItem(id, name, qxh, sha1 string) sharepoint.DriveItem {
	now := time.Date(2026, time.May, 26, 12, 0, 0, 0, time.UTC)
	return sharepoint.DriveItem{
		ID:                   id,
		Name:                 name,
		Size:                 1024,
		LastModifiedDateTime: &now,
		File:                 &sharepoint.FileFacet{MimeType: "application/pdf", Hashes: sharepoint.Hashes{QuickXorHash: qxh, SHA1Hash: sha1}},
		Parent:               &sharepoint.ParentReference{Path: "/drives/d1/root:/Reports"},
	}
}

func driveDeletedItem(id string) sharepoint.DriveItem {
	return sharepoint.DriveItem{
		ID:      id,
		Deleted: &sharepoint.DeletedFacet{State: "deleted"},
	}
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

func TestSyncDriveFollowsNextLinkThenStoresDeltaLink(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}

	initialURL := "https://graph/drives/drive-1/root/delta"
	nextURL := "https://graph/drives/drive-1/root/delta?$skiptoken=next"
	deltaLink := "https://graph/drives/drive-1/root/delta?token=final"

	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initialURL: {
			Items:    []sharepoint.DriveItem{driveFileItem("item-1", "report.docx", "QX==", "abc")},
			NextLink: nextURL,
		},
		nextURL: {
			Items:     []sharepoint.DriveItem{driveFileItem("item-2", "budget.xlsx", "", "")},
			DeltaLink: deltaLink,
		},
	}}

	items := &fakeItemStore{}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	pub := &recordingPublisher{}

	engine := newTestEngine(fetcher, src, items, cursors, pub)
	result, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}

	if result.Pages != 2 {
		t.Errorf("Pages = %d, want 2", result.Pages)
	}
	if result.ItemsUpserted != 2 {
		t.Errorf("ItemsUpserted = %d, want 2", result.ItemsUpserted)
	}
	if result.DeltaLink != deltaLink {
		t.Errorf("DeltaLink = %q, want %q", result.DeltaLink, deltaLink)
	}
	if len(fetcher.calls) != 2 || fetcher.calls[0] != initialURL || fetcher.calls[1] != nextURL {
		t.Errorf("fetcher.calls = %#v, want [initial, next]", fetcher.calls)
	}
	if len(items.upserts) != 2 {
		t.Errorf("len(upserts) = %d, want 2", len(items.upserts))
	}

	// Cursor save was called once after the loop with status=ok + the deltaLink.
	if len(cursors.saved) != 1 {
		t.Fatalf("len(cursors.saved) = %d, want 1", len(cursors.saved))
	}
	saved := cursors.saved[0]
	if saved.deltaLink != deltaLink || saved.status != "ok" {
		t.Errorf("saved cursor = %#v, want deltaLink=%q status=ok", saved, deltaLink)
	}

	// 2 ItemUpserted + 1 SourceSynced.
	if got := countSubjects(pub.emitted); got["finspo-test.item.upserted"] != 2 || got["finspo-test.source.synced"] != 1 {
		t.Errorf("emitted subjects = %#v", got)
	}
}

func TestSyncDriveMirrorsUpsertsToDataPlaneSourceObjects(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", SiteID: "site-1", DriveID: "drive-1"}
	initialURL := "https://graph/drives/drive-1/root/delta"
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initialURL: {
			Items:     []sharepoint.DriveItem{driveFileItem("item-1", "report.docx", "QX==", "abc")},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	items := &fakeItemStore{}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	sink := &recordingSink{}

	engine := newTestEngineWithSink(fetcher, src, items, cursors, &recordingPublisher{}, sink)
	result, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}
	if result.ItemsUpserted != 1 {
		t.Fatalf("ItemsUpserted = %d, want 1", result.ItemsUpserted)
	}
	if len(sink.upserts) != 1 {
		t.Fatalf("len(sink.upserts) = %d, want 1", len(sink.upserts))
	}
	if sink.upserts[0].ItemID != "item-1" || sink.upserts[0].QuickXorHash != "QX==" {
		t.Fatalf("mirrored item = %#v", sink.upserts[0])
	}
}

func TestSyncDriveResumesFromPersistedDeltaLink(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	resumeURL := "https://graph/drives/drive-1/root/delta?token=resume"

	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		resumeURL: {
			Items:     []sharepoint.DriveItem{driveFileItem("item-3", "x.pdf", "", "")},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=after-resume",
		},
	}}
	items := &fakeItemStore{}
	cursors := &fakeCursorStore{loaded: store.Cursor{SourceID: src.ID, DeltaLink: resumeURL}}

	engine := newTestEngine(fetcher, src, items, cursors, &recordingPublisher{})
	if _, err := engine.SyncDrive(context.Background(), src.ID); err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}

	if len(fetcher.calls) != 1 || fetcher.calls[0] != resumeURL {
		t.Errorf("fetcher.calls = %#v, want [%q]", fetcher.calls, resumeURL)
	}
}

func TestSyncDriveHandlesDeletedFacet(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	initial := "https://graph/drives/drive-1/root/delta"

	knownItemID := uuid.New()
	items := &fakeItemStore{
		knownDeletes: map[string]store.Item{
			"to-delete": {ID: knownItemID, Path: "/old/path.pdf"},
		},
	}
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initial: {
			Items: []sharepoint.DriveItem{
				driveDeletedItem("to-delete"),
				driveDeletedItem("never-seen"),
			},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	pub := &recordingPublisher{}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}

	engine := newTestEngine(fetcher, src, items, cursors, pub)
	result, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}

	if result.ItemsDeleted != 1 {
		t.Errorf("ItemsDeleted = %d, want 1 (tombstone for unknown item should be skipped)", result.ItemsDeleted)
	}
	if len(items.softDeletes) != 2 {
		t.Errorf("len(softDeletes) = %d, want 2 calls (one resolved, one not-found)", len(items.softDeletes))
	}

	if got := countSubjects(pub.emitted); got["finspo-test.item.deleted"] != 1 {
		t.Errorf("emitted item.deleted = %d, want 1", got["finspo-test.item.deleted"])
	}
}

func TestSyncDriveMirrorsDeletesToDataPlaneSourceObjects(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	initial := "https://graph/drives/drive-1/root/delta"
	items := &fakeItemStore{
		knownDeletes: map[string]store.Item{
			"to-delete": {ID: uuid.New(), ItemID: "to-delete", Path: "/old/path.pdf"},
		},
	}
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initial: {
			Items:     []sharepoint.DriveItem{driveDeletedItem("to-delete")},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	sink := &recordingSink{}

	engine := newTestEngineWithSink(fetcher, src, items, &fakeCursorStore{loadErr: store.ErrNotFound}, &recordingPublisher{}, sink)
	result, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}
	if result.ItemsDeleted != 1 {
		t.Fatalf("ItemsDeleted = %d, want 1", result.ItemsDeleted)
	}
	if len(sink.deletes) != 1 || sink.deletes[0].ItemID != "to-delete" {
		t.Fatalf("sink.deletes = %#v", sink.deletes)
	}
}

func TestSyncDriveRecordsFailureOnFetchError(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{}} // empty → "unexpected url"

	items := &fakeItemStore{}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	pub := &recordingPublisher{}

	engine := newTestEngine(fetcher, src, items, cursors, pub)
	if _, err := engine.SyncDrive(context.Background(), src.ID); err == nil {
		t.Fatal("expected error, got nil")
	}

	// recordFailure should have written a cursor row with status=error.
	if len(cursors.saved) != 1 {
		t.Fatalf("len(cursors.saved) = %d, want 1", len(cursors.saved))
	}
	if cursors.saved[0].status != "error" {
		t.Errorf("cursor status = %q, want error", cursors.saved[0].status)
	}
}

func countSubjects(es []emitted) map[string]int {
	out := make(map[string]int)
	for _, e := range es {
		out[e.Subject]++
	}
	return out
}
