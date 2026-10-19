package main

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestStartupGate_RefusesWithoutDatabaseUnlessEphemeralIsExactlyTrue(t *testing.T) {
	for _, tc := range []struct {
		name, url, flag string
		refused         bool
	}{
		{"no database, no flag", "", "", true},
		{"no database, flag false", "", "false", true},
		{"no database, flag 1", "", "1", true},
		{"no database, flag TRUE", "", "TRUE", true},
		{"no database, flag with space", "", " true", true},
		{"no database, flag true", "", "true", false},
		{"database, no flag", "postgres://db/x", "", false},
		{"database, flag true", "postgres://db/x", "true", false},
	} {
		err := startupGate(tc.url, tc.flag)
		if tc.refused != (err != nil) {
			t.Errorf("%s: err = %v, refused = %v", tc.name, err, tc.refused)
		}
		if tc.refused && !errors.Is(err, errStartupRefused) {
			t.Errorf("%s: err = %v", tc.name, err)
		}
	}
}

func TestEphemeralDevelopmentEnabled_OnlyExactTrue(t *testing.T) {
	for v, want := range map[string]bool{"true": true, "True": false, "1": false, "": false, "yes": false} {
		if got := ephemeralDevelopmentEnabled(v); got != want {
			t.Errorf("%q = %v, want %v", v, got, want)
		}
	}
}

func TestReadyz_503WhenTheDatabaseDoesNotAnswer(t *testing.T) {
	serve := func(ready func(context.Context) error) *httptest.ResponseRecorder {
		rec := httptest.NewRecorder()
		readyzHandler(ready).ServeHTTP(rec, httptest.NewRequest(http.MethodGet, "/readyz", nil))
		return rec
	}
	if rec := serve(nil); rec.Code != http.StatusOK || rec.Body.String() != "ok" {
		t.Fatalf("ephemeral development: %d %q", rec.Code, rec.Body.String())
	}
	if rec := serve(func(context.Context) error { return nil }); rec.Code != http.StatusOK {
		t.Fatalf("healthy database: %d", rec.Code)
	}
	rec := serve(func(context.Context) error { return errors.New("dial tcp 10.0.0.5: connection refused, password=s3cret") })
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("failing database: %d, want 503", rec.Code)
	}
	if strings.Contains(rec.Body.String(), "s3cret") || strings.Contains(rec.Body.String(), "10.0.0.5") {
		t.Fatalf("readyz leaked the database error: %q", rec.Body.String())
	}
	// A database that hangs is cut off at the 2 s readiness deadline.
	var deadline time.Time
	hung := serve(func(ctx context.Context) error {
		deadline, _ = ctx.Deadline()
		<-ctx.Done()
		return ctx.Err()
	})
	if hung.Code != http.StatusServiceUnavailable {
		t.Fatalf("hung database: %d, want 503", hung.Code)
	}
	if deadline.IsZero() {
		t.Fatal("the readiness check has no deadline")
	}
}

func TestNewPool_MalformedURLErrorDoesNotQuoteTheURL(t *testing.T) {
	const secret = "s3cret-pw"
	_, err := newPool(context.Background(), "postgres://user:"+secret+"@host:notaport/db")
	if !errors.Is(err, errDatabaseURLInvalid) {
		t.Fatalf("err = %v", err)
	}
	if strings.Contains(err.Error(), secret) || strings.Contains(err.Error(), "host") {
		t.Fatalf("error quotes the URL: %q", err.Error())
	}
	pool, err := newPool(context.Background(), "postgres://user:pw@127.0.0.1:1/db")
	if err != nil {
		t.Fatalf("a well-formed URL must not connect eagerly: %v", err)
	}
	pool.Close()
}

type fakePurger struct {
	cutoff time.Time
	calls  int
	err    error
}

func (p *fakePurger) PurgeExpired(_ context.Context, before time.Time) (int64, error) {
	p.calls++
	p.cutoff = before
	return 3, p.err
}

func TestPurge_CadenceAndRetention(t *testing.T) {
	if purgeInterval != 10*time.Minute || purgeRetention != 24*time.Hour {
		t.Fatalf("purge cadence %v, retention %v; want 10m and 24h", purgeInterval, purgeRetention)
	}
	now := time.Date(2026, 10, 19, 12, 0, 0, 0, time.UTC)
	p := &fakePurger{}
	purgeOnce(context.Background(), p, now)
	if p.calls != 1 || !p.cutoff.Equal(now.Add(-24*time.Hour)) {
		t.Fatalf("calls %d, cutoff %v", p.calls, p.cutoff)
	}
	// A failing purge is logged and survived, not fatal.
	p.err = errors.New("db down")
	purgeOnce(context.Background(), p, now)
	if p.calls != 2 {
		t.Fatalf("calls %d", p.calls)
	}
}
