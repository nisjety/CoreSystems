package main

import (
	"bytes"
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// Only a rejection says anything about a key. Every other failure must stop the
// probe without blaming the key and without trying the other one, which would
// hit the same problem.
func TestProbeDoesNotBlameTheKeyForAnyOtherFailure(t *testing.T) {
	t.Parallel()

	failures := map[string]error{
		"an outage":            &doffin.HTTPError{Operation: "search", Status: 503, Message: "Service Unavailable"},
		"a dropped connection": errors.New("doffin: search request failed: connection reset by peer"),
		"a 404 on search":      &doffin.HTTPError{Operation: "search", Status: 404},
	}
	for name, failure := range failures {
		t.Run(name, func(t *testing.T) {
			t.Parallel()

			primary := &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
				return doffin.SearchResponse{}, failure
			}}
			secondary := &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
				t.Error("the probe must stop, not try the other key, when the failure says nothing about the key")
				return doffin.SearchResponse{}, nil
			}}
			var out bytes.Buffer
			p := &probe{out: &out, keys: []keyedClient{{"primary", primary}, {"secondary", secondary}}}

			err := p.run(context.Background(), options{})
			if err == nil || !strings.Contains(err.Error(), "could not be checked") {
				t.Fatalf("error = %v, want one saying the key could not be checked", err)
			}
			if strings.Contains(out.String(), "REJECTED") {
				t.Errorf("a failure that is not a rejection was reported as one:\n%s", out.String())
			}
		})
	}
}

func TestProbeTimeoutScalesWithTheSample(t *testing.T) {
	t.Parallel()

	if got := probeTimeout(0); got != 5*time.Minute+150*time.Second {
		t.Errorf("probeTimeout(0) = %s, want 5m30s", got)
	}
	if probeTimeout(12) <= probeTimeout(0) {
		t.Errorf("a larger sample needs a longer limit: %s vs %s", probeTimeout(12), probeTimeout(0))
	}
	if probeTimeout(100) <= probeTimeout(12) {
		t.Errorf("probeTimeout should grow with the sample size")
	}
}
