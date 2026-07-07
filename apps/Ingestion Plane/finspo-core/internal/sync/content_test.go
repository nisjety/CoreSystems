package sync

import (
	"context"
	"errors"
	"io"
	"testing"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
)

type fakeContentSink struct {
	items []store.Item
	err   error
}

func (f *fakeContentSink) IngestItemContent(_ context.Context, _ store.Source, item store.Item) error {
	f.items = append(f.items, item)
	return f.err
}

func newContentTestEngine(fetcher DeltaFetcher, src store.Source, items *fakeItemStore, cursors *fakeCursorStore, pub *recordingPublisher, content ContentSink) *Engine {
	return NewEngine(Config{
		Fetcher:   fetcher,
		Sources:   &fakeSourceStore{src: src},
		Items:     items,
		Cursors:   cursors,
		Publisher: pub,
		Content:   content,
		Subjects:  events.NewSubjects("finspo-test"),
		Logger:    zerolog.New(io.Discard),
	})
}

func TestSyncDriveForwardsFileContentToContentSink(t *testing.T) {
	t.Parallel()
	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	initialURL := "https://graph/drives/drive-1/root/delta"
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initialURL: {
			Items:     []sharepoint.DriveItem{driveFileItem("item-1", "report.pdf", "QX==", "abc")},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	content := &fakeContentSink{}
	engine := newContentTestEngine(fetcher, src, &fakeItemStore{}, &fakeCursorStore{}, &recordingPublisher{}, content)

	if _, err := engine.SyncDrive(context.Background(), src.ID); err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}
	if len(content.items) != 1 {
		t.Fatalf("expected content sink invoked once for the file item, got %d", len(content.items))
	}
	if content.items[0].ItemID != "item-1" {
		t.Errorf("forwarded item = %q, want item-1", content.items[0].ItemID)
	}
}

func TestSyncDriveContentSinkFailureDoesNotAbortSync(t *testing.T) {
	t.Parallel()
	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	initialURL := "https://graph/drives/drive-1/root/delta"
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initialURL: {
			Items:     []sharepoint.DriveItem{driveFileItem("item-1", "report.pdf", "", "")},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	content := &fakeContentSink{err: errors.New("graph download failed")}
	engine := newContentTestEngine(fetcher, src, &fakeItemStore{}, &fakeCursorStore{}, &recordingPublisher{}, content)

	result, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("content-sink failure must NOT abort the sync, got %v", err)
	}
	if result.ItemsUpserted != 1 {
		t.Errorf("metadata sync must still succeed: ItemsUpserted = %d, want 1", result.ItemsUpserted)
	}
}
