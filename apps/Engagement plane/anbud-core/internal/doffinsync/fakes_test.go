package doffinsync

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// testNow is the fixed clock of the unit tests: 2026-10-09 12:00 UTC. Yesterday,
// 2026-10-08, is the latest closed day.
var testNow = time.Date(2026, time.October, 9, 12, 0, 0, 0, time.UTC)

const isoDay = "2006-01-02"

func day(y int, m time.Month, d int) time.Time {
	return time.Date(y, m, d, 0, 0, 0, 0, time.UTC)
}

// searchCall is one recorded Search call.
type searchCall struct {
	From, To time.Time
	Page     int
	PageSet  bool
	Size     int
	Sort     string
}

// fakeAPI satisfies Searcher and Downloader. Scripts decide each reply and
// every call is recorded.
type fakeAPI struct {
	mu        sync.Mutex
	search    func(call int, p doffin.SearchParams) (doffin.SearchResponse, error)
	download  func(ctx context.Context, id string) (doffin.Notice, error)
	searches  []searchCall
	downloads []string
}

func (f *fakeAPI) Search(_ context.Context, p doffin.SearchParams) (doffin.SearchResponse, error) {
	c := searchCall{From: p.IssueDateFrom, To: p.IssueDateTo, Size: p.NumHitsPerPage, Sort: p.SortBy}
	if p.Page != nil {
		c.Page, c.PageSet = *p.Page, true
	}
	f.mu.Lock()
	f.searches = append(f.searches, c)
	call := len(f.searches)
	script := f.search
	f.mu.Unlock()
	if script == nil {
		return doffin.SearchResponse{}, errors.New("fakeAPI: no search script")
	}
	return script(call, p)
}

func (f *fakeAPI) Download(ctx context.Context, id string) (doffin.Notice, error) {
	f.mu.Lock()
	f.downloads = append(f.downloads, id)
	script := f.download
	f.mu.Unlock()
	if script == nil {
		return notice(id, []byte("notice body "+id)), nil
	}
	return script(ctx, id)
}

// pageNumbers returns the page of every Search call in order.
func (f *fakeAPI) pageNumbers() []int {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]int, 0, len(f.searches))
	for _, c := range f.searches {
		out = append(out, c.Page)
	}
	return out
}

// searchDays returns the single day each Search call asked for, in order.
func (f *fakeAPI) searchDays() []time.Time {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]time.Time, 0, len(f.searches))
	for _, c := range f.searches {
		out = append(out, c.From)
	}
	return out
}

// fakeStore implements PageStore, NoticeStore, RunLedger and DayLedger in
// memory. Each method records its input, and scripted errors stop the run at
// that point.
type fakeStore struct {
	mu sync.Mutex

	dayStates    map[string]DayState
	dayStatesErr error
	resolve      map[string]Resolution
	resolveErr   error
	putErr       error
	storeErr     error
	recordErr    error
	beginRunErr  error
	finishRunErr error
	beginDayErr  error
	finishDayErr error

	resolveCalls  [][]string
	pages         []SearchPage
	notices       []doffin.Notice // every StoreNotice call, in order
	inserted      map[string]bool // id and sha256 seen by StoreNotice
	attempts      []Attempt
	begunDays     []time.Time
	finished      []DayRecord
	finishCtxErrs []error // ctx.Err() seen by each FinishDay
	runStarts     []RunStart
	runOutcomes   []RunOutcome
}

func (s *fakeStore) PutSearchPage(_ context.Context, _ int64, p SearchPage) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.putErr != nil {
		return s.putErr
	}
	s.pages = append(s.pages, p)
	return nil
}

func (s *fakeStore) Resolve(_ context.Context, ids []string) (map[string]Resolution, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.resolveCalls = append(s.resolveCalls, append([]string(nil), ids...))
	if s.resolveErr != nil {
		return nil, s.resolveErr
	}
	out := make(map[string]Resolution)
	for _, id := range ids {
		if r, ok := s.resolve[id]; ok {
			out[id] = r
		}
	}
	return out, nil
}

func (s *fakeStore) StoreNotice(_ context.Context, _ int64, n doffin.Notice) (bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.storeErr != nil {
		return false, s.storeErr
	}
	s.notices = append(s.notices, n)
	if s.inserted == nil {
		s.inserted = make(map[string]bool)
	}
	key := n.ID + "/" + n.SHA256
	if s.inserted[key] {
		return false, nil
	}
	s.inserted[key] = true
	return true, nil
}

func (s *fakeStore) RecordAttempt(_ context.Context, _ int64, a Attempt) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.recordErr != nil {
		return s.recordErr
	}
	s.attempts = append(s.attempts, a)
	return nil
}

func (s *fakeStore) BeginRun(_ context.Context, r RunStart) (int64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.beginRunErr != nil {
		return 0, s.beginRunErr
	}
	s.runStarts = append(s.runStarts, r)
	return 7, nil
}

