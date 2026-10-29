package sync

import (
	"context"
	"testing"

	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
)

func TestPathWithinFolder(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name   string
		folder string
		item   string
		want   bool
	}{
		{name: "empty scope accepts everything", folder: "", item: "/x/y.txt", want: true},
		{name: "item inside folder", folder: "/Reports", item: "/Reports/q1.xlsx", want: true},
		{name: "item deep inside folder", folder: "/Reports", item: "/Reports/2026/q1.xlsx", want: true},
		{name: "the folder itself", folder: "/Reports", item: "/Reports", want: true},
		{name: "sibling with same prefix", folder: "/Reports", item: "/Reports2026/q1.xlsx", want: false},
		{name: "outside folder", folder: "/Reports", item: "/Other/q1.xlsx", want: false},
		{name: "case-insensitive match", folder: "/Reports", item: "/reports/Q1.XLSX", want: true},
		{name: "trailing slash on scope", folder: "/Reports/", item: "/Reports/q1.xlsx", want: true},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			if got := pathWithinFolder(tc.folder, tc.item); got != tc.want {
				t.Fatalf("pathWithinFolder(%q, %q) = %v, want %v", tc.folder, tc.item, got, tc.want)
			}
		})
	}
}

func TestSyncDriveFolderScopeSkipsOutOfScopeItems(t *testing.T) {
	t.Parallel()

	src := store.Source{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		DriveID:        "drive-1",
		Kind:           store.SourceKindDrive,
		FolderID:       "folder-reports",
		FolderPath:     "/Reports",
	}

	inScope := driveFileItem("item-in", "q1.xlsx", "QX==", "abc") // parent /Reports
	outOfScope := driveFileItem("item-out", "notes.txt", "", "")
	outOfScope.Parent = &sharepoint.ParentReference{Path: "/drives/d1/root:/Private"}

	initialURL := "https://graph/drives/drive-1/root/delta"
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initialURL: {
			Items:     []sharepoint.DriveItem{inScope, outOfScope},
			DeltaLink: initialURL + "?token=final",
		},
	}}

	items := &fakeItemStore{}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	pub := &recordingPublisher{}
	engine := newTestEngine(fetcher, src, items, cursors, pub)

	res, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive(): %v", err)
	}

	if res.ItemsUpserted != 1 {
		t.Fatalf("ItemsUpserted = %d, want 1 (out-of-scope item skipped)", res.ItemsUpserted)
	}
	if len(items.upserts) != 1 || items.upserts[0].ID != "item-in" {
		t.Fatalf("upserts = %+v, want only item-in", items.upserts)
	}
	upsertEvents := 0
	for _, e := range pub.emitted {
		if e.Subject == engine.subjects.ItemUpserted() {
			upsertEvents++
		}
	}
	if upsertEvents != 1 {
		t.Fatalf("item.upserted events = %d, want 1 (only the in-scope item announced)", upsertEvents)
	}
}

func TestSyncDriveFolderScopeStillProcessesDeletes(t *testing.T) {
	t.Parallel()

	src := store.Source{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		DriveID:        "drive-1",
		Kind:           store.SourceKindDrive,
		FolderID:       "folder-reports",
		FolderPath:     "/Reports",
	}

	initialURL := "https://graph/drives/drive-1/root/delta"
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initialURL: {
			Items:     []sharepoint.DriveItem{driveDeletedItem("item-gone")},
			DeltaLink: initialURL + "?token=final",
		},
	}}

	items := &fakeItemStore{knownDeletes: map[string]store.Item{
		"item-gone": {Path: "/Reports/old.docx", Name: "old.docx"},
	}}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}
	engine := newTestEngine(fetcher, src, items, cursors, &recordingPublisher{})

	res, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive(): %v", err)
	}
	if res.ItemsDeleted != 1 {
		t.Fatalf("ItemsDeleted = %d, want 1 (tombstones bypass the scope filter)", res.ItemsDeleted)
	}
}
