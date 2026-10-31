package rawstore

import (
	"context"
	"errors"
	"testing"
)

// The notice read side, against a real database: the API parses what it reads here on every
// request, so these tests pin which body it gets, in what order, and that it never gets an erased one.

func putNotice(t *testing.T, s *Store, run int64, id, body string) {
	t.Helper()
	if _, err := s.StoreNotice(testCtx(t), run, guardNotice(id, []byte(body))); err != nil {
		t.Fatalf("StoreNotice %s: %v", id, err)
	}
}

func recentIDs(t *testing.T, s *Store, limit int) []string {
	t.Helper()
	var ids []string
	if err := s.RecentNotices(testCtx(t), limit, func(d Document) error { ids = append(ids, d.ID); return nil }); err != nil {
		t.Fatalf("RecentNotices: %v", err)
	}
	return ids
}

func sameStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func TestRecentNoticesReadsTheNewestFirstAndNoMoreThanTheLimit(t *testing.T) {
	s, _ := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	for _, id := range []string{"2026-100001", "2026-100002", "2026-100003", "2026-100004", "2026-100005"} {
		putNotice(t, s, run, id, "body of "+id)
	}
	if got, want := recentIDs(t, s, 3), []string{"2026-100005", "2026-100004", "2026-100003"}; !sameStrings(got, want) {
		t.Errorf("limit 3 = %v, want %v", got, want)
	}
	if got := recentIDs(t, s, 10); len(got) != 5 {
		t.Errorf("limit 10 = %v, want all 5", got)
	}
}

func TestRecentNoticesCarriesTheEvidenceOfEachBody(t *testing.T) {
	s, _ := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	n := guardNotice("2026-100001", []byte("body of the notice"))
	putNotice(t, s, run, n.ID, "body of the notice")

	var got []Document
	if err := s.RecentNotices(testCtx(t), 5, func(d Document) error { got = append(got, d); return nil }); err != nil {
		t.Fatalf("RecentNotices: %v", err)
	}
	if len(got) != 1 {
		t.Fatalf("documents = %d, want 1", len(got))
	}
	d := got[0]
	if d.ID != n.ID || string(d.Body) != "body of the notice" || d.SHA256 != n.SHA256 || !d.FetchedAt.Equal(n.FetchedAt) {
		t.Errorf("document = id %q body %q sha %q fetched %v; want %q %q %q %v", d.ID, d.Body, d.SHA256, d.FetchedAt, n.ID, n.Body, n.SHA256, n.FetchedAt)
	}
	if d.FetchedAt.Location().String() != "UTC" {
		t.Errorf("fetched at is in %v, want UTC", d.FetchedAt.Location())
	}
}

// A notice that changed has two bodies. The reader gets the latest one, once.
func TestRecentNoticesGivesTheLatestBodyOfAChangedNoticeOnce(t *testing.T) {
	s, _ := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	putNotice(t, s, run, "2026-100001", "version one")
	putNotice(t, s, run, "2026-100002", "another notice")
	putNotice(t, s, run, "2026-100001", "version two")

	bodies := map[string]string{}
	var order []string
	if err := s.RecentNotices(testCtx(t), 10, func(d Document) error {
		bodies[d.ID] = string(d.Body)
		order = append(order, d.ID)
		return nil
	}); err != nil {
		t.Fatalf("RecentNotices: %v", err)
	}
	if want := []string{"2026-100001", "2026-100002"}; !sameStrings(order, want) {
		t.Errorf("order = %v, want %v", order, want)
	}
	if bodies["2026-100001"] != "version two" {
		t.Errorf("body = %q, want the latest version", bodies["2026-100001"])
	}
}

