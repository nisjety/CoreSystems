package tenderdocs

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/docread"
	"github.com/triodelab/anbud/internal/docstore"
)

// reasonOf is the reason of an ArchiveError, or "".
func reasonOf(err error) string {
	var ae *ArchiveError
	if errors.As(err, &ae) {
		return ae.Reason
	}
	return ""
}

// Whatever refuses an upload, nothing is read into the store and nothing of the archive is kept.
func assertNothingStored(t *testing.T, f *fixture) {
	t.Helper()
	if n := f.store.callCount("Create"); n != 0 {
		t.Errorf("Create was called %d times", n)
	}
}

func TestUploadRefusesWhatIsNotAZipArchive(t *testing.T) {
	f := newFixture(t)
	for name, body := range map[string][]byte{
		"text":     []byte("this is not an archive"),
		"a pdf":    miniPDF("x"),
		"a header": []byte("PK\x03\x04 and then nothing that makes a directory"),
		"one byte": []byte("P"),
		"zeroes":   make([]byte, 4096),
	} {
		_, _, err := f.upload(t, body)
		if reasonOf(err) != ReasonNotAZip {
			t.Errorf("%s: err = %v, want %s", name, err, ReasonNotAZip)
		}
	}
	assertNothingStored(t, f)
}

func TestUploadRefusesAnEmptyBody(t *testing.T) {
	f := newFixture(t)
	if _, _, err := f.upload(t, nil); reasonOf(err) != ReasonEmpty {
		t.Errorf("err = %v, want %s", err, ReasonEmpty)
	}
	assertNothingStored(t, f)
}

func TestUploadRefusesAnArchiveOverTheSizeLimitBeforeOpeningIt(t *testing.T) {
	reader := &scriptReader{fn: readsAs(1)}
	f := newFixture(t, func(o *Options) { o.Reader = reader; o.Limits.MaxArchiveBytes = 60 })
	archive := zipOf(t, file("a.txt", strings.Repeat("abcdefghij", 100)))
	if int64(len(archive)) <= 60 {
		t.Fatalf("the archive is only %d bytes: the test needs one over 60", len(archive))
	}
	if _, _, err := f.upload(t, archive); reasonOf(err) != ReasonTooLarge {
		t.Errorf("err = %v, want %s", err, ReasonTooLarge)
	}
	if reader.count() != 0 || f.notices.calls != 0 || f.store.callCount("FindSame") != 0 {
		t.Errorf("an archive that is too large cost work: %d reads, %d notice lookups, %d store calls", reader.count(), f.notices.calls, f.store.callCount("FindSame"))
	}
}

// The limit is on the archive as it arrives, and it is exact: the limit itself is allowed, and one byte
// more is not.
func TestTheArchiveLimitIsExact(t *testing.T) {
	archive := zipOf(t, file("a.txt", "Leverandøren skal ha ISO 9001."))
	size := int64(len(archive))

	f := newFixture(t, func(o *Options) { o.Limits.MaxArchiveBytes = size })
	if _, _, err := f.upload(t, archive); err != nil {
		t.Errorf("an archive of exactly the limit: %v", err)
	}
	f = newFixture(t, func(o *Options) { o.Limits.MaxArchiveBytes = size - 1 })
	if _, _, err := f.upload(t, archive); reasonOf(err) != ReasonTooLarge {
		t.Errorf("an archive one byte over the limit: err = %v, want %s", err, ReasonTooLarge)
	}
	// Refused before anything is asked of the database: the archive reader would refuse it too, later.
	if f.notices.calls != 0 || f.store.callCount("FindSame") != 0 {
		t.Errorf("an archive one byte over the limit cost %d notice lookups and %d store calls", f.notices.calls, f.store.callCount("FindSame"))
	}
	if got := f.svc.MaxArchiveBytes(); got != size-1 {
		t.Errorf("MaxArchiveBytes = %d, want %d", got, size-1)
	}
}

