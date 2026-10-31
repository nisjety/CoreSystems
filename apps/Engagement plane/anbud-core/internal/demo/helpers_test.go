package demo

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/rawstore"
)

// now is 09:00 Oslo time on Tuesday 10 November 2026: the clock of every service test.
var now = time.Date(2026, time.November, 10, 8, 0, 0, 0, time.UTC)

// in is a moment the given number of days from now.
func in(days int) time.Time { return now.Add(time.Duration(days) * 24 * time.Hour) }

var fetched = time.Date(2026, time.November, 9, 12, 0, 0, 0, time.UTC)

func doc(id string, body []byte) rawstore.Document {
	sum := sha256.Sum256(body)
	return rawstore.Document{ID: id, Body: body, SHA256: hex.EncodeToString(sum[:]), FetchedAt: fetched}
}

// fakeNotices is a store of documents in the order a real one returns them: newest first.
type fakeNotices struct {
	docs []rawstore.Document
	err  error

	mu      sync.Mutex
	limits  []int    // the limit of each RecentNotices call
	fetched []string // the ids asked for with Notice
}

func (f *fakeNotices) RecentNotices(_ context.Context, limit int, fn func(rawstore.Document) error) error {
	f.mu.Lock()
	f.limits = append(f.limits, limit)
	f.mu.Unlock()
	if f.err != nil {
		return f.err
	}
	for i, d := range f.docs {
		if i >= limit {
			break
		}
		if err := fn(d); err != nil {
			return err
		}
	}
	return nil
}

func (f *fakeNotices) Notice(_ context.Context, id string) (rawstore.Document, error) {
	f.mu.Lock()
	f.fetched = append(f.fetched, id)
	f.mu.Unlock()
	if f.err != nil {
		return rawstore.Document{}, f.err
	}
	for _, d := range f.docs {
		if d.ID == id {
			return d, nil
		}
	}
	return rawstore.Document{}, rawstore.ErrNotFound
}

// fakeCompanies answers a lookup with the organisation or the error it holds.
type fakeCompanies struct {
	org orgcore.Organisation
	err error

	mu    sync.Mutex
	calls []string
}

func (f *fakeCompanies) Lookup(_ context.Context, orgNumber string) (orgcore.Organisation, error) {
	f.mu.Lock()
	f.calls = append(f.calls, orgNumber)
	f.mu.Unlock()
	return f.org, f.err
}

func newService(t *testing.T, notices NoticeSource, companies CompanyLookup) *Service {
	t.Helper()
	s, err := New(Options{Notices: notices, Companies: companies, Now: func() time.Time { return now }})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return s
}

func resultIDs(results []Result) []string {
	ids := make([]string, len(results))
	for i, r := range results {
		ids[i] = r.NoticeID
	}
	return ids
}
