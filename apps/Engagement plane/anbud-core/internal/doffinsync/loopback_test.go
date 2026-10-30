package doffinsync

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/secret"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// T45: a sync through the real doffin.Client against a loopback server. The
// client paces itself to the documented limits, so this takes about 14 seconds
// and is skipped under -short.
func TestSyncThroughRealClientOnLoopback(t *testing.T) {
	if testing.Short() {
		t.Skip("loopback sync paces to the documented Doffin limits; skipped in -short mode")
	}

	const key = "loopback-test-key-0001"
	all := make([]string, 101)
	for i := range all {
		all[i] = fmt.Sprintf("2025-%06d", i+1)
	}
	// Thirty days old, so a 404 on one of its notices is terminal.
	now := time.Now().UTC()
	d := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC).AddDate(0, 0, -30)
	dayText := d.Format(isoDay)

	var mu sync.Mutex
	var queries []string
	mux := http.NewServeMux()
	mux.HandleFunc("/public/v2/search", func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		mu.Lock()
		queries = append(queries, strings.Join([]string{q.Get("page"), q.Get("issueDateFrom"), q.Get("issueDateTo")}, " "))
		mu.Unlock()
		if r.Header.Get(doffin.KeyHeader) != key {
			http.Error(w, `{"message":"bad key"}`, http.StatusUnauthorized)
			return
		}
		page, err := strconv.Atoi(q.Get("page"))
		if err != nil || page < 1 {
			http.Error(w, `{"message":"bad page"}`, http.StatusBadRequest)
			return
		}
		hits := make([]map[string]string, 0)
		for _, id := range pageOf(all, page) {
			hits = append(hits, map[string]string{"id": id})
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"numHitsTotal": 101, "numHitsAccessible": 101, "hits": hits})
	})
	mux.HandleFunc("/public/v2/download/", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get(doffin.KeyHeader) != key {
			http.Error(w, `{"message":"bad key"}`, http.StatusUnauthorized)
			return
		}
		switch strings.TrimPrefix(r.URL.Path, "/public/v2/download/") {
		case all[0]:
			w.Header().Set("Content-Type", "application/xml")
			_, _ = w.Write([]byte("<notice>one</notice>"))
		case all[1]:
			http.Error(w, `{"message":"notice not found"}`, http.StatusNotFound)
		case all[2]:
			w.Header().Set("Retry-After", "1")
			http.Error(w, `{"message":"slow down"}`, http.StatusTooManyRequests)
		default:
			http.Error(w, `{"message":"unexpected id"}`, http.StatusInternalServerError)
		}
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)

	client, err := doffin.New(doffin.Config{
		BaseURL:     srv.URL + "/public/v2",
		APIKey:      secret.Value(key),
		SearchRPM:   doffin.MaxSearchRPM,
		DownloadRPM: doffin.MaxDownloadRPM,
		MaxRetries:  -1, // surface the 429 to the engine instead of absorbing it
		Timeout:     10 * time.Second,
	})
	if err != nil {
		t.Fatalf("doffin.New: %v", err)
	}

	store := &fakeStore{resolve: held(all[3:])}
	e, err := New(Deps{Search: client, Download: client, Pages: store, Notices: store, Runs: store, Days: store}, Options{Mode: ModeSync})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sum, err := e.Run(context.Background(), Window{From: d, To: d})
	returned := time.Now()
	if err != nil {
		t.Fatalf("Run: %v", err)
	}

	want := Counters{Searches: 4, Downloads: 3, Stored: 1, HeldSkipped: 98, NotFound: 1}
	if sum.Counters != want {
		t.Errorf("counters = %+v, want %+v", sum.Counters, want)
	}
	if sum.Reason != ReasonPaused {
		t.Errorf("run reason = %s, want paused on the 429", sum.Reason)
	}
	// The 429 arrived just before Run returned, so the instant is one second
	// after that moment, less the few milliseconds the engine spent since.
	if sum.RetryAfterUntil == nil {
		t.Error("RetryAfterUntil is nil, want the Retry-After instant")
	} else if ahead := sum.RetryAfterUntil.Sub(returned); ahead < 500*time.Millisecond || ahead > time.Second {
		t.Errorf("RetryAfterUntil is %v after Run returned, want just under 1s", ahead)
	}

	h := &harness{store: store}
	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StatePaused || rec.ErrorCode != CodeRateLimited:
		t.Errorf("day = %s/%s, want paused/rate_limited", rec.State, rec.ErrorCode)
	case rec.Held != 98 || rec.Stored != 1 || rec.NotFound != 1 || rec.Unresolved != 0:
		t.Errorf("day counts held %d stored %d not_found %d unresolved %d, want 98 1 1 0",
			rec.Held, rec.Stored, rec.NotFound, rec.Unresolved)
	}

	mu.Lock()
	gotQueries := append([]string(nil), queries...)
	mu.Unlock()
	page1, page2 := "1 "+dayText+" "+dayText, "2 "+dayText+" "+dayText
	if wantQ := []string{page1, page2, page1, page2}; strings.Join(gotQueries, "|") != strings.Join(wantQ, "|") {
		t.Errorf("search queries = %q, want %q: pages 1 and 2 in each of two passes, never page 0", gotQueries, wantQ)
	}

	if len(store.notices) != 1 || store.notices[0].ID != all[0] {
		t.Errorf("stored notices = %d, want the one notice %s", len(store.notices), all[0])
	}
	if len(store.attempts) != 2 ||
		store.attempts[0].DoffinID != all[1] || store.attempts[0].Outcome != OutcomeNotFound || store.attempts[0].HTTPStatus != 404 ||
		store.attempts[1].DoffinID != all[2] || store.attempts[1].Outcome != OutcomeRateLimited || store.attempts[1].HTTPStatus != 429 {
		t.Errorf("attempts = %+v, want a 404 not_found on %s and a 429 rate_limited on %s", store.attempts, all[1], all[2])
	}
}
