package tenderdocs

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/docread"
	"github.com/triodelab/anbud/internal/docstore"
)

// What the service asks of its stores, in which order, and what each call may cost.

// within runs fn and fails the test if it has not returned in d: a service that waits for ever on a
// database must fail its test, and not hang it.
func within(t *testing.T, d time.Duration, what string, fn func() error) error {
	t.Helper()
	done := make(chan error, 1)
	go func() { done <- fn() }()
	select {
	case err := <-done:
		return err
	case <-time.After(d):
		t.Fatalf("%s did not return within %s", what, d)
		return nil
	}
}

// The same archive again is the upload it already is: it is answered before the quota is asked about,
// because an organisation that is at its limit loses nothing by sending what it already has.
func TestTheSameArchiveAgainIsAnsweredBeforeTheQuotaAndBeforeAnyReading(t *testing.T) {
	reader := &scriptReader{fn: readsAs(1)}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	archive := zipOf(t, file("a.txt", "innhold"))
	first, created, err := f.upload(t, archive)
	if err != nil || !created {
		t.Fatalf("first = %v, %v", created, err)
	}
	reads, counts := reader.count(), f.store.callCount("Counts")

	f.store.heldNotice, f.store.heldOrg = 10, 200 // now at both limits
	again, created, err := f.upload(t, archive)
	if err != nil {
		t.Fatalf("an organisation at its limit sent what it already has: %v", err)
	}
	if created || again.ID != first.ID {
		t.Errorf("again = id %d created %v, want the first upload (%d) and not created", again.ID, created, first.ID)
	}
	if reader.count() != reads {
		t.Errorf("the same archive was read again: %d reads, want %d", reader.count(), reads)
	}
	if f.store.callCount("Counts") != counts {
		t.Errorf("the quota was asked about for an archive that is already held")
	}
	if f.store.callCount("Create") != 1 {
		t.Errorf("Create was called %d times, want 1", f.store.callCount("Create"))
	}
}

// What an organisation holds is its own: the same archive from another organisation is read and stored
// on its own, and does not find the first organisation's upload.
func TestTheSameArchiveFromAnotherOrganisationIsAnotherUpload(t *testing.T) {
	reader := &scriptReader{fn: readsAs(1)}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	archive := zipOf(t, file("a.txt", "innhold"))
	first, _, err := f.upload(t, archive)
	if err != nil {
		t.Fatal(err)
	}
	other, created, err := f.svc.Upload(context.Background(), UploadRequest{OrgID: "org-b", NoticeID: testNotice, Archive: archive})
	if err != nil || !created || other.ID == first.ID || other.OrgID != "org-b" {
		t.Fatalf("org-b = id %d org %q created %v err %v; want an upload of its own", other.ID, other.OrgID, created, err)
	}
	if reader.count() != 2 {
		t.Errorf("%d reads, want 2: the second organisation's archive is read on its own", reader.count())
	}
}

// The list of skipped files is bounded, and a file that does not fit in it is counted and not dropped:
// an archive with that many unusable files is refused, so that the manifest never hides one.
func TestAnArchiveWithMoreSkippedFilesThanTheReportListsIsRefused(t *testing.T) {
	reader := &scriptReader{fn: readsAs(1)}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	var files []entry
	for i := 0; i < 250; i++ {
		files = append(files, file("../utenfor"+itoa(i)+".txt", "x")) // every one of them is skipped
	}
	if _, _, err := f.upload(t, zipOf(t, files...)); reasonOf(err) != ReasonTooManyFiles {
		t.Errorf("err = %v, want %s: 250 skipped files are fewer than a manifest holds, but more than a report lists", err, ReasonTooManyFiles)
	}
	assertNothingStored(t, f)
	if reader.count() != 0 {
		t.Errorf("read %d files of an archive in which every file was skipped", reader.count())
	}
}

// A file that names a capability in every sentence is read to the end, and what an upload keeps is what
// it may keep, whatever the file held.
func TestAFileThatNamesACapabilityInEverySentenceKeepsNoMoreThanAnUploadMay(t *testing.T) {
	const paragraphs = 20000
	reader := &scriptReader{fn: func(context.Context, []byte) (docread.Content, error) {
		c := docread.Content{Kind: docread.KindText, Extractor: "text", Chars: paragraphs}
		for i := 1; i <= paragraphs; i++ {
			c.Units = append(c.Units, docread.Unit{Paragraph: i, Text: "Leverandøren skal ha ISO 9001 sertifisering."})
		}
		return c, nil
	}}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	if _, _, err := f.upload(t, zipOf(t, file("stor.txt", "x"))); err != nil {
		t.Fatal(err)
	}
	in := f.store.creates[0]
	if len(in.Findings) != perCapability {
		t.Errorf("kept %d findings, want %d", len(in.Findings), perCapability)
	}
	if in.FindingsDropped != paragraphs-perCapability {
		t.Errorf("dropped %d, want %d: what is left out is counted", in.FindingsDropped, paragraphs-perCapability)
	}
	if in.Findings[0].Paragraph != 1 || in.Findings[len(in.Findings)-1].Paragraph != perCapability {
		t.Errorf("kept paragraphs %d to %d, want 1 to %d: of equal weight the earliest stay", in.Findings[0].Paragraph, in.Findings[len(in.Findings)-1].Paragraph, perCapability)
	}
}

