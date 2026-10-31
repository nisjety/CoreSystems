package rawstore

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// fakeSearcher answers each day's first page from a table and counts its calls.
// The engine asks for one page per day in these tests, so every answer is complete.
type fakeSearcher struct {
	days  map[string][]string // day label to the ids that day holds
	calls int
}

func (f *fakeSearcher) Search(_ context.Context, p doffin.SearchParams) (doffin.SearchResponse, error) {
	f.calls++
	label := p.IssueDateFrom.Format("2006-01-02")
	ids, ok := f.days[label]
	if !ok {
		return doffin.SearchResponse{}, fmt.Errorf("fake search: no fixture for %s", label)
	}
	return buildSearch(ids)
}

// buildSearch is the one-page response for ids, with the raw bytes the client keeps.
func buildSearch(ids []string) (doffin.SearchResponse, error) {
	type rawHit struct {
		ID string `json:"id"`
	}
	raw := struct {
		NumHitsTotal      int      `json:"numHitsTotal"`
		NumHitsAccessible int      `json:"numHitsAccessible"`
		Hits              []rawHit `json:"hits"`
	}{NumHitsTotal: len(ids), NumHitsAccessible: len(ids)}
	hits := make(doffin.OneOrMany[doffin.Hit], 0, len(ids))
	for _, id := range ids {
		raw.Hits = append(raw.Hits, rawHit{ID: id})
		hits = append(hits, doffin.Hit{ID: id})
	}
	body, err := json.Marshal(raw)
	if err != nil {
		return doffin.SearchResponse{}, fmt.Errorf("fake search: marshal: %w", err)
	}
	return doffin.SearchResponse{
		NumHitsTotal:      doffin.Count(len(ids)),
		NumHitsAccessible: doffin.Count(len(ids)),
		Hits:              hits,
		Raw:               body,
		ContentType:       "application/json",
		FetchedAt:         fixedNow,
	}, nil
}

// fakeDownloader records every id it is asked for. respond decides the outcome;
// nil means the notice comes back intact.
type fakeDownloader struct {
	calls   []string
	respond func(ctx context.Context, id string) (doffin.Notice, error)
}

func (f *fakeDownloader) Download(ctx context.Context, id string) (doffin.Notice, error) {
	f.calls = append(f.calls, id)
	if f.respond == nil {
		return noticeFor(id), nil
	}
	return f.respond(ctx, id)
}

// noticeFor is the notice the upstream returns for id: a body derived from the id.
func noticeFor(id string) doffin.Notice {
	return guardNotice(id, []byte("notice body for "+id))
}

// newSyncEngine builds the real engine over the real store, with a fixed clock.
func newSyncEngine(t *testing.T, s *Store, search doffinsync.Searcher, download doffinsync.Downloader) *doffinsync.Engine {
	t.Helper()
	e, err := doffinsync.New(
		doffinsync.Deps{Search: search, Download: download, Pages: s, Notices: s, Runs: s, Days: s},
		doffinsync.Options{Mode: doffinsync.ModeSync, Now: func() time.Time { return fixedNow }},
	)
	if err != nil {
		t.Fatalf("doffinsync.New: %v", err)
	}
	return e
}

// window is the inclusive window [from, to] as the engine takes it.
func window(t *testing.T, from, to string) doffinsync.Window {
	t.Helper()
	return doffinsync.Window{From: utcDay(t, from), To: utcDay(t, to)}
}
