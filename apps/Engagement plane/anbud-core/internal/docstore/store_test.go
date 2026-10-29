package docstore

import (
	"reflect"
	"testing"
	"time"
)

func TestCreateStoresTheManifestAndTheFindings(t *testing.T) {
	s, _ := openStore(t)
	in := sample("org-a", "2026-100001", "a")
	got, created, err := s.Create(testCtx(t), in)
	if err != nil || !created {
		t.Fatalf("Create = %v, %v", created, err)
	}
	if got.ID <= 0 || got.OrgID != "org-a" || got.NoticeID != "2026-100001" || got.UploadedBy != "user-1" ||
		got.ArchiveName != "konkurransegrunnlag.zip" || got.ArchiveSHA256 != in.ArchiveSHA256 || got.ArchiveBytes != 12345 {
		t.Errorf("upload = %+v", got)
	}
	// One file was read. The scan and the workbook were not, and the upload says so.
	if got.ReadCount != 1 || got.UnreadCount != 2 {
		t.Errorf("read %d unread %d, want 1 and 2", got.ReadCount, got.UnreadCount)
	}
	if !reflect.DeepEqual(got.Documents, in.Documents) || !reflect.DeepEqual(got.Findings, in.Findings) {
		t.Errorf("documents %+v findings %+v", got.Documents, got.Findings)
	}
	if d := time.Since(got.CreatedAt); d < -time.Minute || d > time.Minute {
		t.Errorf("created %v ago", d)
	}
	if want := got.CreatedAt.Add(90 * 24 * time.Hour); got.ExpiresAt.Sub(want) > time.Second || want.Sub(got.ExpiresAt) > time.Second {
		t.Errorf("expires %v, want 90 days after %v", got.ExpiresAt, got.CreatedAt)
	}

	again, err := s.List(testCtx(t), "org-a", "2026-100001")
	if err != nil || len(again) != 1 || !reflect.DeepEqual(again[0].Documents, in.Documents) || !reflect.DeepEqual(again[0].Findings, in.Findings) || again[0].ID != got.ID {
		t.Errorf("List = %+v, %v", again, err)
	}
}

func TestTheSameArchiveForTheSameNoticeIsOneUpload(t *testing.T) {
	s, pool := openStore(t)
	first, created, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "a"))
	if err != nil || !created {
		t.Fatalf("first Create = %v, %v", created, err)
	}
	again, created, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "a"))
	if err != nil || created || again.ID != first.ID {
		t.Errorf("second Create = id %d created %v err %v, want the first upload (id %d) and not created", again.ID, created, err, first.ID)
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 1 {
		t.Errorf("rows = %d, want 1", n)
	}
}

