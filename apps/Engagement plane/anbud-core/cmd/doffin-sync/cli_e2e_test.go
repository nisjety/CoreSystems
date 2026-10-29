package main

import (
	"bytes"
	"context"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Needs a throwaway database on ANBUD_TEST_DSN: run it with make test-db. cli runs
// one sync from its arguments and its environment, against a fresh database and
// the loopback Doffin. It covers the
// same day as TestCLISyncsAgainstLoopbackDoffin, but reaches it through cli rather
// than runSync, so the environment path is covered too. Nothing printed carries
// the DSN or the key.
func TestCLIEndToEndAgainstLoopbackDoffin(t *testing.T) {
	dsn := dbtest.Database(t)
	pool, err := pgxpool.New(context.Background(), dsn)
	if err != nil {
		t.Fatalf("open pool on the fresh database: %v", err)
	}
	t.Cleanup(pool.Close)

	d := loopbackDay()
	ids := loopbackIDs(d)
	seedHeld(t, pool, d, ids[:cliHeld])
	fake, srv := newLoopbackDoffin(t, ids, ids[cliHeld+1])
	stored, notFound := ids[cliHeld], ids[cliHeld+1]

	env := map[string]string{
		"ANBUD_DSN":           dsn,
		"DOFFIN_API_KEY":      cliKey,
		"DOFFIN_BASE_URL":     srv.URL + "/public/v2",
		"DOFFIN_DOWNLOAD_RPM": "10",
	}
	day := d.Format(dateLayout)
	var stdout, stderr bytes.Buffer
	code := cli([]string{"-from", day, "-to", day}, mapEnv(env), &stdout, &stderr)
	if code != exitDone {
		t.Fatalf("exit = %d, want %d (done); stderr %q", code, exitDone, stderr.String())
	}

	searches, downloads := fake.calls()
	if searches != 4 {
		t.Errorf("searches = %d, want 4: two passes over pages 1 and 2 (D-B3)", searches)
	}
	if len(downloads) != 2 || downloads[stored] != 1 || downloads[notFound] != 1 {
		t.Errorf("downloads = %v, want one call each for the two unheld ids and none else", downloads)
	}
	assertRows(t, pool, `SELECT count(*) FROM raw_documents WHERE doffin_id = $1`, 1, stored)
	assertRows(t, pool, `SELECT count(*) FROM download_attempts WHERE doffin_id = $1 AND outcome = 'not_found' AND http_status = 404`, 1, notFound)

	objs := jsonLines(t, stdout.String())
	if len(objs) != 2 {
		t.Fatalf("stdout has %d objects, want one day line and one summary: %q", len(objs), stdout.String())
	}
	if day := objs[0]; day["state"] != "done" || day["rejected"] != 0.0 {
		t.Errorf("day line = %v, want done with a rejected field of 0", day)
	}
	if summary := objs[1]; summary["reason"] != "done" || summary["run_id"] == nil {
		t.Errorf("summary = %v, want reason done and a run id", summary)
	}

	out := stdout.String() + stderr.String()
	for _, secret := range []string{dsn, cliKey} {
		if strings.Contains(out, secret) {
			t.Error("output echoes the DSN or the key")
		}
	}
	assertNoLeak(t, out, ids)
}
