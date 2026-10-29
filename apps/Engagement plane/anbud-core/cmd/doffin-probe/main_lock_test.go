package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/secret"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// The probe shares the Doffin budget with doffin-sync, so it takes the same budget
// lock. These tests use a fake locker and need no database. The tests that use the
// real lock are in main_lock_db_test.go.

// fakeLocker stands in for the database lock and records what the probe does with it.
type fakeLocker struct {
	tryErr     error // returned by TryLock
	releaseErr error // returned by Release

	mu                 sync.Mutex
	tries, releases    int
	held               bool
	releaseCtxErr      error         // the release context's error at the call
	releaseHasDeadline bool          // the release context carries a deadline
	releaseRemaining   time.Duration // time left until that deadline at the call
}

func (f *fakeLocker) TryLock(context.Context) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.tries++
	if f.tryErr != nil {
		return f.tryErr
	}
	f.held = true
	return nil
}

func (f *fakeLocker) Release(ctx context.Context) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.releases++
	f.held = false
	f.releaseCtxErr = ctx.Err()
	if deadline, ok := ctx.Deadline(); ok {
		f.releaseHasDeadline = true
		f.releaseRemaining = time.Until(deadline)
	}
	return f.releaseErr
}

func (f *fakeLocker) isHeld() bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.held
}

func (f *fakeLocker) counts() (tries, releases int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.tries, f.releases
}

// countingDoffin is a loopback Doffin that answers every search with an empty
// result and counts every request, so a test can say how many calls the probe made.
type countingDoffin struct {
	srv      *httptest.Server
	requests atomic.Int64
}