// What makes an upload the same one is who sent it, for which notice, and what the archive was.
func TestAnArchiveIsKeptSeparatelyPerOrganisationAndPerNotice(t *testing.T) {
	s, pool := openStore(t)
	for _, u := range []NewUpload{
		sample("org-a", "2026-100001", "a"),
		sample("org-b", "2026-100001", "a"), // another organisation, the same archive
		sample("org-a", "2026-100002", "a"), // another notice, the same archive
		sample("org-a", "2026-100001", "b"), // the same notice, another archive
	} {
		if _, created, err := s.Create(testCtx(t), u); err != nil || !created {
			t.Fatalf("Create %s %s: %v %v", u.OrgID, u.NoticeID, created, err)
		}
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 4 {
		t.Errorf("rows = %d, want 4", n)
	}
}

func TestListIsScopedToTheOrganisationAndTheNotice(t *testing.T) {
	s, _ := openStore(t)
	for _, u := range []NewUpload{
		sample("org-a", "2026-100001", "a"), sample("org-a", "2026-100001", "b"),
		sample("org-a", "2026-100002", "c"), sample("org-b", "2026-100001", "d"),
	} {
		if _, _, err := s.Create(testCtx(t), u); err != nil {
			t.Fatal(err)
		}
	}
	got, err := s.List(testCtx(t), "org-a", "2026-100001")
	if err != nil || len(got) != 2 {
		t.Fatalf("List = %d uploads, %v; want org-a's two for this notice", len(got), err)
	}
	if got[0].CreatedAt.After(got[1].CreatedAt) || got[0].ID >= got[1].ID {
		t.Errorf("uploads are not oldest first: %+v", []int64{got[0].ID, got[1].ID})
	}
	for _, u := range got {
		if u.OrgID != "org-a" || u.NoticeID != "2026-100001" {
			t.Errorf("a stranger's upload: %+v", u)
		}
	}
	if none, err := s.List(testCtx(t), "org-c", "2026-100001"); err != nil || len(none) != 0 {
		t.Errorf("another organisation sees %d uploads, %v", len(none), err)
	}
}

func TestEvidenceForSumsTheUploadsOfEachNoticeOnce(t *testing.T) {
	s, _ := openStore(t)

	a := sample("org-a", "2026-100001", "a")
	b := sample("org-a", "2026-100001", "b")
	// The second archive is a revision: the same specification again, a contract that is new, the same
	// scan, and a finding that repeats the first archive's.
	b.Documents = []Document{
		{Path: "rev/konkurransegrunnlag.pdf", Kind: "pdf", Size: 5000, SHA256: sha("kg"), Status: StatusRead, Pages: 12, Chars: 3000, Extractor: "pdf"},
		{Path: "rev/kontrakt.pdf", Kind: "pdf", Size: 700, SHA256: sha("kontrakt"), Status: StatusRead, Pages: 4, Chars: 900, Extractor: "pdf"},
		{Path: "rev/skann.pdf", Kind: "pdf", Size: 9000, SHA256: sha("skann"), Status: StatusEmpty, Pages: 3, Extractor: "pdf"},
	}
	b.Findings = []Finding{
		{Document: "konkurransegrunnlag.pdf", Capability: "iso9001", Class: "requirement", Page: 3, Snippet: "Leverandøren skal ha ISO 9001."},
		{Document: "rev/kontrakt.pdf", Capability: "startbank", Class: "requirement", Page: 2, Snippet: "Leverandøren må være registrert i StartBANK."},
	}
	other := sample("org-a", "2026-100002", "c")
	other.Findings = []Finding{{Document: "x.pdf", Capability: "achilles", Class: "requirement", Page: 1, Snippet: "Achilles kreves."}}
	stranger := sample("org-b", "2026-100001", "a")
	for _, u := range []NewUpload{a, b, other, stranger} {
		if _, _, err := s.Create(testCtx(t), u); err != nil {
			t.Fatal(err)
		}
	}

	got, err := s.EvidenceFor(testCtx(t), "org-a", []string{"2026-100001", "2026-100002", "2026-100003"})
	if err != nil {
		t.Fatalf("EvidenceFor: %v", err)
	}
	if len(got) != 2 {
		t.Fatalf("evidence for %d notices, want 2 (the third has none): %v", len(got), got)
	}
	one := got["2026-100001"]
	// Read: the specification (in both archives, one file) and the contract. Unread: the scan (in both,
	// one file) and the workbook.
	if one.Documents != 2 || one.Unread != 2 {
		t.Errorf("documents %d unread %d, want 2 and 2: a file in two archives is one file", one.Documents, one.Unread)
	}
	wantFindings := []Finding{a.Findings[0], b.Findings[1]}
	if !reflect.DeepEqual(one.Findings, wantFindings) {
		t.Errorf("findings = %+v\nwant %+v: the repeated one is kept once, the oldest upload first", one.Findings, wantFindings)
	}
	if two := got["2026-100002"]; two.Documents != 1 || len(two.Findings) != 1 || two.Findings[0].Capability != "achilles" {
		t.Errorf("the other notice: %+v", two)
	}
	for id := range got {
		if id == "2026-100003" {
			t.Errorf("evidence for a notice with no upload")
		}
	}
}

func TestTextWithQuotesNewlinesAndNorwegianLettersSurvives(t *testing.T) {
	s, _ := openStore(t)
	in := sample("org-a", "2026-100001", "a")
	in.Findings[0].Snippet = "Krav: «ISO 9001» \"skal\" ha\nlinjeskift, tab\t, backslash \\ og æøå ÆØÅ   slutt"
	in.Documents[0].Path = "vedlegg/Kravspesifikasjon – æøå (versjon 2).pdf"
	got, _, err := s.Create(testCtx(t), in)
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	if got.Findings[0].Snippet != in.Findings[0].Snippet || got.Documents[0].Path != in.Documents[0].Path {
		t.Errorf("text changed on its way through: %q %q", got.Findings[0].Snippet, got.Documents[0].Path)
	}
}

func TestDeleteRemovesAnUploadForItsOwnOrganisationOnly(t *testing.T) {
	s, pool := openStore(t)
	up, _, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "a"))
	if err != nil {
		t.Fatal(err)
	}
	if ok, err := s.Delete(testCtx(t), "org-b", "2026-100001", up.ID); err != nil || ok {
		t.Errorf("another organisation deleted it: %v, %v", ok, err)
	}
	// An upload belongs to one notice and is deleted through it.
	if ok, err := s.Delete(testCtx(t), "org-a", "2026-100002", up.ID); err != nil || ok {
		t.Errorf("a delete through another notice: %v, %v", ok, err)
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 1 {
		t.Fatalf("rows = %d after the wrong deletes, want 1", n)
	}
	if ok, err := s.Delete(testCtx(t), "org-a", "2026-100001", up.ID); err != nil || !ok {
		t.Errorf("the owner's delete = %v, %v", ok, err)
	}
	if ok, err := s.Delete(testCtx(t), "org-a", "2026-100001", up.ID); err != nil || ok {
		t.Errorf("a second delete = %v, %v; want false: there is nothing left", ok, err)
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 0 {
		t.Errorf("rows = %d, want 0", n)
	}
}

func TestFindSameFindsTheLiveUploadOfTheSameArchiveAndNothingElse(t *testing.T) {
	s, pool := openStore(t)
	a, _, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "a"))
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "b")); err != nil {
		t.Fatal(err)
	}
	got, found, err := s.FindSame(testCtx(t), "org-a", "2026-100001", sha("a"))
	if err != nil || !found || got.ID != a.ID || !reflect.DeepEqual(got.Documents, a.Documents) || !reflect.DeepEqual(got.Findings, a.Findings) {
		t.Errorf("FindSame = %+v, %v, %v; want the upload of archive a", got, found, err)
	}
	for name, tc := range map[string]struct{ org, notice, archive string }{
		"another organisation": {"org-b", "2026-100001", "a"},
		"another notice":       {"org-a", "2026-100002", "a"},
		"another archive":      {"org-a", "2026-100001", "c"},
	} {
		if _, found, err := s.FindSame(testCtx(t), tc.org, tc.notice, sha(tc.archive)); err != nil || found {
			t.Errorf("%s: found %v, %v; want nothing", name, found, err)
		}
	}
	// An upload that has expired is gone to it as to every read.
	if _, err := pool.Exec(testCtx(t), `UPDATE tenant.tender_uploads SET created_at = now() - interval '3 days', expires_at = now() - interval '1 day' WHERE id = $1`, a.ID); err != nil {
		t.Fatal(err)
	}
	if _, found, err := s.FindSame(testCtx(t), "org-a", "2026-100001", sha("a")); err != nil || found {
		t.Errorf("an expired upload: found %v, %v; want nothing", found, err)
	}
}

