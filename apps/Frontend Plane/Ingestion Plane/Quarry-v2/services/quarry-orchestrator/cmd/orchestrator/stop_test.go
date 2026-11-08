package main

import (
	"strings"
	"testing"
	"time"
)

// WS6.J (stop timeout only): the per-worker stop bound is read from
// QUARRY_ORCH_WORKER_STOP_TIMEOUT_S; main waits for the worker within 2*bound+slack.

func envFrom(values map[string]string) func(string) string {
	return func(key string) string { return values[key] }
}

func TestLoadWorkerStopTimeout(t *testing.T) {
	cases := []struct {
		name    string
		raw     string
		want    time.Duration
		wantErr bool
	}{
		{name: "unset is the default", raw: "", want: 20 * time.Second},
		{name: "blank is unset", raw: "   ", want: 20 * time.Second},
		{name: "lower bound", raw: "1", want: time.Second},
		{name: "upper bound", raw: "25", want: 25 * time.Second},
		{name: "zero is refused", raw: "0", wantErr: true},
		{name: "above the bound is refused", raw: "26", wantErr: true},
		{name: "negative is refused", raw: "-5", wantErr: true},
		{name: "not a number is refused", raw: "thirty", wantErr: true},
		{name: "fraction is refused", raw: "1.5", wantErr: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := loadWorkerStopTimeout(envFrom(map[string]string{envWorkerStopTimeout: tc.raw}))
			if tc.wantErr {
				if err == nil {
					t.Fatalf("raw %q: want an error, got %v", tc.raw, got)
				}
				if !strings.Contains(err.Error(), envWorkerStopTimeout) {
					t.Fatalf("raw %q: error %q does not name %s", tc.raw, err, envWorkerStopTimeout)
				}
				return
			}
			if err != nil {
				t.Fatalf("raw %q: unexpected error %v", tc.raw, err)
			}
			if got != tc.want {
				t.Fatalf("raw %q: got %v want %v", tc.raw, got, tc.want)
			}
		})
	}
}

func TestAwaitWorkerStopReturnsWhenTheWorkerStops(t *testing.T) {
	workerErr := make(chan error, 1)
	workerErr <- nil
	done := make(chan struct{})
	go func() {
		awaitWorkerStop(workerErr, time.Minute)
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("awaitWorkerStop did not return after the worker stopped")
	}
}

func TestAwaitWorkerStopGivesUpAfterTheBound(t *testing.T) {
	workerErr := make(chan error) // a worker that never returns
	done := make(chan struct{})
	go func() {
		awaitWorkerStop(workerErr, 20*time.Millisecond)
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("awaitWorkerStop waited past its bound")
	}
}