// A passage with nothing in it costs no more than looking at it, however many findings are already kept:
// a long file that names a few certificates up front and then runs on for pages must not cost the whole
// list of what is kept once for each of those pages. Without the shortcut this takes the better part of a minute
// under the race detector; with it, a second or so.
func TestAPassageWithNothingInItDoesNotCostTheFindingsAlreadyKept(t *testing.T) {
	const empty = 700000
	named := []string{
		"Leverandøren skal ha ISO 9001 sertifisering.", "Leverandøren skal ha ISO 14001 sertifisering.",
		"Leverandøren skal være registrert i StartBANK.", "Leverandøren skal være registrert i Achilles.",
		"Leverandøren skal levere ESPD.", "Leverandøren skal levere skatteattest.",
	}
	reader := &scriptReader{fn: func(context.Context, []byte) (docread.Content, error) {
		var c docread.Content
		c.Kind, c.Extractor = docread.KindText, "text"
		for i := 0; i < 60; i++ { // enough for the list of what is kept to be long
			for _, text := range named {
				c.Units = append(c.Units, docread.Unit{Paragraph: len(c.Units) + 1, Text: text})
			}
		}
		for i := 0; i < empty; i++ {
			c.Units = append(c.Units, docread.Unit{Paragraph: len(c.Units) + 1, Text: "x"})
		}
		c.Chars = len(c.Units)
		return c, nil
	}}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	start := time.Now()
	if _, _, err := f.upload(t, zipOf(t, file("stor.txt", "x"))); err != nil {
		t.Fatal(err)
	}
	if took := time.Since(start); took > 5*time.Second {
		t.Errorf("a file of %d passages with nothing in them, after a few that name a certificate, took %s", empty, took)
	}
	in := f.store.creates[0]
	if len(in.Findings) != len(named)*perCapability || in.FindingsDropped != len(named)*(60-perCapability) {
		t.Errorf("kept %d findings and dropped %d, want %d and %d", len(in.Findings), in.FindingsDropped, len(named)*perCapability, len(named)*(60-perCapability))
	}
}

// What the store refuses that this package built is a fault in this package, not in the request: the
// caller is not told it was theirs, so the chain of the store's error stops here.
func TestAStoreThatRefusesWhatWasBuiltFromValidPartsIsNotTheCallersFault(t *testing.T) {
	f := newFixture(t)
	f.store.failWith["Create"] = docstore.ErrInvalid
	_, _, err := f.upload(t, zipOf(t, file("a.txt", "x")))
	if err == nil {
		t.Fatal("a refused upload was reported as stored")
	}
	if errors.Is(err, docstore.ErrInvalid) {
		t.Errorf("err = %v: the store's ErrInvalid is passed on as if the caller's input were invalid", err)
	}
	if !strings.Contains(err.Error(), "bug") {
		t.Errorf("err = %v: the message does not say that it is a fault here", err)
	}
}

// The slot is for reading. What is left once an archive is read is a call to the database, and it must
// not hold a slot that another archive could be read in.
func TestTheSlotIsGivenBackBeforeTheStoreIsCalled(t *testing.T) {
	f := newFixture(t, func(o *Options) { o.MaxConcurrent = 1 })
	f.store.blockCreate = make(chan struct{})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	ended := make(chan error, 2)
	start := func(name string) {
		archive := zipOf(t, file(name, "Innhold i "+name))
		go func() {
			_, _, err := f.svc.Upload(ctx, UploadRequest{OrgID: testOrg, NoticeID: testNotice, Archive: archive})
			ended <- err
		}()
	}
	reachesTheStore := func(what string) {
		t.Helper()
		select {
		case <-f.store.blockCreate:
		case err := <-ended:
			t.Fatalf("%s ended before it reached the store: %v (a slot held across the call to the database makes the next archive busy)", what, err)
		case <-time.After(10 * time.Second):
			t.Fatalf("%s never reached the store", what)
		}
	}

	// The first has read its archive and is in the call to the database. The only slot there is must be
	// free, so that the second archive can be read and is not told the service is busy.
	start("a.txt")
	reachesTheStore("the first upload")
	if n := len(f.svc.slots); n != 0 {
		t.Errorf("%d slots are held while the store is called", n)
	}
	start("b.txt")
	reachesTheStore("the second upload")

	cancel()
	for i := 0; i < 2; i++ {
		select {
		case err := <-ended:
			if !errors.Is(err, context.Canceled) {
				t.Errorf("err = %v, want the caller's cancel", err)
			}
		case <-time.After(10 * time.Second):
			t.Fatal("an upload did not end when its context did")
		}
	}
}

