package doffin

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestCountAcceptsNumbersStringsAndNull(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		in      string
		want    Count
		wantErr bool
	}{
		{"number", `3`, 3, false},
		{"numeric string", `"4"`, 4, false},
		{"null", `null`, 0, false},
		{"empty string", `""`, 0, false},
		{"float", `5.0`, 5, false},
		{"not a number", `"many"`, 0, true},
		{"array", `[1]`, 0, true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			var got Count
			err := json.Unmarshal([]byte(tc.in), &got)
			if (err != nil) != tc.wantErr {
				t.Fatalf("error = %v, wantErr %v", err, tc.wantErr)
			}
			if !tc.wantErr && got != tc.want {
				t.Fatalf("Count = %d, want %d", got, tc.want)
			}
		})
	}
}

func TestSearchResponseRejectsMalformedEnvelopes(t *testing.T) {
	t.Parallel()

	tests := map[string]string{
		"not json":             `[`,
		"hits is a string":     `{"hits":"none"}`,
		"hits is a number":     `{"hits":7}`,
		"hit is not an object": `{"hits":[1]}`,
		"single hit is bad":    `{"hits":{"receivedTenders":"many"}}`,
	}
	for name, body := range tests {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			var resp SearchResponse
			if err := json.Unmarshal([]byte(body), &resp); err == nil {
				t.Fatalf("expected an error for %s", body)
			}
		})
	}
}

func TestErrorMessages(t *testing.T) {
	t.Parallel()

	withMessage := &HTTPError{Operation: "search", Status: 400, Message: "Invalid sortBy"}
	if got := withMessage.Error(); !strings.Contains(got, "400") || !strings.Contains(got, "Invalid sortBy") {
		t.Errorf("Error() = %q", got)
	}
	bare := &HTTPError{Operation: "download", Status: 502}
	if got := bare.Error(); !strings.Contains(got, "502") || strings.Contains(got, ":") && strings.HasSuffix(got, ": ") {
		t.Errorf("Error() = %q", got)
	}
	limited := &RateLimitedError{Operation: "search", RetryAfter: 90 * time.Second}
	if got := limited.Error(); !strings.Contains(got, "search") || !strings.Contains(got, "1m30s") {
		t.Errorf("RateLimitedError = %q", got)
	}
}

func TestHTTPErrorOnlyMatchesItsOwnSentinels(t *testing.T) {
	t.Parallel()

	server := &HTTPError{Operation: "search", Status: http.StatusInternalServerError}
	for _, target := range []error{ErrUnauthorized, ErrNotFound, ErrInvalidParams} {
		if errors.Is(server, target) {
			t.Errorf("a 500 must not match %v", target)
		}
	}
	if !errors.Is(&HTTPError{Status: http.StatusUnauthorized}, ErrUnauthorized) ||
		!errors.Is(&HTTPError{Status: http.StatusForbidden}, ErrUnauthorized) ||
		!errors.Is(&HTTPError{Status: http.StatusNotFound}, ErrNotFound) {
		t.Error("401/403 should match ErrUnauthorized and 404 should match ErrNotFound")
	}
}

func TestSleepContext(t *testing.T) {
	t.Parallel()

	if err := sleepContext(context.Background(), time.Millisecond); err != nil {
		t.Fatalf("a short sleep should finish cleanly: %v", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	start := time.Now()
	if err := sleepContext(ctx, time.Hour); !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want context.Canceled", err)
	}
	if time.Since(start) > time.Second {
		t.Fatal("a cancelled sleep must return promptly")
	}
}

func TestParseRetryAfterClampsAbsurdValues(t *testing.T) {
	t.Parallel()

	now := time.Date(2026, 10, 9, 12, 0, 0, 0, time.UTC)
	if got := parseRetryAfter("999999999999", now, time.Minute); got != 24*time.Hour {
		t.Fatalf("parseRetryAfter = %v, want it clamped to 24h", got)
	}
}
