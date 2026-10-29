package doffin

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"golang.org/x/time/rate"
)

// A 429 can land while another caller is queued behind the token bucket. That
// caller must not spend its token on a request the API has just said it will
// refuse: it re-checks the gate and waits.
func TestWaitForBudgetRechecksTheGateAfterQueueing(t *testing.T) {
	t.Parallel()

	c, _, clock := newTestClient(t, http.NotFoundHandler())
	limiter := rate.NewLimiter(rate.Every(150*time.Millisecond), 1)
	limiter.Allow() // spend the burst so the next Wait genuinely queues
	c.limiters[opSearch] = limiter

	done := make(chan error, 1)
	go func() { done <- c.waitForBudget(context.Background(), opSearch) }()

	time.Sleep(30 * time.Millisecond)
	c.closeGate(opSearch, 20*time.Second) // the 429 arrives while that caller is queued

	if err := <-done; err != nil {
		t.Fatalf("waitForBudget: %v", err)
	}
	if got := clock.total(); got < 20*time.Second {
		t.Fatalf("the queued caller went on after %v of back-off, want at least the 20s the 429 asked for", got)
	}
}

// Cancellation while the limiter is genuinely blocking (no deadline, so the
// limiter does not refuse up front) must end the call promptly.
func TestSearchStopsWaitingWhenCancelledWhileBlockedOnTheBudget(t *testing.T) {
	t.Parallel()

	var calls atomic.Int32
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		_, _ = w.Write([]byte(`{"hits":[]}`))
	}))
	c.limiters[opSearch] = rate.NewLimiter(rate.Every(400*time.Millisecond), 1)

	if _, err := c.Search(context.Background(), SearchParams{}); err != nil {
		t.Fatalf("first Search: %v", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	time.AfterFunc(40*time.Millisecond, cancel)
	start := time.Now()
	_, err := c.Search(ctx, SearchParams{})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want context.Canceled", err)
	}
	if elapsed := time.Since(start); elapsed > 300*time.Millisecond {
		t.Fatalf("cancellation took %v; the call kept waiting for its token", elapsed)
	}
	if calls.Load() != 1 {
		t.Fatalf("the cancelled call reached the API (calls = %d)", calls.Load())
	}
}

func TestParseRetryAfterClampsDatesToo(t *testing.T) {
	t.Parallel()

	now := time.Date(2026, 10, 9, 12, 0, 0, 0, time.UTC)
	if got := parseRetryAfter("Fri, 31 Dec 9999 23:59:59 GMT", now, time.Minute); got != 24*time.Hour {
		t.Fatalf("a date in the year 9999 gave %v, want it clamped to 24h", got)
	}
}

// The probe exists to learn shapes the spec gets wrong, so a body that does not
// decode must still come back, untouched, next to the error.
func TestDecodeFailureStillReturnsTheRawBody(t *testing.T) {
	t.Parallel()

	const body = `{"numHitsTotal":1,"hits":[{"id":"2025-1","heading":{"unexpected":"object"}}]}`
	c, _, _ := newTestClient(t, jsonHandler(body))

	resp, err := c.Search(context.Background(), SearchParams{})
	var decodeErr *DecodeError
	if !errors.As(err, &decodeErr) {
		t.Fatalf("error = %v, want *DecodeError", err)
	}
	if string(resp.Raw) != body {
		t.Fatalf("the raw body was lost on a decode failure: %q", resp.Raw)
	}
	if !strings.Contains(err.Error(), "heading") {
		t.Errorf("the error should say which field did not fit: %v", err)
	}
}

func TestSearchToleratesTheShapeVariantsTheSpecInvites(t *testing.T) {
	t.Parallel()

	const body = `{
	  "numHitsTotal": "12",
	  "numHitsAccessible": 2.0,
	  "hits": [{
	    "id": "2025-1",
	    "buyer": {"id": "b", "organizationId": "974760673", "name": "Buyer"},
	    "locationId": "NO081",
	    "cpvCodes": "90910000",
	    "allTypes": "COMPETITION",
	    "estimatedValue": {"currencyCode": "NOK", "amount": ""},
	    "receivedTenders": "3",
	    "allReceivedTenders": {"type": "all", "total": "3"},
	    "lots": {"heading": "L", "description": "d", "winner": {"id": "w", "organizationId": "1", "name": "W"}}
	  }]
	}`
	c, _, _ := newTestClient(t, jsonHandler(body))

	resp, err := c.Search(context.Background(), SearchParams{})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if resp.NumHitsTotal != 12 || resp.NumHitsAccessible != 2 {
		t.Errorf("totals = %d/%d, want 12/2", resp.NumHitsTotal, resp.NumHitsAccessible)
	}
	if len(resp.Hits) != 1 {
		t.Fatalf("hits = %d", len(resp.Hits))
	}
	hit := resp.Hits[0]
	if len(hit.Buyer) != 1 || hit.Buyer[0].Name != "Buyer" {
		t.Errorf("buyer = %+v", hit.Buyer)
	}
	if len(hit.LocationID) != 1 || hit.LocationID[0] != "NO081" {
		t.Errorf("locationId = %v", hit.LocationID)
	}
	if len(hit.CPVCodes) != 1 || len(hit.AllTypes) != 1 {
		t.Errorf("cpvCodes/allTypes = %v / %v", hit.CPVCodes, hit.AllTypes)
	}
	if hit.EstimatedValue == nil || hit.EstimatedValue.Amount.String() != "" {
		t.Errorf("estimatedValue = %+v", hit.EstimatedValue)
	}
	if hit.ReceivedTenders != 3 || len(hit.AllReceivedTenders) != 1 || hit.AllReceivedTenders[0].Total != 3 {
		t.Errorf("tender counts = %d / %+v", hit.ReceivedTenders, hit.AllReceivedTenders)
	}
	if len(hit.Lots) != 1 || len(hit.Lots[0].Winner) != 1 || hit.Lots[0].Winner[0].Name != "W" {
		t.Errorf("lots = %+v", hit.Lots)
	}
}

func TestHTTPErrorMessagesDropInvisibleFormattingCharacters(t *testing.T) {
	t.Parallel()

	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		// A right-to-left override and a zero-width space: invisible, and able to
		// make "txt.exe" read as something else when pasted into a chat.
		_, _ = w.Write([]byte(`{"message":"safe‮txt.exe ​hidden"}`))
	}))

	_, err := c.Search(context.Background(), SearchParams{})
	if err == nil {
		t.Fatal("expected an error")
	}
	for _, r := range err.Error() {
		if r == '‮' || r == '​' {
			t.Fatalf("the error carries an invisible formatting character %U: %q", r, err)
		}
	}
	if !strings.Contains(err.Error(), "safe") {
		t.Errorf("the visible text should survive: %q", err)
	}
}
