package publicsignals

import (
	"context"
	"encoding/json"
	"net/http"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

const captureFixture = `{"url":"https://triodelab.no/","timestamp":"20261020123456","status":"200"}`

func TestReadyCrUXEvidenceWinsOverExpiredContext(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	for range 30 {
		results := make(chan deviceResult, 2)
		results <- deviceResult{0, unavailableDevice("PHONE", "no_data")}
		results <- deviceResult{1, unavailableDevice("DESKTOP", "not_configured")}
		got := collectDeviceResults(ctx, results, []Device{unavailableDevice("PHONE", "source_unavailable"), unavailableDevice("DESKTOP", "source_unavailable")})
		if got[0].Reason != "no_data" || got[1].Reason != "not_configured" {
			t.Fatal("completed CrUX result discarded", got)
		}
	}
}

func TestArchiveMetadataFailureDoesNotClaimWebsiteHasNoData(t *testing.T) {
	var calls atomic.Int64
	s := fixtureService(func(*http.Request) (*http.Response, error) {
		calls.Add(1)
		return response(404, "missing metadata"), nil
	})
	got := s.archive(context.Background(), "https://triodelab.no/")
	if got.Reason != "source_unavailable" || got.Index != "" || calls.Load() != 1 {
		t.Fatalf("metadata absence became website no-data: %+v, requests=%d", got, calls.Load())
	}
}

func TestArchiveRetriesOnlyTransientQueryResponsesOnceWithSpacing(t *testing.T) {
	for _, tc := range []struct {
		name       string
		statuses   []int
		body       string
		wantReason string
		wantCalls  int
	}{
		{"gateway timeout recovers", []int{504, 200}, captureFixture, "", 2},
		{"bad gateway recovers", []int{502, 200}, captureFixture, "", 2},
		{"gateway timeout persists", []int{504, 504}, "", "timeout", 2},
		{"bad gateway persists", []int{502, 502}, "", "source_unavailable", 2},
		{"throttle", []int{429}, "", "quota_limited", 1},
		{"overloaded", []int{503}, "", "quota_limited", 1},
		{"no captures", []int{404}, "", "no_data", 1},
		{"malformed captures", []int{200}, "not json", "source_unavailable", 1},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var starts []time.Time
			var query string
			s := fixtureService(func(r *http.Request) (*http.Response, error) {
				if r.URL.Path == "/collinfo.json" {
					return response(200, indexes), nil
				}
				if query != "" && query != r.URL.String() {
					t.Error("retry changed index or scope")
				}
				query = r.URL.String()
				starts = append(starts, time.Now())
				return response(tc.statuses[len(starts)-1], tc.body), nil
			})
			s.gate.interval = 10 * time.Millisecond
			index, why := s.archiveIndex(context.Background())
			if why != "" {
				t.Fatal(why)
			}
			got, _ := s.archiveHost(context.Background(), "https://triodelab.no/", index)
			if got.Reason != tc.wantReason || len(starts) != tc.wantCalls || got.Index != "CC-MAIN-2026-42" {
				t.Fatalf("got=%+v calls=%d", got, len(starts))
			}
			if len(starts) == 2 && starts[1].Sub(starts[0]) < s.gate.interval-time.Millisecond {
				t.Fatal("retry bypassed pacing gate")
			}
		})
	}
}

func TestArchiveDoesNotRetryWhenDeadlineBudgetCannotFitPacing(t *testing.T) {
	var calls atomic.Int64
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/collinfo.json" {
			return response(200, indexes), nil
		}
		calls.Add(1)
		return response(504, ""), nil
	})
	// Metadata is cached first so only the query consumes the short budget.
	_ = s.archive(context.Background(), "https://triodelab.no/")
	calls.Store(0)
	s.gate = &requestGate{interval: 2 * time.Second, slot: make(chan struct{}, 1)}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	got := s.archive(ctx, "https://triodelab.no/")
	if got.Reason != "timeout" || calls.Load() != 1 {
		t.Fatalf("got=%+v calls=%d", got, calls.Load())
	}
}

func TestProviderDeadlinesRemainTimeoutNotMissingData(t *testing.T) {
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		<-r.Context().Done()
		return nil, r.Context().Err()
	})
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Millisecond)
	defer cancel()
	got, err := s.Collect(ctx, "https://triodelab.no/")
	if err != nil || got.CommonCrawl.Reason != "timeout" || got.CrUX[0].Reason != "timeout" || got.CrUX[1].Reason != "timeout" {
		t.Fatalf("deadlines lost: %+v err=%v", got, err)
	}
}