func TestUploadRefusesAnArchiveWithTooManyEntries(t *testing.T) {
	f := newFixture(t, func(o *Options) { o.Limits.MaxEntries = 3 })
	archive := zipOf(t, file("a.txt", "1"), file("b.txt", "2"), file("c.txt", "3"), file("d.txt", "4"))
	if _, _, err := f.upload(t, archive); reasonOf(err) != ReasonTooManyEntries {
		t.Errorf("err = %v, want %s", err, ReasonTooManyEntries)
	}
	assertNothingStored(t, f)
}

// The entries of every directory are counted across the archive and the archives inside it: five archives of ten
// files in an archive of ten entries are sixty entries, and four times ten is what a walk may visit.
func TestUploadRefusesAnArchiveWhoseNestedArchivesHoldTooManyEntriesInAll(t *testing.T) {
	inner := func(prefix string) string {
		var files []entry
		for i := 0; i < 10; i++ {
			files = append(files, file(prefix+itoa(i)+".txt", "innhold "+prefix+itoa(i)))
		}
		return string(zipOf(t, files...))
	}
	var top []entry
	for i := 0; i < 5; i++ {
		top = append(top, file("n"+itoa(i)+".zip", inner("n"+itoa(i)+"-")))
	}
	for i := 5; i < 10; i++ {
		top = append(top, file("f"+itoa(i)+".txt", "x"))
	}
	f := newFixture(t, func(o *Options) { o.Limits.MaxEntries = 10 })
	if _, _, err := f.upload(t, zipOf(t, top...)); reasonOf(err) != ReasonTooManyEntries {
		t.Errorf("err = %v, want %s", err, ReasonTooManyEntries)
	}
	assertNothingStored(t, f)
	// Three archives of ten files are forty entries in all, which is allowed.
	f = newFixture(t, func(o *Options) { o.Limits.MaxEntries = 10 })
	var fewer []entry
	for i := 0; i < 3; i++ {
		fewer = append(fewer, file("n"+itoa(i)+".zip", inner("n"+itoa(i)+"-")))
	}
	for i := 3; i < 10; i++ {
		fewer = append(fewer, file("f"+itoa(i)+".txt", "x"))
	}
	if _, _, err := f.upload(t, zipOf(t, fewer...)); err != nil {
		t.Errorf("forty entries in all, with forty allowed: %v", err)
	}
}

func TestUploadRefusesAZipBomb(t *testing.T) {
	f := newFixture(t)
	archive := zipOf(t, entry{name: "stor.bin", data: make([]byte, 3<<20)}) // three megabytes of zeroes: a few kilobytes of archive
	if int64(len(archive)) > 64<<10 {
		t.Fatalf("the archive is %d bytes: the test needs a small one that expands a lot", len(archive))
	}
	if _, _, err := f.upload(t, archive); reasonOf(err) != ReasonExpandsTooFar {
		t.Errorf("err = %v, want %s", err, ReasonExpandsTooFar)
	}
	assertNothingStored(t, f)
}

func TestUploadRefusesAnArchiveWithMoreFilesThanAManifestHolds(t *testing.T) {
	reader := &scriptReader{fn: readsAs(1)}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	var files []entry
	for i := 0; i < docstore.MaxDocuments+1; i++ {
		files = append(files, file("f"+itoa(i)+".txt", "innhold "+itoa(i)))
	}
	if _, _, err := f.upload(t, zipOf(t, files...)); reasonOf(err) != ReasonTooManyFiles {
		t.Errorf("err = %v, want %s", err, ReasonTooManyFiles)
	}
	assertNothingStored(t, f)
	if reader.count() > docstore.MaxDocuments {
		t.Errorf("read %d files of an archive that was going to be refused: it stops at the first one over", reader.count())
	}
}