// A call to the database that hangs must not hold a request, and with it a slot, for ever: each call has
// its own deadline.
func TestEveryCallToTheDatabaseHasItsOwnDeadline(t *testing.T) {
	ctx := context.Background()
	archive := func(t *testing.T) []byte { return zipOf(t, file("a.txt", "Leverandøren skal ha ISO 9001.")) }
	upload := func(f *fixture, t *testing.T) error {
		_, _, err := f.upload(t, archive(t))
		return err
	}
	for name, tc := range map[string]struct {
		hang func(*fixture)
		call func(*fixture, *testing.T) error
	}{
		"HasNotice": {func(f *fixture) { f.notices.hang = true }, upload},
		"FindSame":  {func(f *fixture) { f.store.hang["FindSame"] = true }, upload},
		"Counts":    {func(f *fixture) { f.store.hang["Counts"] = true }, upload},
		"Create":    {func(f *fixture) { f.store.hang["Create"] = true }, upload},
		"EvidenceFor": {func(f *fixture) { f.store.hang["EvidenceFor"] = true }, func(f *fixture, _ *testing.T) error {
			_, err := f.svc.Evidence(ctx, testOrg, []string{testNotice})
			return err
		}},
		"List":         {func(f *fixture) { f.store.hang["List"] = true }, func(f *fixture, _ *testing.T) error { _, err := f.svc.List(ctx, testOrg, testNotice); return err }},
		"Delete":       {func(f *fixture) { f.store.hang["Delete"] = true }, func(f *fixture, _ *testing.T) error { return f.svc.Delete(ctx, testOrg, testNotice, 1) }},
		"PurgeExpired": {func(f *fixture) { f.store.hang["PurgeExpired"] = true }, func(f *fixture, _ *testing.T) error { _, err := f.svc.PurgeExpired(ctx); return err }},
	} {
		t.Run(name, func(t *testing.T) {
			f := newFixture(t, func(o *Options) { o.StoreTimeout = 40 * time.Millisecond })
			tc.hang(f)
			err := within(t, 10*time.Second, name, func() error { return tc.call(f, t) })
			if !errors.Is(err, context.DeadlineExceeded) {
				t.Errorf("err = %v, want the deadline of the call", err)
			}
			if n := len(f.svc.slots); n != 0 {
				t.Errorf("%d slots are held after a call to the database timed out", n)
			}
		})
	}
}

// The deadline is the call's and not the upload's: an upload of four calls, each well within the
// deadline and all of them together over it, is not cut off.
func TestTheDeadlineOfTheDatabaseIsPerCallAndNotForTheWholeUpload(t *testing.T) {
	f := newFixture(t, func(o *Options) { o.StoreTimeout = 250 * time.Millisecond })
	f.notices.delay = 100 * time.Millisecond
	f.store.delay = 100 * time.Millisecond // FindSame, Counts and Create: four calls, 400 ms in all
	if _, _, err := f.upload(t, zipOf(t, file("a.txt", "Leverandøren skal ha ISO 9001."))); err != nil {
		t.Fatalf("an upload whose calls are each within the deadline: %v", err)
	}
	for _, m := range []string{"FindSame", "Counts", "Create"} {
		if f.store.callCount(m) != 1 {
			t.Errorf("%s was called %d times, want 1", m, f.store.callCount(m))
		}
	}
}

// Reading the evidence of many notices is several calls, and each has the deadline of one.
func TestTheDeadlineOfTheDatabaseIsPerBatchOfEvidence(t *testing.T) {
	f := newFixture(t, func(o *Options) { o.StoreTimeout = 250 * time.Millisecond })
	f.store.delay = 100 * time.Millisecond
	var ids []string
	for i := 0; i < 4*docstore.MaxEvidenceNotices; i++ {
		ids = append(ids, fmt.Sprintf("2026-%06d", 100000+i))
	}
	if _, err := f.svc.Evidence(context.Background(), testOrg, ids); err != nil {
		t.Fatalf("four batches of 100 ms each, with a deadline of 250 ms for each call: %v", err)
	}
	if got := len(f.store.evidenceN); got != 4 {
		t.Errorf("%d calls to the store, want 4", got)
	}
}