func TestCollectDeadlineDoesNotOverwriteCompletedSourceFailure(t *testing.T) {
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		if r.URL.Host == "chromeuxreport.googleapis.com" {
			var body struct{ FormFactor string }
			_ = json.NewDecoder(r.Body).Decode(&body)
			if body.FormFactor == "PHONE" {
				return response(500, ""), nil
			}
		}
		<-r.Context().Done()
		return nil, r.Context().Err()
	})
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	got, err := s.Collect(ctx, "https://triodelab.no/")
	if err != nil || got.CrUX[0].Reason != "source_unavailable" || got.CrUX[1].Reason != "timeout" {
		t.Fatalf("completed failure overwritten: %+v err=%v", got, err)
	}
}

func TestCollectPreservesCompletedArchiveWhenCrUXTimesOut(t *testing.T) {
	for range 20 {
		s := fixtureService(func(r *http.Request) (*http.Response, error) {
			if r.URL.Host == "chromeuxreport.googleapis.com" {
				<-r.Context().Done()
				return nil, r.Context().Err()
			}
			if r.URL.Path == "/collinfo.json" {
				return response(200, indexes), nil
			}
			return response(200, captureFixture), nil
		})
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
		got, err := s.Collect(ctx, "https://triodelab.no/")
		cancel()
		if err != nil || got.CommonCrawl.Status != "connected" || got.CommonCrawl.Index != "CC-MAIN-2026-42" {
			t.Fatalf("completed archive discarded: %+v err=%v", got, err)
		}
	}
}

func TestCollectPreservesSelectedArchiveIndexAtQueryDeadline(t *testing.T) {
	for range 20 {
		s := fixtureService(func(r *http.Request) (*http.Response, error) {
			if r.URL.Host == "chromeuxreport.googleapis.com" {
				var body struct{ FormFactor string }
				_ = json.NewDecoder(r.Body).Decode(&body)
				return response(200, cruxFixture(body.FormFactor)), nil
			}
			if r.URL.Path == "/collinfo.json" {
				return response(200, indexes), nil
			}
			<-r.Context().Done()
			return nil, r.Context().Err()
		})
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
		got, err := s.Collect(ctx, "https://triodelab.no/")
		cancel()
		if err != nil || got.CommonCrawl.Reason != "timeout" || got.CommonCrawl.Index != "CC-MAIN-2026-42" {
			t.Fatalf("selected index lost: %+v err=%v", got, err)
		}
	}
}

func TestArchiveMetadataCacheCoalescesConcurrentReaders(t *testing.T) {
	var metadata, queries atomic.Int64
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/collinfo.json" {
			metadata.Add(1)
			return response(200, indexes), nil
		}
		queries.Add(1)
		return response(200, captureFixture), nil
	})
	var wg sync.WaitGroup
	for range 8 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if got := s.archive(context.Background(), "https://triodelab.no/"); got.Status != "connected" {
				t.Errorf("got=%+v", got)
			}
		}()
	}
	wg.Wait()
	if metadata.Load() != 1 || queries.Load() != 8 {
		t.Fatalf("metadata=%d queries=%d", metadata.Load(), queries.Load())
	}
}

func TestArchiveExpiredMetadataIsNotUsedWhenRefreshFails(t *testing.T) {
	var metadata, queries atomic.Int64
	var now atomic.Int64
	now.Store(observed.Unix())
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/collinfo.json" {
			if metadata.Add(1) == 2 {
				return response(503, ""), nil
			}
			return response(200, indexes), nil
		}
		queries.Add(1)
		return response(200, captureFixture), nil
	})
	s.now = func() time.Time { return time.Unix(now.Load(), 0) }
	if got := s.archive(context.Background(), "https://triodelab.no/"); got.Status != "connected" {
		t.Fatal(got)
	}
	now.Add(int64((29 * time.Minute) / time.Second))
	if got := s.archive(context.Background(), "https://triodelab.no/"); got.Status != "connected" || metadata.Load() != 1 {
		t.Fatal("fresh metadata not reused", got)
	}
	now.Add(int64((time.Minute) / time.Second))
	if got := s.archive(context.Background(), "https://triodelab.no/"); got.Reason != "quota_limited" || got.Index != "" || queries.Load() != 2 {
		t.Fatal("expired index used after failed refresh", got)
	}
	if got := s.archive(context.Background(), "https://triodelab.no/"); got.Status != "connected" || metadata.Load() != 3 {
		t.Fatal("failed refresh cached", got)
	}
}