// Files that were skipped are in the manifest too, and the manifest has room for only so many.
func TestUploadCountsTheSkippedFilesAgainstTheManifest(t *testing.T) {
	f := newFixture(t)
	var files []entry
	for i := 0; i < docstore.MaxDocuments; i++ {
		files = append(files, file("f"+itoa(i)+".txt", "innhold "+itoa(i)))
	}
	files = append(files, file("../utenfor.txt", "x")) // skipped, and listed: one more than the manifest holds
	if _, _, err := f.upload(t, zipOf(t, files...)); reasonOf(err) != ReasonTooManyFiles {
		t.Errorf("err = %v, want %s", err, ReasonTooManyFiles)
	}
	assertNothingStored(t, f)

	// Exactly as many as it holds is allowed.
	f = newFixture(t)
	if _, _, err := f.upload(t, zipOf(t, files[:docstore.MaxDocuments]...)); err != nil {
		t.Errorf("a manifest of exactly %d files: %v", docstore.MaxDocuments, err)
	}
}

func itoa(n int) string {
	const digits = "0123456789"
	if n == 0 {
		return "0"
	}
	var b []byte
	for ; n > 0; n /= 10 {
		b = append([]byte{digits[n%10]}, b...)
	}
	return string(b)
}

func TestUploadRefusesAZDROrganisationBeforeAnyWork(t *testing.T) {
	reader := &scriptReader{fn: readsAs(1)}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	_, _, err := f.svc.Upload(context.Background(), UploadRequest{
		OrgID: testOrg, NoticeID: testNotice, ZDR: true, Archive: zipOf(t, file("a.txt", "x")),
	})
	if !errors.Is(err, ErrZDR) {
		t.Fatalf("err = %v, want ErrZDR", err)
	}
	if reader.count() != 0 || f.notices.calls != 0 || len(f.store.calls) != 0 {
		t.Errorf("a ZDR upload cost work: %d reads, %d notice lookups, store calls %v", reader.count(), f.notices.calls, f.store.calls)
	}
}

func TestUploadRefusesANoticeIdThatIsNotOne(t *testing.T) {
	f := newFixture(t)
	for _, id := range []string{"", "x", "2026-1", "../x", "2026-100001/../x", "2026-100001 "} {
		_, _, err := f.svc.Upload(context.Background(), UploadRequest{OrgID: testOrg, NoticeID: id, Archive: zipOf(t, file("a.txt", "x"))})
		if !errors.Is(err, ErrInvalidNotice) {
			t.Errorf("%q: err = %v, want ErrInvalidNotice", id, err)
		}
	}
	if f.notices.calls != 0 {
		t.Errorf("the notice store was asked about an id that is not one")
	}
}

func TestUploadRefusesANoticeThatIsNotHeld(t *testing.T) {
	reader := &scriptReader{fn: readsAs(1)}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	_, _, err := f.svc.Upload(context.Background(), UploadRequest{OrgID: testOrg, NoticeID: "2026-999999", Archive: zipOf(t, file("a.txt", "x"))})
	if !errors.Is(err, ErrNoticeNotFound) {
		t.Errorf("err = %v, want ErrNoticeNotFound", err)
	}
	if reader.count() != 0 {
		t.Errorf("an archive for a notice that is not held was read")
	}
	assertNothingStored(t, f)
}

func TestUploadPassesOnAFailureOfTheNoticeStore(t *testing.T) {
	f := newFixture(t)
	f.notices.err = errors.New("the database is down")
	if _, _, err := f.upload(t, zipOf(t, file("a.txt", "x"))); err == nil || errors.Is(err, ErrNoticeNotFound) {
		t.Errorf("err = %v: a failed lookup is not 'not found'", err)
	}
}

func TestUploadRefusesAnOrganisationThatIsNotOne(t *testing.T) {
	// The store knows what an organisation id looks like and says so, and nothing has been read by then.
	reader := &scriptReader{fn: readsAs(1)}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	f.store.failWith["FindSame"] = docstore.ErrInvalid
	_, _, err := f.svc.Upload(context.Background(), UploadRequest{OrgID: "bad org", NoticeID: testNotice, Archive: zipOf(t, file("a.txt", "x"))})
	if !errors.Is(err, docstore.ErrInvalid) {
		t.Errorf("err = %v, want docstore.ErrInvalid", err)
	}
	if reader.count() != 0 {
		t.Errorf("an archive was read for an organisation that is not one")
	}
}

