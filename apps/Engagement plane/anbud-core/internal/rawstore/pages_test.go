package rawstore

import (
	"bytes"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
)

const testQueryKey = "issueDate=2026-10-01;perPage=100;sort=PUBLICATION_DATE_ASC"

// T55: a search response that does not decode is still evidence. Its bytes are
// stored exactly, and a second put of the same page adds no row.
func TestSearchPageStoresUndecodableBytes(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	body := []byte("<html>not json\x00\xff\r\n")
	page := doffinsync.SearchPage{
		QueryKey:    testQueryKey,
		Page:        1,
		ContentType: "text/html",
		Body:        body,
		FetchedAt:   fixedNow,
	}
	if err := s.PutSearchPage(ctx, run, page); err != nil {
		t.Fatalf("first PutSearchPage: %v", err)
	}
	if err := s.PutSearchPage(ctx, run, page); err != nil {
		t.Fatalf("second PutSearchPage: %v", err)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_search_pages`); got != 1 {
		t.Fatalf("search page rows = %d after two identical puts, want 1", got)
	}

	var stored []byte
	var contentType string
	if err := pool.QueryRow(ctx, `SELECT body, content_type FROM raw_search_pages`).Scan(&stored, &contentType); err != nil {
		t.Fatalf("read back: %v", err)
	}
	if !bytes.Equal(stored, body) {
		t.Fatalf("stored page differs from the response bytes: %q", stored)
	}
	if contentType != "text/html" {
		t.Fatalf("content type = %q, want text/html", contentType)
	}
}

// A hostile content type on a search page is cleaned, not allowed to fail the put.
func TestSearchPageContentTypeCannotFailTheInsert(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	hostile := "text/html\x00\xfe" + strings.Repeat("y", 300)
	err := s.PutSearchPage(ctx, run, doffinsync.SearchPage{
		QueryKey:    testQueryKey,
		Page:        1,
		ContentType: hostile,
		Body:        []byte("<html>x</html>"),
		FetchedAt:   fixedNow,
	})
	if err != nil {
		t.Fatalf("PutSearchPage with a hostile content type: %v", err)
	}
	var stored string
	if err := pool.QueryRow(ctx, `SELECT content_type FROM raw_search_pages`).Scan(&stored); err != nil {
		t.Fatalf("read content type: %v", err)
	}
	if strings.ContainsAny(stored, "\x00") || len([]rune(stored)) > 255 || !strings.HasPrefix(stored, "text/html") {
		t.Fatalf("stored content type %q is not clean and clipped", stored)
	}
}
