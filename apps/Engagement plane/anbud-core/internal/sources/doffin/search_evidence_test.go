package doffin

import (
	"context"
	"errors"
	"net/http"
	"testing"
	"time"
)

// A search page is evidence: its content type and fetch time travel with the
// bytes so a stored page can be replayed and dated without asking Doffin again.

func TestSearchReturnsContentTypeAndFetchedAt(t *testing.T) {
	t.Parallel()

	const body = `{"numHitsTotal":0,"hits":[]}`
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json; charset=utf-8")
		_, _ = w.Write([]byte(body))
	}))
	// A local time in a zone ahead of UTC: FetchedAt must be the same instant in UTC.
	local := time.Date(2026, 10, 9, 14, 30, 0, 0, time.FixedZone("CEST", 2*60*60))
	c.now = func() time.Time { return local }

	resp, err := c.Search(context.Background(), SearchParams{})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if resp.ContentType != "application/json; charset=utf-8" {
		t.Errorf("ContentType = %q, want the response's Content-Type header", resp.ContentType)
	}
	if want := local.UTC(); !resp.FetchedAt.Equal(want) || resp.FetchedAt.Location() != time.UTC {
		t.Errorf("FetchedAt = %v (location %s), want %v in UTC", resp.FetchedAt, resp.FetchedAt.Location(), want)
	}
}

func TestDecodeErrorKeepsRawAndContentType(t *testing.T) {
	t.Parallel()

	const body = "<html>maintenance</html>"
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		_, _ = w.Write([]byte(body))
	}))
	fixed := time.Date(2026, 10, 9, 12, 0, 0, 0, time.UTC)
	c.now = func() time.Time { return fixed }

	resp, err := c.Search(context.Background(), SearchParams{})
	var decodeErr *DecodeError
	if !errors.As(err, &decodeErr) {
		t.Fatalf("error = %v, want *DecodeError", err)
	}
	if string(resp.Raw) != body {
		t.Errorf("Raw = %q, want the exact body", resp.Raw)
	}
	if resp.ContentType != "text/html; charset=utf-8" {
		t.Errorf("ContentType = %q, want the response's Content-Type header", resp.ContentType)
	}
	if !resp.FetchedAt.Equal(fixed) {
		t.Errorf("FetchedAt = %v, want %v", resp.FetchedAt, fixed)
	}
}
