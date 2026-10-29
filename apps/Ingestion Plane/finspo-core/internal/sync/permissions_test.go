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

type fakePermsFetcher struct {
	entries map[string][]sharepoint.PermissionEntry // keyed by itemID
	calls   []string
	err     error
}

func (f *fakePermsFetcher) ListItemPermissions(_ context.Context, _ string, _ string, itemID string) ([]sharepoint.PermissionEntry, error) {
	f.calls = append(f.calls, itemID)
	if f.err != nil {
		return nil, f.err
	}
	return f.entries[itemID], nil
}

type fakePermsStore struct {
	calls []permsStoreCall
	saved []store.Permission
	err   error
}

type permsStoreCall struct {
	itemPK  uuid.UUID
	entries []sharepoint.PermissionEntry
}

func (f *fakePermsStore) ReplaceAll(_ context.Context, itemPK uuid.UUID, entries []sharepoint.PermissionEntry) ([]store.Permission, error) {
	f.calls = append(f.calls, permsStoreCall{itemPK: itemPK, entries: entries})
	return append([]store.Permission(nil), f.saved...), f.err
}

func newTestEngineWithPerms(fetcher DeltaFetcher, src store.Source, items *fakeItemStore, cursors *fakeCursorStore, pub *recordingPublisher, pf *fakePermsFetcher, ps *fakePermsStore, capture bool) *Engine {
	return NewEngine(Config{
		Fetcher:            fetcher,
		Sources:            &fakeSourceStore{src: src},
		Items:              items,
		Cursors:            cursors,
		Publisher:          pub,
		PermissionsFetcher: pf,
		PermissionsStore:   ps,
		CapturePermissions: capture,
		Subjects:           events.NewSubjects("finspo-test"),
		Logger:             zerolog.New(io.Discard),
	})
}

func TestSyncDriveCapturesPermissionsForFileUpsertsOnly(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	initial := "https://graph/drives/drive-1/root/delta"

	folder := sharepoint.DriveItem{
		ID:     "folder-1",
		Name:   "Reports",
		Folder: &sharepoint.FolderFacet{},
		Parent: &sharepoint.ParentReference{Path: "/drives/drive-1/root:"},
	}
	file := driveFileItem("file-1", "report.pdf", "QX==", "abc")

	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initial: {
			Items:     []sharepoint.DriveItem{folder, file},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	pf := &fakePermsFetcher{entries: map[string][]sharepoint.PermissionEntry{
		"file-1": {{Roles: []string{"read"}, GrantedToV2: &sharepoint.PermissionIdentitySet{User: &sharepoint.PermissionIdentity{ID: "u1"}}}},
	}}
	ps := &fakePermsStore{}
	cursors := &fakeCursorStore{loadErr: store.ErrNotFound}

	engine := newTestEngineWithPerms(fetcher, src, &fakeItemStore{}, cursors, &recordingPublisher{}, pf, ps, true)
	if _, err := engine.SyncDrive(context.Background(), src.ID); err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}

	// Permissions should be fetched ONLY for the file, not the folder.
	if len(pf.calls) != 1 || pf.calls[0] != "file-1" {
		t.Errorf("pf.calls = %#v, want [file-1]", pf.calls)
	}
	if len(ps.calls) != 1 || len(ps.calls[0].entries) != 1 {
		t.Errorf("ps.calls = %#v", ps.calls)
	}
}

func TestSyncDriveSkipsPermissionsWhenCaptureDisabled(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	initial := "https://graph/drives/drive-1/root/delta"
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initial: {
			Items:     []sharepoint.DriveItem{driveFileItem("file-1", "x.pdf", "", "")},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	pf := &fakePermsFetcher{}
	ps := &fakePermsStore{}
	engine := newTestEngineWithPerms(fetcher, src, &fakeItemStore{}, &fakeCursorStore{loadErr: store.ErrNotFound}, &recordingPublisher{}, pf, ps, false)
	if _, err := engine.SyncDrive(context.Background(), src.ID); err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}
	if len(pf.calls) != 0 || len(ps.calls) != 0 {
		t.Errorf("expected no perm calls; pf=%d ps=%d", len(pf.calls), len(ps.calls))
	}
}

func TestSyncDriveContinuesWhenPermFetchFails(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	initial := "https://graph/drives/drive-1/root/delta"
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initial: {
			Items: []sharepoint.DriveItem{
				driveFileItem("file-1", "a.pdf", "", ""),
				driveFileItem("file-2", "b.pdf", "", ""),
			},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	pf := &fakePermsFetcher{err: errors.New("graph 500")}
	ps := &fakePermsStore{}
	items := &fakeItemStore{}

	engine := newTestEngineWithPerms(fetcher, src, items, &fakeCursorStore{loadErr: store.ErrNotFound}, &recordingPublisher{}, pf, ps, true)
	result, err := engine.SyncDrive(context.Background(), src.ID)
	if err != nil {
		t.Fatalf("SyncDrive should not fail on permission error: %v", err)
	}
	// Both items should still be persisted despite ACL errors.
	if result.ItemsUpserted != 2 {
		t.Errorf("ItemsUpserted = %d, want 2", result.ItemsUpserted)
	}
	if len(items.upserts) != 2 {
		t.Errorf("len(items.upserts) = %d, want 2", len(items.upserts))
	}
}

func TestSyncDriveMirrorsCapturedPermissionsToPermissionAwareSink(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1"}
	initial := "https://graph/drives/drive-1/root/delta"
	fetcher := &fakeFetcher{pages: map[string]sharepoint.DeltaPage{
		initial: {
			Items:     []sharepoint.DriveItem{driveFileItem("file-1", "acl.pdf", "", "")},
			DeltaLink: "https://graph/drives/drive-1/root/delta?token=done",
		},
	}}
	pf := &fakePermsFetcher{entries: map[string][]sharepoint.PermissionEntry{
		"file-1": {{Roles: []string{"read"}, GrantedToV2: &sharepoint.PermissionIdentitySet{User: &sharepoint.PermissionIdentity{ID: "user-1"}}}},
	}}
	ps := &fakePermsStore{saved: []store.Permission{{
		PrincipalID:   "user-1",
		PrincipalType: "user",
		Roles:         []string{"read"},
	}}}
	sink := &recordingSink{}

	engine := NewEngine(Config{
		Fetcher:            fetcher,
		Sources:            &fakeSourceStore{src: src},
		Items:              &fakeItemStore{},
		Cursors:            &fakeCursorStore{loadErr: store.ErrNotFound},
		Publisher:          &recordingPublisher{},
		Sink:               sink,
		PermissionsFetcher: pf,
		PermissionsStore:   ps,
		CapturePermissions: true,
		Subjects:           events.NewSubjects("finspo-test"),
		Logger:             zerolog.New(io.Discard),
	})

	if _, err := engine.SyncDrive(context.Background(), src.ID); err != nil {
		t.Fatalf("SyncDrive: %v", err)
	}
	if len(sink.permissionUpserts) != 1 {
		t.Fatalf("len(permissionUpserts) = %d, want 1", len(sink.permissionUpserts))
	}
	if len(sink.permissionUpserts[0].permissions) != 1 {
		t.Fatalf("permissions = %#v", sink.permissionUpserts[0].permissions)
	}
	if sink.permissionUpserts[0].permissions[0].PrincipalID != "user-1" {
		t.Fatalf("permissions = %#v", sink.permissionUpserts[0].permissions)
	}
}