func newCountingDoffin(t *testing.T) *countingDoffin {
	t.Helper()
	c := &countingDoffin{}
	c.srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		c.requests.Add(1)
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"numHitsTotal":1,"numHitsAccessible":1,"hits":[]}`)
	}))
	t.Cleanup(c.srv.Close)
	return c
}

func (c *countingDoffin) baseURL() string { return c.srv.URL + "/public/v2" }

// client is the real Doffin client pointed at the loopback server.
func (c *countingDoffin) client(t *testing.T) *doffin.Client {
	t.Helper()
	cl, err := newClient(config.Doffin{
		BaseURL:     c.baseURL(),
		SearchRPM:   doffin.MaxSearchRPM,
		DownloadRPM: doffin.MaxDownloadRPM,
		HTTPTimeout: 10 * time.Second,
	}, secret.Value(runKey))
	if err != nil {
		t.Fatalf("newClient: %v", err)
	}
	return cl
}

// useLoopbackDoffin points run at d, with a valid primary key and no secondary key.
func useLoopbackDoffin(t *testing.T, d *countingDoffin) {
	t.Helper()
	t.Setenv("DOFFIN_API_KEY", runKey)
	t.Setenv("DOFFIN_API_KEY_SECONDARY", "")
	t.Setenv("DOFFIN_BASE_URL", d.baseURL())
}

// Another sync holds the budget. The probe must not spend any of it: no Doffin
// call, and exit code 7, the code doffin-sync uses for the same condition.
func TestProbeCallsNothingAndExitsSevenWhenAnotherSyncHoldsTheLock(t *testing.T) {
	d := newCountingDoffin(t)
	locker := &fakeLocker{tryErr: rawstore.ErrLocked}
	var out bytes.Buffer
	p := &probe{out: &out, keys: []keyedClient{{"primary", d.client(t)}}, locker: locker}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	err := p.run(ctx, options{discover: true, samples: 3})

	var stderr bytes.Buffer
	if code := finish(err, &stderr); code != 7 {
		t.Errorf("exit code = %d, want 7; error: %v", code, err)
	}
	const message = "another sync holds the Doffin budget lock; the probe made no Doffin calls"
	if !strings.Contains(stderr.String(), message) {
		t.Errorf("stderr should say %q:\n%s", message, stderr.String())
	}
	if n := d.requests.Load(); n != 0 {
		t.Errorf("Doffin saw %d requests, want none: the probe must not call Doffin without the lock", n)
	}
	if tries, releases := locker.counts(); tries != 1 || releases != 0 {
		t.Errorf("TryLock calls = %d and Release calls = %d, want 1 and 0: nothing was taken, so nothing is released", tries, releases)
	}
	if strings.Contains(out.String(), lockHeldLine) {
		t.Errorf("the output claims a lock the probe does not hold:\n%s", out.String())
	}
}

// The lock is held from before the first Doffin call to after the last, and
// released once, and the header says so.
func TestProbeHoldsTheLockForItsWholeRunAndReleasesItOnSuccess(t *testing.T) {
	locker := &fakeLocker{}
	var searches, downloads int
	client := &fakeNotices{
		search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
			searches++
			if !locker.isHeld() {
				t.Error("a Doffin search ran while the budget lock was not held")
			}
			return response("2026-100001"), nil
		},
		download: func(id string) (doffin.Notice, error) {
			downloads++
			if !locker.isHeld() {
				t.Error("a Doffin download ran while the budget lock was not held")
			}
			return sampleNotice(id, sampleEU, "permalink.mercell.com"), nil
		},
	}
	var out bytes.Buffer
	p := &probe{out: &out, keys: []keyedClient{{"primary", client}}, locker: locker}

	if err := p.run(context.Background(), options{discover: true, samples: 2}); err != nil {
		t.Fatalf("run: %v", err)
	}

	if searches == 0 || downloads == 0 {
		t.Errorf("the run made %d searches and %d downloads, want both kinds so the lock check covers them", searches, downloads)
	}
	if tries, releases := locker.counts(); tries != 1 || releases != 1 {
		t.Errorf("TryLock calls = %d and Release calls = %d, want 1 and 1", tries, releases)
	}
	if locker.isHeld() {
		t.Error("the lock is still held after the run")
	}
	text := out.String()
	if !strings.Contains(text, lockHeldLine+"\n") {
		t.Errorf("the header should say %q:\n%s", lockHeldLine, text)
	}
	if strings.Contains(text, "not taken") || strings.Contains(text, "could not be released") {
		t.Errorf("nothing was wrong with the lock:\n%s", text)
	}
}

func TestProbeReleasesTheLockWhenTheRunFails(t *testing.T) {
	locker := &fakeLocker{}
	failing := &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
		return doffin.SearchResponse{}, &doffin.HTTPError{Operation: "search", Status: 503, Message: "Service Unavailable"}
	}}
	p := &probe{out: &bytes.Buffer{}, keys: []keyedClient{{"primary", failing}}, locker: locker}

	err := p.run(context.Background(), options{discover: true})
	if err == nil {
		t.Fatal("a key check that fails with an outage should end the run with an error")
	}

	if tries, releases := locker.counts(); tries != 1 || releases != 1 {
		t.Errorf("TryLock calls = %d and Release calls = %d, want 1 and 1: the error path must release", tries, releases)
	}
	if locker.isHeld() {
		t.Error("the lock is still held after a failed run")
	}
	if code := finish(err, io.Discard); code != 1 {
		t.Errorf("exit code = %d, want 1 for an ordinary failure", code)
	}
}

// A signal or the time limit ends the run's context. The release must still run,
// on a context that the run's cancellation does not reach and that has its own bound.
func TestProbeReleasesOnAContextThatTheRunCannotCancel(t *testing.T) {
	locker := &fakeLocker{}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	client := &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
		cancel() // the time limit or a signal ends the run during its first call
		return doffin.SearchResponse{}, ctx.Err()
	}}
	p := &probe{out: &bytes.Buffer{}, keys: []keyedClient{{"primary", client}}, locker: locker}

	if err := p.run(ctx, options{}); err == nil {
		t.Fatal("a run whose context ended should return an error")
	}

	locker.mu.Lock()
	defer locker.mu.Unlock()
	if locker.releases != 1 {
		t.Fatalf("Release calls = %d, want 1 although the run's context had ended", locker.releases)
	}
	if locker.releaseCtxErr != nil {
		t.Errorf("the release context was already ended (%v): it must not inherit the run's cancellation", locker.releaseCtxErr)
	}
	if !locker.releaseHasDeadline || locker.releaseRemaining <= 5*time.Second || locker.releaseRemaining > 10*time.Second {
		t.Errorf("release context: deadline %v with %s left, want a bound of 10 seconds", locker.releaseHasDeadline, locker.releaseRemaining)
	}
}

// A lock that cannot be taken for any other reason also stops the probe before its
// first call, and there is nothing to release.
func TestProbeCallsNothingWhenTheLockCannotBeTaken(t *testing.T) {
	locker := &fakeLocker{tryErr: errTakeLock}
	client := &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
		t.Error("a Doffin search ran although the budget lock could not be taken")
		return doffin.SearchResponse{}, nil
	}}
	p := &probe{out: &bytes.Buffer{}, keys: []keyedClient{{"primary", client}}, locker: locker}

	err := p.run(context.Background(), options{discover: true, samples: 3})
	if err == nil || errors.Is(err, errBudgetLocked) {
		t.Fatalf("error = %v, want a failure that is not the held-lock condition", err)
	}

	var stderr bytes.Buffer
	if code := finish(err, &stderr); code != 1 {
		t.Errorf("exit code = %d, want 1", code)
	}
	if !strings.Contains(stderr.String(), "made no Doffin calls") {
		t.Errorf("stderr should say that no Doffin call was made:\n%s", stderr.String())
	}
	if _, releases := locker.counts(); releases != 0 {
		t.Errorf("Release calls = %d, want 0", releases)
	}
}

// A release that fails does not change the result of a probe that succeeded; the
// session is closed either way, and that frees the lock. The error text is not
// printed, because it can quote connection details.
func TestProbeReportsAFailedReleaseWithoutChangingTheResult(t *testing.T) {
	const detail = "release-detail-marker"
	locker := &fakeLocker{releaseErr: errors.New(detail)}
	var out bytes.Buffer
	p := &probe{out: &out, keys: []keyedClient{{"primary", accepting("2026-100001")}}, locker: locker}

	if err := p.run(context.Background(), options{}); err != nil {
		t.Fatalf("a failed release must not fail a probe that succeeded: %v", err)
	}

	text := out.String()
	if !strings.Contains(text, "could not be released cleanly") {
		t.Errorf("the output should say the release failed:\n%s", text)
	}
	if strings.Contains(text, detail) {
		t.Errorf("the output echoed the release error:\n%s", text)
	}
}

// With no ANBUD_DSN the probe takes no lock. It says so in its header, before its
// first step, and carries on as it always did.
func TestProbeSaysSoWhenItTakesNoLock(t *testing.T) {
	var out bytes.Buffer
	p := &probe{out: &out, keys: []keyedClient{{"primary", accepting("2026-100001")}}}

	if err := p.run(context.Background(), options{}); err != nil {
		t.Fatalf("run: %v", err)
	}

	text := out.String()
	const want = "budget lock: not taken (ANBUD_DSN is not set); do not run this while a sync is running\n"
	at := strings.Index(text, want)
	if at < 0 {
		t.Fatalf("the header should say %q:\n%s", want, text)
	}
	if first := strings.Index(text, "[0] keys"); first < at {
		t.Errorf("the lock line should come in the header, before the first step:\n%s", text)
	}
	if strings.Contains(text, lockHeldLine) {
		t.Errorf("the output claims a lock the probe does not hold:\n%s", text)
	}
}

// The same through run, which reads ANBUD_DSN. An unset or blank value means no lock.
func TestRunWithoutADSNTakesNoLockAndCarriesOn(t *testing.T) {
	for _, dsn := range []string{"", "   "} {
		t.Run(fmt.Sprintf("ANBUD_DSN=%q", dsn), func(t *testing.T) {
			d := newCountingDoffin(t)
			useLoopbackDoffin(t, d)
			t.Setenv(envDSN, dsn)

			var out bytes.Buffer
			if err := run([]string{"-discover=false"}, &out); err != nil {
				t.Fatalf("run: %v", err)
			}

			if !strings.Contains(out.String(), "budget lock: not taken (ANBUD_DSN is not set); do not run this while a sync is running") {
				t.Errorf("the header should warn that no lock is taken:\n%s", out.String())
			}
			if n := d.requests.Load(); n == 0 {
				t.Error("the probe should carry on and call Doffin when no lock is asked for")
			}
		})
	}
}

// A DSN that would send the password to a host it cannot verify is refused by the
// rule doffin-sync applies, before any connection and before any Doffin call. The
// refusal names the variable and never a part of the value.
func TestRunRefusesADSNThatDoesNotVerifyTheServerWithoutEchoingIt(t *testing.T) {
	const (
		user     = "probe-user"
		password = "probe-marker-pw"
		host     = "db.example.com"
		database = "probe_db"
	)
	cases := []struct{ name, dsn string }{
		{"a remote host that is not verified", "postgres://" + user + ":" + password + "@" + host + ":5432/" + database + "?sslmode=require"},
		{"a remote host behind a loopback host", "host=127.0.0.1," + host + " user=" + user + " password=" + password + " dbname=" + database + " sslmode=disable"},
		{"a DSN that does not parse", "postgres://" + user + ":" + password + "@" + host + ":notaport/" + database + "?sslmode=require"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			d := newCountingDoffin(t)
			useLoopbackDoffin(t, d)
			t.Setenv(envDSN, c.dsn)

			var out, stderr bytes.Buffer
			err := run([]string{"-discover=false"}, &out)
			code := finish(err, &stderr)

			if err == nil {
				t.Fatal("run accepted a DSN that does not verify the server")
			}
			if code != 1 {
				t.Errorf("exit code = %d, want 1", code)
			}
			if !strings.Contains(err.Error(), "ANBUD_DSN") {
				t.Errorf("the error should name ANBUD_DSN: %v", err)
			}
			shown := map[string]string{"output": out.String(), "error": err.Error(), "stderr": stderr.String()}
			for label, text := range shown {
				for _, leak := range []string{user, password, host, database, c.dsn} {
					if strings.Contains(text, leak) {
						t.Errorf("the %s carries part of the DSN (%q)", label, leak)
					}
				}
			}
			if n := d.requests.Load(); n != 0 {
				t.Errorf("Doffin saw %d requests, want none: a refused DSN stops the probe before any call", n)
			}
		})
	}
}

func TestFinishExitsSevenForAHeldLockAndOneForEveryOtherError(t *testing.T) {
	cases := []struct {
		name string
		err  error
		want int
	}{
		{"no error", nil, 0},
		{"another sync holds the lock", errBudgetLocked, 7},
		{"the same condition, wrapped", fmt.Errorf("the probe stopped: %w", errBudgetLocked), 7},
		{"any other error", errors.New("boom"), 1},
		{"a failed lock that is not a held one", fmt.Errorf("the budget lock could not be taken: %w", errTakeLock), 1},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			var stderr bytes.Buffer
			if got := finish(c.err, &stderr); got != c.want {
				t.Errorf("finish = %d, want %d", got, c.want)
			}
			switch {
			case c.err == nil && stderr.Len() != 0:
				t.Errorf("a clean run printed to stderr: %q", stderr.String())
			case c.err != nil && !strings.HasPrefix(stderr.String(), "doffin-probe: "):
				t.Errorf("stderr = %q, want a line that starts with the command name", stderr.String())
			}
		})
	}
}