// docs/PERSONAL-DATA.md section 6: a reader reads raw_documents_live, so an erased body is never
// parsed again.
func TestRecentNoticesNeverReadsAnErasedBody(t *testing.T) {
	s, pool := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	putNotice(t, s, run, "2026-100001", "kept")
	putNotice(t, s, run, "2026-100002", "erased")
	erased := guardNotice("2026-100002", []byte("erased"))
	if _, err := pool.Exec(testCtx(t), redactNoticeSQL, rawDocumentID(t, pool, erased.ID, erased.SHA256)); err != nil {
		t.Fatalf("redact: %v", err)
	}

	if got, want := recentIDs(t, s, 10), []string{"2026-100001"}; !sameStrings(got, want) {
		t.Errorf("ids = %v, want %v", got, want)
	}
	if _, err := s.Notice(testCtx(t), erased.ID); !errors.Is(err, ErrNotFound) {
		t.Errorf("Notice of an erased body: err = %v, want ErrNotFound", err)
	}
}

func TestRecentNoticesStopsWhenTheCallbackFails(t *testing.T) {
	s, _ := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	for _, id := range []string{"2026-100001", "2026-100002", "2026-100003"} {
		putNotice(t, s, run, id, "body of "+id)
	}
	stop := errors.New("stop here")
	calls := 0
	err := s.RecentNotices(testCtx(t), 10, func(Document) error {
		calls++
		if calls == 2 {
			return stop
		}
		return nil
	})
	if !errors.Is(err, stop) || calls != 2 {
		t.Errorf("err = %v after %d calls, want the callback's error after 2", err, calls)
	}
}

func TestRecentNoticesHonoursACancelledContext(t *testing.T) {
	s, _ := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	putNotice(t, s, run, "2026-100001", "body")
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := s.RecentNotices(ctx, 5, func(Document) error { return nil }); err == nil {
		t.Errorf("a cancelled context read notices")
	}
}

func TestNoticeReturnsTheLatestLiveBody(t *testing.T) {
	s, _ := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	putNotice(t, s, run, "2026-100001", "version one")
	putNotice(t, s, run, "2026-100001", "version two")
	putNotice(t, s, run, "2026-100002", "other")

	d, err := s.Notice(testCtx(t), "2026-100001")
	if err != nil {
		t.Fatalf("Notice: %v", err)
	}
	if d.ID != "2026-100001" || string(d.Body) != "version two" {
		t.Errorf("document = %q %q, want the latest version of the one asked for", d.ID, d.Body)
	}
	if _, err := s.Notice(testCtx(t), "2026-999999"); !errors.Is(err, ErrNotFound) {
		t.Errorf("a notice that is not held: err = %v, want ErrNotFound", err)
	}
}

// HasNotice answers whether a live body is held without reading it: a notice may be megabytes, and
// the question is asked once for each upload.
func TestHasNoticeSaysWhetherALiveBodyIsHeld(t *testing.T) {
	s, pool := openStore(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	putNotice(t, s, run, "2026-100001", "kept")
	putNotice(t, s, run, "2026-100002", "erased")
	erased := guardNotice("2026-100002", []byte("erased"))
	if _, err := pool.Exec(testCtx(t), redactNoticeSQL, rawDocumentID(t, pool, erased.ID, erased.SHA256)); err != nil {
		t.Fatalf("redact: %v", err)
	}

	for id, want := range map[string]bool{
		"2026-100001": true,
		"2026-100002": false, // erased: not a body any more
		"2026-999999": false, // never stored
	} {
		got, err := s.HasNotice(testCtx(t), id)
		if err != nil || got != want {
			t.Errorf("HasNotice(%s) = %v, %v; want %v", id, got, err, want)
		}
	}
}

func TestHasNoticeRefusesAnIdThatIsNotANoticeIdBeforeAskingTheDatabase(t *testing.T) {
	s := New(nil) // a nil pool panics on any query: the refusal must come first
	for _, id := range []string{"", "x", "2026-1", "../etc/passwd", "2026-100001; DROP TABLE raw_documents"} {
		if _, err := s.HasNotice(context.Background(), id); !errors.Is(err, ErrInvalidQuery) {
			t.Errorf("HasNotice(%q): err = %v, want ErrInvalidQuery", id, err)
		}
	}
}
