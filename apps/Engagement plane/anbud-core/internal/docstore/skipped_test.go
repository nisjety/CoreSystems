package docstore

import (
	"errors"
	"reflect"
	"testing"
)

// A file that was skipped was never read, so there are no bytes to hash. It is in the manifest all
// the same, with its path and the reason, so that nothing that was in the archive is missing from it.
func TestAFileThatWasSkippedNeedsNoHash(t *testing.T) {
	s, _ := openStore(t)
	in := sample("org-a", "2026-100001", "a")
	in.Documents = append(in.Documents,
		Document{Path: "tegninger/../hemmelig.pdf", Kind: "other", Status: StatusSkipped, Reason: "bad_name"},
		Document{Path: "vedlegg.zip/dypt.zip", Kind: "other", Status: StatusSkipped, Reason: "too_deep"},
	)
	got, created, err := s.Create(testCtx(t), in)
	if err != nil || !created {
		t.Fatalf("Create = %v, %v", created, err)
	}
	if got.ReadCount != 1 || got.UnreadCount != 4 {
		t.Errorf("read %d unread %d, want 1 and 4: a skipped file is a file that could not be read", got.ReadCount, got.UnreadCount)
	}
	if !reflect.DeepEqual(got.Documents, in.Documents) {
		t.Errorf("documents = %+v\nwant %+v", got.Documents, in.Documents)
	}
}

// Every other file has been read, or tried, and has the hash of its bytes.
func TestAFileThatWasNotSkippedNeedsItsHash(t *testing.T) {
	s, _ := openStore(t)
	for _, status := range []string{StatusRead, StatusEmpty, StatusUnsupported, StatusFailed, StatusEncrypted, StatusTooLarge, StatusDuplicate} {
		in := sample("org-a", "2026-100001", "a")
		in.Documents = []Document{{Path: "a.pdf", Kind: "pdf", Status: status}}
		if _, _, err := s.Create(testCtx(t), in); !errors.Is(err, ErrInvalid) {
			t.Errorf("status %s without a hash: err = %v, want ErrInvalid", status, err)
		}
	}
	in := sample("org-a", "2026-100001", "a")
	in.Documents = []Document{{Path: "a.pdf", Kind: "pdf", Status: StatusSkipped, SHA256: "not-a-hash"}}
	if _, _, err := s.Create(testCtx(t), in); !errors.Is(err, ErrInvalid) {
		t.Errorf("a skipped file with a hash that is not one: err = %v, want ErrInvalid", err)
	}
}

// Skipped files have no hash to tell them apart by, so the path does: two of them are two files that
// could not be read, and the same path in two archives of a notice is one.
func TestSkippedFilesAreCountedByPathAcrossArchives(t *testing.T) {
	s, _ := openStore(t)
	a := sample("org-a", "2026-100001", "a")
	a.Documents = []Document{
		{Path: "x.zip/a.bin", Kind: "other", Status: StatusSkipped, Reason: "too_large"},
		{Path: "x.zip/b.bin", Kind: "other", Status: StatusSkipped, Reason: "too_large"},
	}
	a.Findings = nil
	b := sample("org-a", "2026-100001", "b")
	b.Documents = []Document{
		{Path: "x.zip/a.bin", Kind: "other", Status: StatusSkipped, Reason: "too_large"}, // the same file, uploaded again
		{Path: "x.zip/c.bin", Kind: "other", Status: StatusSkipped, Reason: "encrypted"},
	}
	b.Findings = nil
	for _, u := range []NewUpload{a, b} {
		if _, _, err := s.Create(testCtx(t), u); err != nil {
			t.Fatal(err)
		}
	}
	ev, err := s.EvidenceFor(testCtx(t), "org-a", []string{"2026-100001"})
	if err != nil {
		t.Fatal(err)
	}
	if got := ev["2026-100001"]; got.Documents != 0 || got.Unread != 3 {
		t.Errorf("documents %d unread %d, want 0 and 3 (a, b and c)", got.Documents, got.Unread)
	}
}

// What was left out of an upload is counted, not lost: the findings the store keeps are capped, and
// how many were dropped is kept with the upload.
func TestTheNumberOfFindingsThatWereDroppedIsKept(t *testing.T) {
	s, _ := openStore(t)
	in := sample("org-a", "2026-100001", "a")
	in.FindingsDropped = 37
	got, _, err := s.Create(testCtx(t), in)
	if err != nil || got.FindingsDropped != 37 {
		t.Fatalf("Create = %+v, %v; want 37 dropped", got, err)
	}
	listed, err := s.List(testCtx(t), "org-a", "2026-100001")
	if err != nil || len(listed) != 1 || listed[0].FindingsDropped != 37 {
		t.Errorf("List = %+v, %v", listed, err)
	}

	in = sample("org-a", "2026-100001", "b")
	in.FindingsDropped = -1
	if _, _, err := s.Create(testCtx(t), in); !errors.Is(err, ErrInvalid) {
		t.Errorf("a negative count: err = %v, want ErrInvalid", err)
	}
}

// An upload that has expired is still a row until it is purged, and it holds the name of the same
// archive: uploading that archive again must not have to wait for the purge.
func TestTheSameArchiveCanBeUploadedAgainOnceItsEarlierUploadHasExpired(t *testing.T) {
	s, pool := openStore(t)
	insertExpired(t, pool, "org-a", "2026-100001", "same")

	got, created, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "same"))
	if err != nil || !created {
		t.Fatalf("Create = %+v, %v, %v; want a new upload", got, created, err)
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 1 {
		t.Errorf("rows = %d, want the expired one replaced by the new", n)
	}
	if live, err := s.List(testCtx(t), "org-a", "2026-100001"); err != nil || len(live) != 1 || live[0].ID != got.ID {
		t.Errorf("List = %+v, %v", live, err)
	}
}

// Replacing an expired upload must touch nothing else: not another organisation's, not another archive's.
func TestReplacingAnExpiredUploadLeavesTheOthersAlone(t *testing.T) {
	s, pool := openStore(t)
	insertExpired(t, pool, "org-b", "2026-100001", "same")      // another organisation's expired upload of the same archive
	insertExpired(t, pool, "org-a", "2026-100002", "same")      // another notice
	insertExpired(t, pool, "org-a", "2026-100001", "different") // another archive
	if _, _, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "same")); err != nil {
		t.Fatal(err)
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 4 {
		t.Errorf("rows = %d, want 4: the three that are not the same upload stay until they are purged", n)
	}
}
