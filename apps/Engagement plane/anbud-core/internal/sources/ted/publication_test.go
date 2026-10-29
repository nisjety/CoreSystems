package ted

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TED's search answers each notice's publication day with the offset it was published under; the
// day is what the app shows, and the offset does not move it.
func TestTheSearchAsksForAndReadsThePublicationDay(t *testing.T) {
	var asked searchRequest
	c := fake(t, func(req searchRequest) (int, string) {
		asked = req
		return 200, `{"notices":[{"publication-number":"741144-2026","publication-date":"2026-10-27+01:00"},{"publication-number":"741190-2026","publication-date":"not a date"}],"totalNoticeCount":2}`
	}, nil)
	resp, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day, IssueDateTo: day, NumHitsPerPage: 10})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if len(asked.Fields) != 2 || asked.Fields[1] != "publication-date" {
		t.Errorf("fields asked %v, want the publication date too", asked.Fields)
	}
	if len(resp.Hits) != 2 || resp.Hits[0].PublicationDate != "2026-10-27" || resp.Hits[1].PublicationDate != "" {
		t.Errorf("hits %+v; want the first dated, and no date for an unreadable one", resp.Hits)
	}
}

func TestPublicationDatesOfAStoredPage(t *testing.T) {
	body, _ := json.Marshal(map[string]any{"notices": []map[string]string{
		{"publication-number": "741144-2026", "publication-date": "2026-10-27+01:00"},
		{"publication-number": "741144-2026", "publication-date": "2026-10-28+01:00"},
		{"publication-number": "741190-2026"},
	}})
	got := PublicationDates(body)
	if got["741144-2026"] != "2026-10-27" || len(got) != 1 {
		t.Errorf("dates %v; want the first answer for 741144-2026, and nothing for the undated one", got)
	}
	if len(PublicationDates([]byte("<html>"))) != 0 {
		t.Error("a page that is not JSON gave dates")
	}
}