func TestExpiredUploadsAreGoneToEveryReadBeforeTheyArePurged(t *testing.T) {
	s, pool := openStore(t)
	insertExpired(t, pool, "org-a", "2026-100001", "old")
	if _, _, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "new")); err != nil {
		t.Fatal(err)
	}

	got, err := s.List(testCtx(t), "org-a", "2026-100001")
	if err != nil || len(got) != 1 {
		t.Errorf("List = %d uploads, %v; want only the one that has not expired", len(got), err)
	}
	ev, err := s.EvidenceFor(testCtx(t), "org-a", []string{"2026-100001"})
	if err != nil || ev["2026-100001"].Documents != 1 {
		t.Errorf("EvidenceFor = %+v, %v", ev, err)
	}
	if forNotice, forOrg, err := s.Counts(testCtx(t), "org-a", "2026-100001"); err != nil || forNotice != 1 || forOrg != 1 {
		t.Errorf("Counts = %d, %d, %v; want 1 and 1", forNotice, forOrg, err)
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 2 {
		t.Errorf("rows = %d: the expired one is still there until it is purged", n)
	}
}

func TestPurgeExpiredRemovesOnlyWhatHasExpired(t *testing.T) {
	s, pool := openStore(t)
	insertExpired(t, pool, "org-a", "2026-100001", "old-a")
	insertExpired(t, pool, "org-b", "2026-100002", "old-b")
	if _, _, err := s.Create(testCtx(t), sample("org-a", "2026-100001", "new")); err != nil {
		t.Fatal(err)
	}

	removed, err := s.PurgeExpired(testCtx(t))
	if err != nil || removed != 2 {
		t.Errorf("PurgeExpired = %d, %v; want 2, for both organisations", removed, err)
	}
	if n := count(t, pool, `SELECT count(*) FROM tenant.tender_uploads`); n != 1 {
		t.Errorf("rows = %d, want the one that has not expired", n)
	}
	if removed, err := s.PurgeExpired(testCtx(t)); err != nil || removed != 0 {
		t.Errorf("a second purge = %d, %v", removed, err)
	}
}

