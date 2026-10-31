package main

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// seedRefusals records n earlier refusals of id, each age old, under a finished run,
// as earlier runs of the sync would have left them.
func seedRefusals(t *testing.T, pool *pgxpool.Pool, d time.Time, id string, n int, age time.Duration) {
	t.Helper()
	ctx := context.Background()
	store := rawstore.New(pool)
	runID, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: d, To: d})
	if err != nil {
		t.Fatalf("seed run: %v", err)
	}
	for range n {
		a := doffinsync.Attempt{
			DoffinID: id, Outcome: doffinsync.OutcomeEmptyBody, Message: "empty response body",
			At: time.Now().UTC().Add(-age),
		}
		if err := store.RecordAttempt(ctx, runID, a); err != nil {
			t.Fatalf("seed refusal: %v", err)
		}
	}
	out := doffinsync.RunOutcome{Reason: doffinsync.ReasonPartial, LastError: "days remain incomplete"}
	if err := store.FinishRun(ctx, runID, out); err != nil {
		t.Fatalf("finish seed run: %v", err)
	}
}

// R1 to R5 through the CLI, the real client, the real store and the database: a
// notice refused twice, two days ago, is eligible again. Doffin answers it with an
// empty body once more, which is its third refusal, so it is terminal in this run. The
// day is done, and the day line says rejected 1 where an unresolved count used to be.
func TestCLICountsTheThirdRefusalAsRejected(t *testing.T) {
	pool := dbtest.Open(t)
	d := loopbackDay()
	ids := loopbackIDs(d)[:3]
	refused := ids[2]
	seedHeld(t, pool, d, ids[:2])
	seedRefusals(t, pool, d, refused, doffinsync.MaxRejections-1, 48*time.Hour)

	// The loopback answers the refused id with a 200 and no body, which the client
	// reports as an empty response. Any other id would come back as a notice, but the
	// two others are held and are never asked for.
	fake := &loopbackDoffin{ids: ids, downloads: make(map[string]int)}
	var refusedCalls atomic.Int32
	mux := http.NewServeMux()
	mux.HandleFunc("/public/v2/search", fake.search)
	mux.HandleFunc("/public/v2/download/", func(w http.ResponseWriter, r *http.Request) {
		if strings.TrimPrefix(r.URL.Path, "/public/v2/download/") == refused && r.Header.Get(doffin.KeyHeader) == cliKey {
			refusedCalls.Add(1)
			w.Header().Set("Content-Type", "application/xml")
			return
		}
		fake.download(w, r)
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)

	req := request{window: doffinsync.Window{From: d, To: d}, mode: doffinsync.ModeSync}
	code, stdout, stderr := invoke(t, pool, loopbackClient(t, srv.URL), req)
	if code != exitDone {
		t.Fatalf("exit = %d, want %d (done); stderr %q", code, exitDone, stderr)
	}

	if got := refusedCalls.Load(); got != 1 {
		t.Errorf("downloads of the refused id = %d, want 1", got)
	}
	if _, downloads := fake.calls(); len(downloads) != 0 {
		t.Errorf("downloads of the held ids = %v, want none", downloads)
	}
	objs := jsonLines(t, stdout)
	if len(objs) != 2 {
		t.Fatalf("stdout has %d objects, want one day line and one summary: %q", len(objs), stdout)
	}
	day := objs[0]
	if day["state"] != "done" || day["held"] != 2.0 || day["stored"] != 0.0 || day["not_found"] != 0.0 ||
		day["rejected"] != 1.0 || day["unresolved"] != 0.0 {
		t.Errorf("day line = %v, want done with held 2, rejected 1 and nothing else counted", day)
	}
	if _, hasCode := day["error_code"]; hasCode {
		t.Errorf("day line = %v, want no error code on a done day", day)
	}
	if summary := objs[1]; summary["reason"] != "done" {
		t.Errorf("summary = %v, want reason done", summary)
	}

	assertRows(t, pool, `SELECT count(*) FROM download_attempts WHERE doffin_id = $1 AND outcome = 'empty_body'`, 3, refused)
	assertRows(t, pool, `SELECT count(*) FROM sync_days WHERE state = 'done' AND rejected = 1 AND held = 2 AND unresolved = 0`, 1)
	assertNoLeak(t, stdout+stderr, ids)
}
