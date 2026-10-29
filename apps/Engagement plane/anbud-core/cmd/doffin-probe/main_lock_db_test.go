package main

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/rawstore"
)

// These tests take the real budget lock on the throwaway database that
// make test-db starts. Without ANBUD_TEST_DSN they skip, as the other database
// tests do. They set ANBUD_DSN themselves, from the database dbtest gives them,
// and run the probe through run, so the wiring from the environment to the lock is
// what is under test. They set environment variables, so they cannot run in parallel.

// dbTimeout bounds each database test's own calls.
const dbTimeout = 60 * time.Second

// A sync holds the budget lock, so the probe exits 7 and the loopback Doffin sees
// no request. Once the sync lets go, the probe runs, holds the lock while it runs
// and gives it back when it ends.
func TestProbeExitsSevenWhileASyncHoldsTheRealLockAndRunsOnceItIsReleased(t *testing.T) {
	pool := dbtest.Open(t)
	store := rawstore.New(pool)
	ctx, cancel := context.WithTimeout(context.Background(), dbTimeout)
	defer cancel()

	d := newCountingDoffin(t)
	useLoopbackDoffin(t, d)
	t.Setenv(envDSN, pool.Config().ConnString())

	// A sync holds the budget.
	syncLock, err := store.TryLock(ctx)
	if err != nil {
		t.Fatalf("the test could not take the budget lock for its stand-in sync: %v", err)
	}
	syncReleased := false
	t.Cleanup(func() {
		if !syncReleased {
			_ = syncLock.Release(context.Background())
		}
	})

	var out, stderr bytes.Buffer
	err = run([]string{"-discover=false"}, &out)
	if code := finish(err, &stderr); code != 7 {
		t.Errorf("exit code while a sync holds the lock = %d, want 7; error: %v", code, err)
	}
	if !strings.Contains(stderr.String(), "another sync holds the Doffin budget lock; the probe made no Doffin calls") {
		t.Errorf("stderr should say that another sync holds the lock:\n%s", stderr.String())
	}
	if n := d.requests.Load(); n != 0 {
		t.Fatalf("Doffin saw %d requests while a sync held the lock, want none", n)
	}

	// The sync lets go, and the probe runs.
	if err := syncLock.Release(ctx); err != nil {
		t.Fatalf("release of the stand-in sync's lock: %v", err)
	}
	syncReleased = true

	out.Reset()
	stderr.Reset()
	err = run([]string{"-discover=false"}, &out)
	if code := finish(err, &stderr); code != 0 {
		t.Fatalf("exit code with the lock free = %d, want 0; error: %v; stderr: %s", code, err, stderr.String())
	}
	if n := d.requests.Load(); n == 0 {
		t.Error("the probe should call Doffin once it holds the lock")
	}
	for _, want := range []string{lockHeldLine + "\n", "Done."} {
		if !strings.Contains(out.String(), want) {
			t.Errorf("output should contain %q:\n%s", want, out.String())
		}
	}

	// The probe gave the budget back on its way out.
	again, err := store.TryLock(ctx)
	if err != nil {
		t.Fatalf("the budget lock was not released when the probe ended: %v", err)
	}
	if err := again.Release(ctx); err != nil {
		t.Errorf("release: %v", err)
	}
}

// While the probe is inside a Doffin call, the lock is held: a sync that tries to
// start then finds the budget taken. The loopback Doffin holds the first request
// open until the test has looked.
func TestProbeHoldsTheRealLockWhileItCallsDoffin(t *testing.T) {
	pool := dbtest.Open(t)
	store := rawstore.New(pool)
	ctx, cancel := context.WithTimeout(context.Background(), dbTimeout)
	defer cancel()

	var (
		requests  atomic.Int64
		started   = make(chan struct{})
		proceed   = make(chan struct{})
		startOnce sync.Once
		freeOnce  sync.Once
	)
	letGo := func() { freeOnce.Do(func() { close(proceed) }) }
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		startOnce.Do(func() { close(started) })
		select {
		case <-proceed:
		case <-r.Context().Done():
		case <-time.After(dbTimeout):
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"numHitsTotal":1,"numHitsAccessible":1,"hits":[]}`)
	}))
	t.Cleanup(srv.Close)
	t.Cleanup(letGo) // runs before srv.Close, so a failed test cannot leave a request open

	t.Setenv(envDSN, pool.Config().ConnString())
	t.Setenv("DOFFIN_API_KEY", runKey)
	t.Setenv("DOFFIN_API_KEY_SECONDARY", "")
	t.Setenv("DOFFIN_BASE_URL", srv.URL+"/public/v2")

	var out bytes.Buffer
	done := make(chan error, 1)
	go func() { done <- run([]string{"-discover=false"}, &out) }()

	select {
	case <-started:
	case err := <-done:
		t.Fatalf("the probe ended before it called Doffin: %v", err)
	case <-time.After(dbTimeout):
		t.Fatal("the probe never called Doffin")
	}

	// The probe is inside its first Doffin call. It must hold the budget lock now.
	if lock, err := store.TryLock(ctx); !errors.Is(err, rawstore.ErrLocked) {
		if err == nil {
			_ = lock.Release(ctx)
		}
		t.Fatalf("TryLock while the probe is calling Doffin: error = %v, want rawstore.ErrLocked", err)
	}

	letGo()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("run: %v", err)
		}
	case <-time.After(dbTimeout):
		t.Fatal("the probe did not finish")
	}

	lock, err := store.TryLock(ctx)
	if err != nil {
		t.Fatalf("the budget lock was not released when the probe ended: %v", err)
	}
	if err := lock.Release(ctx); err != nil {
		t.Errorf("release: %v", err)
	}
}

// A database the probe cannot open stops it before its first call. The message is
// fixed text: pgx errors can quote the connection details, so none is passed on.
func TestProbeCallsNothingWhenTheDatabaseCannotBeOpened(t *testing.T) {
	pool := dbtest.Open(t)
	u, err := url.Parse(pool.Config().ConnString())
	if err != nil {
		t.Fatalf("the test database's DSN does not parse: %v", err)
	}
	password, _ := u.User.Password()
	const missing = "anbud_test_database_that_does_not_exist"
	u.Path = "/" + missing

	d := newCountingDoffin(t)
	useLoopbackDoffin(t, d)
	t.Setenv(envDSN, u.String())

	var out, stderr bytes.Buffer
	err = run([]string{"-discover=false"}, &out)
	code := finish(err, &stderr)

	if code != 1 {
		t.Errorf("exit code = %d, want 1; error: %v", code, err)
	}
	if err == nil || !strings.Contains(err.Error(), "could not open the database; check ANBUD_DSN") {
		t.Errorf("error = %v, want one that says the database could not be opened and names ANBUD_DSN", err)
	}
	if err != nil {
		shown := map[string]string{"output": out.String(), "error": err.Error(), "stderr": stderr.String()}
		for label, text := range shown {
			for _, leak := range []string{password, u.Host, missing, u.String()} {
				if leak != "" && strings.Contains(text, leak) {
					t.Errorf("the %s carries part of the DSN", label)
				}
			}
		}
	}
	if n := d.requests.Load(); n != 0 {
		t.Errorf("Doffin saw %d requests, want none: the probe must not call Doffin without the lock", n)
	}
}