func TestUploadStopsAtTheQuota(t *testing.T) {
	for name, tc := range map[string]struct {
		forNotice, forOrg int
		scope             string
	}{
		"per notice":       {10, 10, "notice"},
		"per organisation": {1, 200, "organisation"},
	} {
		reader := &scriptReader{fn: readsAs(1)}
		f := newFixture(t, func(o *Options) { o.Reader = reader })
		f.store.heldNotice, f.store.heldOrg = tc.forNotice, tc.forOrg
		_, _, err := f.upload(t, zipOf(t, file("a.txt", "x")))
		var q *QuotaError
		if !errors.As(err, &q) || q.Scope != tc.scope {
			t.Errorf("%s: err = %v, want a quota error for the %s", name, err, tc.scope)
		}
		if reader.count() != 0 {
			t.Errorf("%s: an archive was read over the quota", name)
		}
		assertNothingStored(t, f)
	}
	// One below the limit is allowed.
	f := newFixture(t)
	f.store.heldNotice, f.store.heldOrg = 9, 199
	if _, _, err := f.upload(t, zipOf(t, file("a.txt", "Leverandøren skal ha ISO 9001."))); err != nil {
		t.Errorf("one below the quota: %v", err)
	}
}

// What the reader does wrong is a fact about one file, not about the archive: the file is listed with
// what happened to it and the rest is read.
func TestUploadMapsWhatTheReaderSaysToTheStatusOfTheFile(t *testing.T) {
	reader := &scriptReader{}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	reader.fn = func(_ context.Context, data []byte) (docread.Content, error) {
		switch string(data) {
		case "encrypted":
			return docread.Content{Kind: docread.KindPDF}, docread.ErrEncrypted
		case "toolarge":
			return docread.Content{Kind: docread.KindPDF}, docread.ErrTooLarge
		case "timeout":
			return docread.Content{Kind: docread.KindPDF}, docread.ErrTimeout
		case "crash":
			return docread.Content{}, docread.ErrFailed
		case "damaged":
			return docread.Content{}, docread.ErrMalformed
		case "unsupported":
			return docread.Content{Kind: docread.KindXLSX}, docread.ErrUnsupported
		case "panic":
			panic("a reader that panics, with a value that must not be logged: HEMMELIG-PANIKK")
		case "other":
			return docread.Content{}, errors.New("something else")
		}
		return readsAs(2)(context.Background(), data)
	}
	var files []entry
	for _, name := range []string{"encrypted", "toolarge", "timeout", "crash", "damaged", "unsupported", "panic", "other", "fine"} {
		files = append(files, file(name+".bin", name))
	}
	if _, _, err := f.upload(t, zipOf(t, files...)); err != nil {
		t.Fatalf("Upload: %v", err)
	}
	want := map[string]struct{ status, reason string }{
		"encrypted.bin":   {docstore.StatusEncrypted, ""},
		"toolarge.bin":    {docstore.StatusTooLarge, ""},
		"timeout.bin":     {docstore.StatusFailed, "timeout"},
		"crash.bin":       {docstore.StatusFailed, "reader_error"},
		"damaged.bin":     {docstore.StatusFailed, "damaged"},
		"unsupported.bin": {docstore.StatusUnsupported, ""},
		"panic.bin":       {docstore.StatusFailed, "reader_error"},
		"other.bin":       {docstore.StatusFailed, "reader_error"},
		"fine.bin":        {docstore.StatusRead, ""},
	}
	docs := byPath(f.store.creates[0].Documents)
	for path, w := range want {
		if d := docs[path]; d.Status != w.status || d.Reason != w.reason {
			t.Errorf("%s = status %q reason %q, want %q %q", path, d.Status, d.Reason, w.status, w.reason)
		}
	}
	if docs["unsupported.bin"].Kind != "xlsx" {
		t.Errorf("the kind of an unsupported file is %q, want what the reader said it is", docs["unsupported.bin"].Kind)
	}
	if strings.Contains(f.logs.String(), "HEMMELIG-PANIKK") {
		t.Errorf("the value of a panic was logged")
	}
	if !strings.Contains(f.logs.String(), "panicked") {
		t.Errorf("a panic of a reader leaves no trace in the log")
	}
}