func TestCountsSayHowManyUploadsAnOrganisationHolds(t *testing.T) {
	s, _ := openStore(t)
	for _, u := range []NewUpload{
		sample("org-a", "2026-100001", "a"), sample("org-a", "2026-100001", "b"), sample("org-a", "2026-100002", "c"),
		sample("org-b", "2026-100001", "d"),
	} {
		if _, _, err := s.Create(testCtx(t), u); err != nil {
			t.Fatal(err)
		}
	}
	forNotice, forOrg, err := s.Counts(testCtx(t), "org-a", "2026-100001")
	if err != nil || forNotice != 2 || forOrg != 3 {
		t.Errorf("Counts = %d, %d, %v; want 2 for the notice and 3 in all", forNotice, forOrg, err)
	}
}

func TestTheDatabaseRefusesWhatTheStoreWouldNot(t *testing.T) {
	_, pool := openStore(t)
	// The checks are in the table too, so a bug in the store cannot write what the table forbids.
	for name, sql := range map[string]string{
		"an expiry before the creation":   `INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, expires_at) VALUES ('o', '2026-100001', repeat('a', 64), 1, '[]', '[]', 0, 0, now() - interval '1 day')`,
		"three years of retention":        `INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, expires_at) VALUES ('o', '2026-100001', repeat('a', 64), 1, '[]', '[]', 0, 0, now() + interval '3 years')`,
		"documents that are not an array": `INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, expires_at) VALUES ('o', '2026-100001', repeat('a', 64), 1, '{}', '[]', 0, 0, now() + interval '1 day')`,
		"a notice id that is not one":     `INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, expires_at) VALUES ('o', 'x', repeat('a', 64), 1, '[]', '[]', 0, 0, now() + interval '1 day')`,
		"an organisation with a space":    `INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, expires_at) VALUES ('o r', '2026-100001', repeat('a', 64), 1, '[]', '[]', 0, 0, now() + interval '1 day')`,
	} {
		if _, err := pool.Exec(testCtx(t), sql); err == nil {
			t.Errorf("%s: the table accepted it", name)
		}
	}
}