func (s *fakeStore) FinishRun(_ context.Context, _ int64, out RunOutcome) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.runOutcomes = append(s.runOutcomes, out)
	return s.finishRunErr
}

func (s *fakeStore) DayStates(_ context.Context, _, _ time.Time) (map[string]DayState, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.dayStatesErr != nil {
		return nil, s.dayStatesErr
	}
	out := make(map[string]DayState, len(s.dayStates))
	for k, v := range s.dayStates {
		out[k] = v
	}
	return out, nil
}

func (s *fakeStore) BeginDay(_ context.Context, d time.Time, _ int64) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.beginDayErr != nil {
		return s.beginDayErr
	}
	s.begunDays = append(s.begunDays, d)
	return nil
}

func (s *fakeStore) FinishDay(ctx context.Context, rec DayRecord) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.finishCtxErrs = append(s.finishCtxErrs, ctx.Err())
	if s.finishDayErr != nil {
		return s.finishDayErr
	}
	s.finished = append(s.finished, rec)
	return nil
}

// harness wires the fakes into an Engine.
type harness struct {
	api   *fakeAPI
	store *fakeStore
}

func newHarness() *harness {
	return &harness{api: &fakeAPI{}, store: &fakeStore{}}
}

func (h *harness) deps() Deps {
	return Deps{Search: h.api, Download: h.api, Pages: h.store, Notices: h.store, Runs: h.store, Days: h.store}
}

// engine builds an Engine on the fixed test clock. mutate adjusts the options.
func (h *harness) engine(t *testing.T, mode Mode, mutate ...func(*Options)) *Engine {
	t.Helper()
	opts := Options{Mode: mode, Now: func() time.Time { return testNow }}
	for _, m := range mutate {
		m(&opts)
	}
	e, err := New(h.deps(), opts)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return e
}

// run runs the window and fails the test on a ledger error.
func (h *harness) run(t *testing.T, e *Engine, from, to time.Time) Summary {
	t.Helper()
	sum, err := e.Run(context.Background(), Window{From: from, To: to})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	return sum
}

// finishedDay returns the last DayRecord written for d, or fails the test.
func (h *harness) finishedDay(t *testing.T, d time.Time) DayRecord {
	t.Helper()
	h.store.mu.Lock()
	defer h.store.mu.Unlock()
	for i := len(h.store.finished) - 1; i >= 0; i-- {
		if h.store.finished[i].Day.Equal(d) {
			return h.store.finished[i]
		}
	}
	t.Fatalf("no FinishDay for %s", d.Format(isoDay))
	return DayRecord{}
}

// notice builds a downloaded notice whose SHA-256 is computed over body.
func notice(id string, body []byte) doffin.Notice {
	sum := sha256.Sum256(body)
	return doffin.Notice{
		ID:          id,
		ContentType: "application/xml",
		Body:        body,
		SHA256:      hex.EncodeToString(sum[:]),
		FetchedAt:   testNow,
	}
}

// ids returns n Doffin-shaped notice ids starting at first.
func ids(first, n int) []string {
	out := make([]string, n)
	for i := range out {
		out[i] = fmt.Sprintf("2026-%06d", first+i)
	}
	return out
}

// pageOf returns the hits of page n (1-based, 100 per page) of all.
func pageOf(all []string, n int) []string {
	start := (n - 1) * 100
	if n < 1 || start >= len(all) {
		return nil
	}
	return all[start:min(start+100, len(all))]
}

// searchResp is a successful search page with the given counts and hit ids.
func searchResp(total, accessible int64, hitIDs []string) doffin.SearchResponse {
	hits := make(doffin.OneOrMany[doffin.Hit], 0, len(hitIDs))
	for _, id := range hitIDs {
		hits = append(hits, doffin.Hit{ID: id})
	}
	body := fmt.Sprintf(`{"numHitsTotal":%d,"numHitsAccessible":%d}`, total, accessible)
	return doffin.SearchResponse{
		NumHitsTotal:      doffin.Count(total),
		NumHitsAccessible: doffin.Count(accessible),
		Hits:              hits,
		Raw:               json.RawMessage(body),
		ContentType:       "application/json; charset=utf-8",
		FetchedAt:         testNow,
	}
}

// pageNum is the page a search asked for, or -1 when none was set.
func pageNum(p doffin.SearchParams) int {
	if p.Page == nil {
		return -1
	}
	return *p.Page
}

// held marks every id as already held in the raw store.
func held(all []string) map[string]Resolution {
	m := make(map[string]Resolution, len(all))
	for _, id := range all {
		m[id] = Resolution{Kind: ResolutionHeld}
	}
	return m
}

// seq returns the integers from..to inclusive.
func seq(from, to int) []int {
	var out []int
	for i := from; i <= to; i++ {
		out = append(out, i)
	}
	return out
}