func TestUploadBusyWhenAllTheSlotsAreTaken(t *testing.T) {
	release := make(chan struct{})
	started := make(chan struct{}, 1)
	reader := &scriptReader{}
	reader.fn = func(ctx context.Context, data []byte) (docread.Content, error) {
		started <- struct{}{}
		select {
		case <-release:
		case <-ctx.Done():
		}
		return readsAs(1)(ctx, data)
	}
	f := newFixture(t, func(o *Options) { o.Reader = reader; o.MaxConcurrent = 1 })

	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		if _, _, err := f.upload(t, zipOf(t, file("a.txt", "första"))); err != nil {
			t.Errorf("the first upload: %v", err)
		}
	}()
	select {
	case <-started:
	case <-time.After(10 * time.Second):
		t.Fatal("the first upload never started reading")
	}

	// The second upload must be told at once. It runs on its own goroutine so that a service that waits
	// for the slot instead fails this test, and does not hang it.
	second := make(chan error, 1)
	go func() {
		_, _, err := f.upload(t, zipOf(t, file("b.txt", "andre")))
		second <- err
	}()
	answered := false
	select {
	case err := <-second:
		answered = true
		if !errors.Is(err, ErrBusy) {
			t.Errorf("the second upload: err = %v, want ErrBusy", err)
		}
	case <-time.After(10 * time.Second):
		t.Error("the second upload waited for a slot instead of saying the service is busy")
	}
	close(release)
	wg.Wait()
	if !answered {
		<-second // it ends too, now that the slot is free
	}

	// The slot is free again.
	if _, _, err := f.upload(t, zipOf(t, file("c.txt", "tredje"))); err != nil {
		t.Errorf("after the first finished: %v", err)
	}
}

func TestUploadTimesOutAndStoresNothing(t *testing.T) {
	reader := &scriptReader{fn: func(ctx context.Context, _ []byte) (docread.Content, error) {
		<-ctx.Done()
		return docread.Content{}, docread.ErrTimeout
	}}
	f := newFixture(t, func(o *Options) { o.Reader = reader; o.Timeout = 50 * time.Millisecond })
	_, _, err := f.upload(t, zipOf(t, file("a.txt", "x")))
	if !errors.Is(err, ErrTimeout) {
		t.Errorf("err = %v, want ErrTimeout", err)
	}
	assertNothingStored(t, f)
}

// The caller's own cancel is the caller's: it is not a timeout, and it stores nothing either.
func TestUploadStopsWhenTheCallerGivesUp(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	reader := &scriptReader{fn: func(ctx context.Context, _ []byte) (docread.Content, error) {
		cancel()
		<-ctx.Done()
		return docread.Content{}, ctx.Err()
	}}
	f := newFixture(t, func(o *Options) { o.Reader = reader })
	_, _, err := f.svc.Upload(ctx, UploadRequest{OrgID: testOrg, NoticeID: testNotice, Archive: zipOf(t, file("a.txt", "x"))})
	if !errors.Is(err, context.Canceled) || errors.Is(err, ErrTimeout) {
		t.Errorf("err = %v, want the caller's cancel and not a timeout", err)
	}
	assertNothingStored(t, f)
}

// A deadline of the caller's own is the caller's, as a cancel is: it is not the archive taking too long.
func TestUploadReportsTheCallersOwnDeadlineAsTheirs(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	reader := &scriptReader{fn: func(ctx context.Context, _ []byte) (docread.Content, error) {
		<-ctx.Done()
		return docread.Content{}, docread.ErrTimeout
	}}
	f := newFixture(t, func(o *Options) { o.Reader = reader; o.Timeout = time.Minute })
	_, _, err := f.svc.Upload(ctx, UploadRequest{OrgID: testOrg, NoticeID: testNotice, Archive: zipOf(t, file("a.txt", "x"))})
	if !errors.Is(err, context.DeadlineExceeded) || errors.Is(err, ErrTimeout) {
		t.Errorf("err = %v, want the caller's deadline and not ErrTimeout", err)
	}
	assertNothingStored(t, f)
}

