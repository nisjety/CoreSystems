package publicsignals

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func TestCommonCrawlRejectsUnsafeEvidenceAndBoundsRows(t *testing.T) {
	for _, bad := range []string{
		`{"url":"https://foreign.no/","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://blog.triodelab.no/","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://www.triodelab.no/account/private","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://person:secret@triodelab.no/","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no:443/","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no/#private","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no/","timestamp":"20261131123456","status":"200"}`,
		`{"url":"https://triodelab.no/","timestamp":"20261024123456","status":"200"}`,
		`{"url":"https://triodelab.no/","timestamp":"20261020123456","status":"999"}`,
		`{"url":"https://triodelab.no/","timestamp":"20261020123456","status":"0200"}`,
		`{"url":"https://triodelab.no/","timestamp":"20261020123456","status":200}`,
		`{"url":"https://triodelab.no/reset-password/private-token","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no/%2561uth/private-token","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no/%255cprivate","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no/%2500private","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no/%252525252fprivate","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no/%zzprivate","timestamp":"20261020123456","status":"200"}`,
		`{"url":"https://triodelab.no/` + strings.Repeat("x", 2048) + `","timestamp":"20261020123456","status":"200"}`,
		strings.Repeat("x", maxResponse+1), `{"broken":`,
	} {
		s := fixtureService(func(r *http.Request) (*http.Response, error) {
			if r.URL.Path == "/collinfo.json" {
				return response(200, indexes), nil
			}
			return response(200, bad), nil
		})
		got := s.archive(context.Background(), "https://triodelab.no/")
		if got.Status != "unavailable" || got.Reason != "source_unavailable" || len(got.Captures) != 0 {
			t.Fatal("unsafe archive accepted", got)
		}
	}
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/collinfo.json" {
			return response(200, indexes), nil
		}
		return response(200, strings.Repeat(`{"url":"https://triodelab.no/","timestamp":"20261020123456","status":"200"}`+"\n", 21)), nil
	})
	got := s.archive(context.Background(), "https://triodelab.no/")
	if got.Status != "connected" || !got.Truncated || got.Limit != 20 || len(got.Captures) != 1 {
		t.Fatal("unbounded or duplicated captures", got)
	}
	for _, path := range []string{"/auth", "/oauth", "/magic-link", "/sign-up", "/wp-login.php", "/ACCOUNT", "/%2561dmin", "/login;hidden", "/%25xx", "/%c2%80", "/a/../b", "/a/%252e%252e/b", "/./b"} {
		if publicArchivePath(path) {
			t.Fatal("sensitive path accepted", path)
		}
	}
	for _, path := range []string{"/", "/innsikt", "/services/website", "/article%20name"} {
		if !publicArchivePath(path) {
			t.Fatal("public path rejected", path)
		}
	}
}
func TestArchiveIndexSelectionAndNoDataFailures(t *testing.T) {
	for _, body := range []string{`[]`, `[{"id":"CC-MAIN-2026-00"}]`, `[{"id":"CC-MAIN-2099-42"}]`, `[{"id":"../../private"}]`, `{}`, strings.Repeat("x", (1<<20)+1)} {
		s := fixtureService(func(*http.Request) (*http.Response, error) { return response(200, body), nil })
		if got := s.archive(context.Background(), "https://triodelab.no/"); got.Status != "unavailable" || got.Reason != "source_unavailable" {
			t.Fatal("invalid index accepted", got)
		}
	}
	for _, status := range []int{200, 404, 429, 500} {
		s := fixtureService(func(r *http.Request) (*http.Response, error) {
			if r.URL.Path == "/collinfo.json" {
				return response(200, indexes), nil
			}
			return response(status, ""), nil
		})
		got := s.archive(context.Background(), "https://triodelab.no/")
		want := statusReason(status)
		if status == 200 {
			want = "no_data"
		}
		if got.Status != "unavailable" || got.Reason != want || got.Index != "CC-MAIN-2026-42" {
			t.Fatal("failure lost index/reason", got)
		}
	}
}
func TestCrUXMissingMetricAndProviderFailures(t *testing.T) {
	for _, status := range []int{403, 404, 429, 500} {
		s := fixtureService(func(*http.Request) (*http.Response, error) { return response(status, "private-error"), nil })
		got := s.crux(context.Background(), "https://triodelab.no/", "PHONE")
		want := statusReason(status)
		if status == 403 {
			want = "not_configured"
		}
		if got.Status != "unavailable" || got.Reason != want || got.Metrics != nil {
			t.Fatal(got)
		}
	}
	s := fixtureService(func(*http.Request) (*http.Response, error) {
		value := cruxFixture("PHONE")
		var raw map[string]any
		_ = json.Unmarshal([]byte(value), &raw)
		delete(raw["record"].(map[string]any)["metrics"].(map[string]any), "interaction_to_next_paint")
		body, _ := json.Marshal(raw)
		return response(200, string(body)), nil
	})
	got := s.crux(context.Background(), "https://triodelab.no/", "PHONE")
	if got.Status != "connected" || got.Metrics.INPMs != nil || got.Metrics.LCPMs == nil {
		t.Fatal("missing metric fabricated", got)
	}
	for _, invalid := range []struct {
		raw     string
		integer bool
	}{{"null", true}, {"", true}, {"-1", true}, {"1.5", true}, {"9007199254740990.9", true}, {"1e10000000", true}, {"1e-10000000", true}, {`"12"`, true}, {`"Inf"`, false}, {`"-1"`, false}, {"9007199254740992", true}, {strings.Repeat("1", 100), false}} {
		if _, ok := percentile(json.RawMessage(invalid.raw), invalid.integer); ok {
			t.Fatal("invalid percentile accepted", invalid)
		}
	}
}
func TestProviderRedirectsTimeoutsAndFixedEgress(t *testing.T) {
	var calls atomic.Int64
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		calls.Add(1)
		result := response(302, "")
		result.Header.Set("Location", "https://foreign.example/steal")
		return result, nil
	})
	if got := s.crux(context.Background(), "https://triodelab.no/", "PHONE"); got.Status != "unavailable" || calls.Load() != 1 {
		t.Fatal("redirect followed")
	}
	request, _ := http.NewRequest("GET", "https://foreign.example/", nil)
	if _, why := s.request(request, 100); why != "source_unavailable" || calls.Load() != 1 {
		t.Fatal("foreign egress allowed")
	}
	s = fixtureService(func(r *http.Request) (*http.Response, error) { <-r.Context().Done(); return nil, r.Context().Err() })
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	start := time.Now()
	got, err := s.Collect(ctx, "https://triodelab.no/")
	if err != nil || time.Since(start) > time.Second || got.CrUX[0].Status != "unavailable" || got.CommonCrawl.Status != "unavailable" {
		t.Fatal("parent timeout not bounded")
	}
	s = fixtureService(func(*http.Request) (*http.Response, error) { return nil, errors.New("secret transport URL") })
	if got := s.crux(context.Background(), "https://triodelab.no/", "PHONE"); got.Reason != "source_unavailable" {
		t.Fatal(got)
	}
	first, second := New("", nil, nil), New("", nil, nil)
	if first.gate != second.gate || first.gate != commonCrawlGate || first.gate.interval != 2*time.Second || first.client.Jar != nil || first.client.Timeout != 30*time.Second {
		t.Fatal("global gate/client guard missing")
	}
}
func TestCommonCrawlGateSerializesAndSpacesRequests(t *testing.T) {
	gate := &requestGate{interval: 15 * time.Millisecond, slot: make(chan struct{}, 1)}
	var mu sync.Mutex
	var started []time.Time
	var active atomic.Int64
	var wg sync.WaitGroup
	for range 3 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			release, ok := gate.acquire(context.Background())
			if !ok {
				t.Error("gate denied")
				return
			}
			defer release()
			if active.Add(1) != 1 {
				t.Error("parallel request")
			}
			mu.Lock()
			started = append(started, time.Now())
			mu.Unlock()
			time.Sleep(time.Millisecond)
			active.Add(-1)
		}()
	}
	wg.Wait()
	for i := 1; i < len(started); i++ {
		if started[i].Sub(started[i-1]) < gate.interval-time.Millisecond {
			t.Fatal("index requests insufficiently spaced")
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if release, ok := gate.acquire(ctx); ok {
		release()
		t.Fatal("cancelled context admitted")
	}
}

func TestCollectAdmissionIsBoundedAndCancellationReleasesCapacity(t *testing.T) {
	entered := make(chan struct{}, 6)
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		entered <- struct{}{}
		<-r.Context().Done()
		return nil, r.Context().Err()
	})
	ctx, cancel := context.WithCancel(context.Background())
	var wg sync.WaitGroup
	for range 2 {
		wg.Add(1)
		go func() { defer wg.Done(); _, _ = s.Collect(ctx, "https://triodelab.no/") }()
	}
	for range 4 {
		select {
		case <-entered:
		case <-time.After(time.Second):
			t.Fatal("collectors not admitted")
		}
	}
	start := time.Now()
	if _, err := s.Collect(context.Background(), "https://triodelab.no/"); !errors.Is(err, ErrBusy) || time.Since(start) > time.Second {
		t.Fatal("admission not bounded", err)
	}
	cancel()
	wg.Wait()
	if len(s.active) != 0 {
		t.Fatal("cancelled job retained capacity")
	}
}
