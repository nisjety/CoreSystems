package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/secret"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

const (
	cliKey  = "cli-loopback-key-0001"
	cliBody = "<notice>cli-body-marker-7f3a</notice>"
	// cliHits is the day's total. It is more than one search page (the engine
	// pages at 100), so the search really paginates.
	cliHits = 101
	// cliHeld is how many of those hits an earlier run already stored. The run
	// under test therefore downloads only the two that remain.
	cliHeld = 99
)

// loopbackDay is 30 days old, so a 404 on one of its notices is terminal (D11).
func loopbackDay() time.Time {
	now := time.Now().UTC()
	return time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC).AddDate(0, 0, -30)
}

// loopbackIDs are the day's hits, in search order.
func loopbackIDs(d time.Time) []string {
	ids := make([]string, cliHits)
	for i := range ids {
		ids[i] = fmt.Sprintf("%04d-%06d", d.Year(), i+1)
	}
	return ids
}

// loopbackDoffin stands in for Doffin on the loopback interface. It serves the
// day's hits in pages of 100 and answers downloads, counting every call.
type loopbackDoffin struct {
	ids      []string
	notFound string

	mu        sync.Mutex
	searches  int
	downloads map[string]int
}

func newLoopbackDoffin(t *testing.T, ids []string, notFound string) (*loopbackDoffin, *httptest.Server) {
	t.Helper()
	f := &loopbackDoffin{ids: ids, notFound: notFound, downloads: make(map[string]int)}
	mux := http.NewServeMux()
	mux.HandleFunc("/public/v2/search", f.search)
	mux.HandleFunc("/public/v2/download/", f.download)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return f, srv
}

func (f *loopbackDoffin) search(w http.ResponseWriter, r *http.Request) {
	if r.Header.Get(doffin.KeyHeader) != cliKey {
		http.Error(w, `{"message":"bad key"}`, http.StatusUnauthorized)
		return
	}
	page, err := strconv.Atoi(r.URL.Query().Get("page"))
	if err != nil || page < 1 {
		http.Error(w, `{"message":"bad page"}`, http.StatusBadRequest)
		return
	}
	f.mu.Lock()
	f.searches++
	f.mu.Unlock()

	lo, hi := min((page-1)*100, len(f.ids)), min(page*100, len(f.ids))
	hits := make([]map[string]string, 0, hi-lo)
	for _, id := range f.ids[lo:hi] {
		hits = append(hits, map[string]string{"id": id})
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]any{
		"numHitsTotal":      len(f.ids),
		"numHitsAccessible": len(f.ids),
		"hits":              hits,
	})
}

func (f *loopbackDoffin) download(w http.ResponseWriter, r *http.Request) {
	if r.Header.Get(doffin.KeyHeader) != cliKey {
		http.Error(w, `{"message":"bad key"}`, http.StatusUnauthorized)
		return
	}
	id := strings.TrimPrefix(r.URL.Path, "/public/v2/download/")
	f.mu.Lock()
	f.downloads[id]++
	f.mu.Unlock()
	if id == f.notFound {
		http.Error(w, `{"message":"notice not found"}`, http.StatusNotFound)
		return
	}
	w.Header().Set("Content-Type", "application/xml")
	_, _ = w.Write([]byte(cliBody))
}

// calls returns the search count and a copy of the per-id download counts.
func (f *loopbackDoffin) calls() (int, map[string]int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	downloads := make(map[string]int, len(f.downloads))
	for id, n := range f.downloads {
		downloads[id] = n
	}
	return f.searches, downloads
}

// loopbackClient is the real client pointed at the loopback server, with the
// documented per-key limits so the test is not paced to a crawl.
func loopbackClient(t *testing.T, base string) *doffin.Client {
	t.Helper()
	c, err := doffin.New(doffin.Config{
		BaseURL:     base + "/public/v2",
		APIKey:      secret.Value(cliKey),
		SearchRPM:   doffin.MaxSearchRPM,
		DownloadRPM: doffin.MaxDownloadRPM,
		Timeout:     10 * time.Second,
	})
	if err != nil {
		t.Fatalf("doffin.New: %v", err)
	}
	return c
}

// seedHeld stores ids as an earlier run would have: raw rows and stored attempts
// under a finished run. A sync then finds them held and never downloads them.
func seedHeld(t *testing.T, pool *pgxpool.Pool, d time.Time, ids []string) {
	t.Helper()
	ctx := context.Background()
	store := rawstore.New(pool)
	runID, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: d, To: d})
	if err != nil {
		t.Fatalf("seed run: %v", err)
	}
	fetched := time.Now().UTC()
	for _, id := range ids {
		body := []byte("<notice>held " + id + "</notice>")
		sum := sha256.Sum256(body)
		n := doffin.Notice{
			ID:          id,
			ContentType: "application/xml",
			Body:        body,
			SHA256:      hex.EncodeToString(sum[:]),
			FetchedAt:   fetched,
		}
		if _, err := store.StoreNotice(ctx, runID, n); err != nil {
			t.Fatalf("seed notice: %v", err)
		}
	}
	out := doffinsync.RunOutcome{Reason: doffinsync.ReasonDone, Counters: doffinsync.Counters{Stored: len(ids)}}
	if err := store.FinishRun(ctx, runID, out); err != nil {
		t.Fatalf("finish seed run: %v", err)
	}
}

