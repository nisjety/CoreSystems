package bzp

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TestLivePageOne asks the live board for the first page of one day. It is skipped unless BZP_LIVE names
// the day (YYYY-MM-DD), because it makes a real request.
func TestLivePageOne(t *testing.T) {
	day := os.Getenv("BZP_LIVE")
	if day == "" {
		t.Skip("set BZP_LIVE to a day, YYYY-MM-DD, to ask the live board for its first page")
	}
	start, err := parseDay(day)
	if err != nil {
		t.Fatal(err)
	}
	c, err := New(Config{})
	if err != nil {
		t.Fatal(err)
	}
	page := 1
	resp, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: start, IssueDateTo: start, NumHitsPerPage: 10, Page: &page})
	if err != nil {
		t.Fatalf("live Search: %v", err)
	}
	t.Logf("first page: total %d, hits %d", resp.NumHitsTotal, len(resp.Hits))
}

func parseDay(s string) (time.Time, error) { return time.Parse("2006-01-02", s) }