func TestUploadPassesOnAFailureOfTheStore(t *testing.T) {
	f := newFixture(t)
	f.store.failWith["Create"] = errors.New("the database is down")
	if _, _, err := f.upload(t, zipOf(t, file("a.txt", "x"))); err == nil || !strings.Contains(err.Error(), "the database is down") {
		t.Errorf("err = %v, want the store's failure", err)
	}
}

func TestNewRefusesWhatCannotWork(t *testing.T) {
	good := func() Options {
		return Options{Store: newFakeStore(), Notices: &fakeNotices{}, Reader: &scriptReader{}, Limits: testLimits(), Retention: time.Hour}
	}
	for name, mutate := range map[string]func(*Options){
		"no store":                  func(o *Options) { o.Store = nil },
		"no notices":                func(o *Options) { o.Notices = nil },
		"no reader":                 func(o *Options) { o.Reader = nil },
		"no retention":              func(o *Options) { o.Retention = 0 },
		"negative retention":        func(o *Options) { o.Retention = -time.Hour },
		"retention over the max":    func(o *Options) { o.Retention = docstore.MaxRetention + time.Second },
		"retention under a second":  func(o *Options) { o.Retention = docstore.MinRetention - time.Nanosecond },
		"retention of a nanosecond": func(o *Options) { o.Retention = time.Nanosecond },
		"an archive of no size":     func(o *Options) { o.Limits.MaxArchiveBytes = 0 },
	} {
		o := good()
		mutate(&o)
		if _, err := New(o); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
	if _, err := New(good()); err != nil {
		t.Errorf("a good set of options: %v", err)
	}
	for _, retention := range []time.Duration{docstore.MinRetention, docstore.MaxRetention} {
		o := good()
		o.Retention = retention
		if _, err := New(o); err != nil {
			t.Errorf("a retention of %s, which is within what the store holds: %v", retention, err)
		}
	}
}

// What the docs say a service does when it is not told otherwise.
func TestTheDefaultsAreTheOnesTheDocsGive(t *testing.T) {
	svc, err := New(Options{Store: newFakeStore(), Notices: &fakeNotices{}, Reader: &scriptReader{}, Retention: DefaultRetention})
	if err != nil {
		t.Fatal(err)
	}
	if DefaultRetention != 90*24*time.Hour || DefaultMaxArchiveBytes != 100<<20 {
		t.Errorf("retention %s and archive %d bytes, want 90 days and 100 MiB", DefaultRetention, DefaultMaxArchiveBytes)
	}
	if svc.timeout != 3*time.Minute || svc.storeWait != 30*time.Second {
		t.Errorf("an archive has %s and a call to the database %s, want 3 minutes and 30 seconds", svc.timeout, svc.storeWait)
	}
	if svc.perNotice != 10 || svc.perOrg != 200 || cap(svc.slots) != 2 {
		t.Errorf("%d uploads for a notice, %d for an organisation and %d slots, want 10, 200 and 2", svc.perNotice, svc.perOrg, cap(svc.slots))
	}
	lim := svc.limits
	if lim.MaxArchiveBytes != DefaultMaxArchiveBytes || lim.MaxEntries != 2000 || lim.MaxTotalEntries != 8000 || lim.MaxRatio != 200 || lim.MaxDepth != 3 || lim.MaxTotalBytes != 1<<30 {
		t.Errorf("limits = %+v", lim)
	}
	if lim.MaxEntryBytes != docread.DefaultLimits.MaxPDFBytes {
		t.Errorf("a file may be %d bytes, want what a PDF may be (%d)", lim.MaxEntryBytes, docread.DefaultLimits.MaxPDFBytes)
	}
	if got := svc.MaxArchiveBytes(); got != DefaultMaxArchiveBytes {
		t.Errorf("MaxArchiveBytes = %d", got)
	}
}
