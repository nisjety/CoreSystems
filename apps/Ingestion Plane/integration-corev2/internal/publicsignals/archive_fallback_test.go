package publicsignals

import (
	"context"
	"net/http"
	"strings"
	"testing"
)

func TestOlderArchiveOnlyReplacesTimeoutWithActualEvidence(t *testing.T) {
	for _, olderStatus := range []int{200, 404} {
		t.Run(http.StatusText(olderStatus), func(t *testing.T) {
			olderCalls := 0
			s := fixtureService(func(r *http.Request) (*http.Response, error) {
				if r.URL.Path == "/collinfo.json" {
					return response(200, indexes), nil
				}
				if strings.Contains(r.URL.Path, "2026-38") {
					olderCalls++
					return response(olderStatus, captureFixture), nil
				}
				return response(504, ""), nil
			})
			got := s.archive(context.Background(), "https://triodelab.no/")
			if olderCalls != 1 {
				t.Fatalf("expected one bounded older archive request; got %d", olderCalls)
			}
			if olderStatus == 200 {
				if got.Status != "connected" || got.Index != "CC-MAIN-2026-38" || len(got.Captures) != 1 {
					t.Fatal("fallback lost actual index provenance", got)
				}
			} else if got.Reason != "timeout" || got.Index != "CC-MAIN-2026-42" {
				t.Fatal("older no-data erased current archive uncertainty", got)
			}
		})
	}
}

func TestArchiveThrottleDoesNotTryOtherCollections(t *testing.T) {
	calls := 0
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/collinfo.json" {
			return response(200, indexes), nil
		}
		calls++
		return response(429, ""), nil
	})
	got := s.archive(context.Background(), "https://triodelab.no/")
	if calls != 1 || got.Reason != "quota_limited" {
		t.Fatal("backpressure bypassed", got, calls)
	}
}