// invoke runs one sync and returns its exit code with what it printed.
func invoke(t *testing.T, pool *pgxpool.Pool, up upstream, req request) (int, string, string) {
	t.Helper()
	var stdout, stderr bytes.Buffer
	code := runSync(context.Background(), pool, up, req, &stdout, &stderr)
	return code, stdout.String(), stderr.String()
}

// jsonLines decodes each line of a run's stdout as one JSON object.
func jsonLines(t *testing.T, out string) []map[string]any {
	t.Helper()
	var objs []map[string]any
	for _, line := range strings.Split(strings.TrimSpace(out), "\n") {
		if line == "" {
			continue
		}
		var obj map[string]any
		if err := json.Unmarshal([]byte(line), &obj); err != nil {
			t.Fatalf("stdout line is not one JSON object: %v", err)
		}
		objs = append(objs, obj)
	}
	return objs
}

// assertNoLeak fails when the output carries a notice id or a body. The report
// carries counts, states and the run id only.
func assertNoLeak(t *testing.T, output string, ids []string) {
	t.Helper()
	for _, id := range ids {
		if strings.Contains(output, id) {
			t.Errorf("output carries a notice id")
			break
		}
	}
	if strings.Contains(output, "cli-body-marker") {
		t.Error("output carries a notice body")
	}
}

// assertRows runs a COUNT query and checks the result.
func assertRows(t *testing.T, pool *pgxpool.Pool, query string, want int64, args ...any) {
	t.Helper()
	var got int64
	if err := pool.QueryRow(context.Background(), query, args...).Scan(&got); err != nil {
		t.Fatalf("count query: %v", err)
	}
	if got != want {
		t.Errorf("%s %v = %d, want %d", query, args, got, want)
	}
}

// T69: the CLI's run function syncs one day through the real client against a
// loopback Doffin. The day has 101 hits over two search pages. 99 are already
// held, so the run downloads two notices: one is stored and one answers 404.
func TestCLISyncsAgainstLoopbackDoffin(t *testing.T) {
	pool := dbtest.Open(t)
	d := loopbackDay()
	ids := loopbackIDs(d)
	seedHeld(t, pool, d, ids[:cliHeld])
	fake, srv := newLoopbackDoffin(t, ids, ids[cliHeld+1])
	stored, notFound := ids[cliHeld], ids[cliHeld+1]

	req := request{window: doffinsync.Window{From: d, To: d}, mode: doffinsync.ModeSync}
	code, stdout, stderr := invoke(t, pool, loopbackClient(t, srv.URL), req)
	if code != exitDone {
		t.Fatalf("exit = %d, want %d (done); stderr %q", code, exitDone, stderr)
	}

	searches, downloads := fake.calls()
	if searches != 4 {
		t.Errorf("searches = %d, want 4: two passes over pages 1 and 2 (D-B3)", searches)
	}
	if len(downloads) != 2 || downloads[stored] != 1 || downloads[notFound] != 1 {
		t.Errorf("downloads = %v, want one call each for the two unheld ids and none else", downloads)
	}

	assertRows(t, pool, `SELECT count(*) FROM raw_documents WHERE doffin_id = $1`, 1, stored)
	assertRows(t, pool, `SELECT count(*) FROM raw_documents WHERE doffin_id = $1`, 0, notFound)
	assertRows(t, pool, `SELECT count(*) FROM download_attempts WHERE doffin_id = $1 AND outcome = 'stored'`, 1, stored)
	assertRows(t, pool, `SELECT count(*) FROM download_attempts WHERE doffin_id = $1 AND outcome = 'not_found' AND http_status = 404`, 1, notFound)

	var state string
	if err := pool.QueryRow(context.Background(), `SELECT state FROM sync_runs ORDER BY id DESC LIMIT 1`).Scan(&state); err != nil {
		t.Fatalf("read run state: %v", err)
	}
	if state != "done" {
		t.Errorf("latest run state = %q, want done", state)
	}

	objs := jsonLines(t, stdout)
	if len(objs) != 2 {
		t.Fatalf("stdout has %d objects, want one day line and one summary: %q", len(objs), stdout)
	}
	if day := objs[0]; day["state"] != "done" || day["held"] != 99.0 || day["stored"] != 1.0 || day["not_found"] != 1.0 || day["rejected"] != 0.0 {
		t.Errorf("day line = %v, want done with held 99, stored 1, not_found 1, rejected 0", day)
	}
	if summary := objs[1]; summary["reason"] != "done" || summary["run_id"] == nil {
		t.Errorf("summary = %v, want reason done and a run id", summary)
	}
	assertNoLeak(t, stdout+stderr, ids)
}

// A second run over the same window, with the last day rechecked, searches the
// day again and downloads nothing. Its 100 held ids (the 99 seeded plus the one the
// first run stored) and its 404 id are never sent again.
func TestRerunMakesNoDownloadCallsForHeldIDs(t *testing.T) {
	pool := dbtest.Open(t)
	d := loopbackDay()
	ids := loopbackIDs(d)
	seedHeld(t, pool, d, ids[:cliHeld])
	fake, srv := newLoopbackDoffin(t, ids, ids[cliHeld+1])
	up := loopbackClient(t, srv.URL)
	window := doffinsync.Window{From: d, To: d}

	first := request{window: window, mode: doffinsync.ModeSync}
	if code, _, stderr := invoke(t, pool, up, first); code != exitDone {
		t.Fatalf("first run exit = %d, want %d; stderr %q", code, exitDone, stderr)
	}
	searchesBefore, downloadsBefore := fake.calls()

	rerun := request{window: window, mode: doffinsync.ModeSync, recheck: 1}
	code, stdout, stderr := invoke(t, pool, up, rerun)
	if code != exitDone {
		t.Fatalf("rerun exit = %d, want %d; stderr %q", code, exitDone, stderr)
	}
	searchesAfter, downloadsAfter := fake.calls()
	if got := searchesAfter - searchesBefore; got != 4 {
		t.Errorf("rerun searches = %d, want 4: the rechecked day, two passes over two pages (D-B3)", got)
	}
	if !reflect.DeepEqual(downloadsBefore, downloadsAfter) {
		t.Errorf("rerun made download calls: before %v, after %v", downloadsBefore, downloadsAfter)
	}
	objs := jsonLines(t, stdout)
	if len(objs) != 2 {
		t.Fatalf("stdout has %d objects, want 2: %q", len(objs), stdout)
	}
	if day := objs[0]; day["stored"] != 0.0 || day["held"] != 100.0 || day["not_found"] != 1.0 || day["rejected"] != 0.0 {
		t.Errorf("rerun day line = %v, want stored 0, held 100, not_found 1, rejected 0", day)
	}
}

// A stored retry instant still ahead closes the gate: exit 3, the instant printed,
// and not one call to Doffin.
func TestRefusedStartMakesNoDoffinCalls(t *testing.T) {
	pool := dbtest.Open(t)
	ctx := context.Background()
	d := loopbackDay()
	ids := loopbackIDs(d)
	fake, srv := newLoopbackDoffin(t, ids, ids[cliHeld+1])

	store := rawstore.New(pool)
	runID, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: d, To: d})
	if err != nil {
		t.Fatalf("begin run: %v", err)
	}
	until := time.Now().UTC().Add(time.Hour).Truncate(time.Second)
	out := doffinsync.RunOutcome{Reason: doffinsync.ReasonPaused, RetryAfterUntil: &until}
	if err := store.FinishRun(ctx, runID, out); err != nil {
		t.Fatalf("finish paused run: %v", err)
	}

	req := request{window: doffinsync.Window{From: d, To: d}, mode: doffinsync.ModeSync}
	code, stdout, stderr := invoke(t, pool, loopbackClient(t, srv.URL), req)
	if code != exitPaused {
		t.Fatalf("exit = %d, want %d (paused); stderr %q", code, exitPaused, stderr)
	}
	searches, downloads := fake.calls()
	if searches != 0 || len(downloads) != 0 {
		t.Errorf("made %d searches and %d download ids before the gate opened, want none", searches, len(downloads))
	}
	objs := jsonLines(t, stdout)
	if len(objs) != 1 || objs[0]["reason"] != "paused" || objs[0]["retry_after_until"] == nil {
		t.Errorf("stdout = %q, want one paused summary carrying the retry instant", stdout)
	}
}

// Another sync holds the budget lock: exit 7, nothing called, nothing printed.
func TestHeldLockExitsSevenWithoutCalls(t *testing.T) {
	pool := dbtest.Open(t)
	holder, err := rawstore.New(pool).TryLock(context.Background())
	if err != nil {
		t.Fatalf("take the lock: %v", err)
	}
	t.Cleanup(func() { _ = holder.Release(context.Background()) })

	d := loopbackDay()
	ids := loopbackIDs(d)
	fake, srv := newLoopbackDoffin(t, ids, ids[cliHeld+1])
	req := request{window: doffinsync.Window{From: d, To: d}, mode: doffinsync.ModeSync}
	code, stdout, stderr := invoke(t, pool, loopbackClient(t, srv.URL), req)
	if code != exitLocked {
		t.Fatalf("exit = %d, want %d (locked); stderr %q", code, exitLocked, stderr)
	}
	searches, downloads := fake.calls()
	if searches != 0 || len(downloads) != 0 {
		t.Errorf("made %d searches and %d download ids while locked, want none", searches, len(downloads))
	}
	if stdout != "" {
		t.Errorf("stdout = %q, want nothing", stdout)
	}
}
