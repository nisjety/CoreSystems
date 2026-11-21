package consumers

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
)

// orderLog records the order in which the purgers of one erasure ran, across every store and provider step.
type orderLog struct {
	mu     sync.Mutex
	events []string
}

func (o *orderLog) add(event string) {
	o.mu.Lock()
	defer o.mu.Unlock()
	o.events = append(o.events, event)
}

func (o *orderLog) snapshot() []string {
	o.mu.Lock()
	defer o.mu.Unlock()
	return append([]string(nil), o.events...)
}

// recordingPurger notes its name in a shared orderLog every time it is called. It returns the counts and error a test sets.
type recordingPurger struct {
	name   string
	order  *orderLog
	mu     sync.Mutex
	counts map[string]int64
	err    error
}

func (r *recordingPurger) PurgeOrganization(_ context.Context, _ string) (map[string]int64, error) {
	r.order.add(r.name)
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.err != nil {
		return nil, r.err
	}
	return r.counts, nil
}

func (r *recordingPurger) setErr(err error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.err = err
}

// The provider-side erasure runs before every local store. The membership rows that name each provider subscriber are
// local, so a local store purged first would take them away before the provider step could read them.
func TestOrgErasure_ProviderStepRunsBeforeEveryLocalPurge(t *testing.T) {
	order := &orderLog{}
	provider := &recordingPurger{name: "provider", order: order, counts: map[string]int64{"novu_subscribers": 1, "novu_subscriber_messages": 2}}
	rows := &recordingPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 2}}
	pushes := &recordingPurger{name: "push", order: order, counts: map[string]int64{"notification_web_push_subscriptions": 1}}
	s := NewOrgErasureSubscriber(nil, rows, pushes).WithProviderSteps(provider)
	logged := captureLog(t)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck", got)
	}
	if got, want := order.snapshot(), []string{"provider", "rows", "push"}; strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("purge order = %v, want %v: the provider step must run first", got, want)
	}
	line := logged.String()
	if !strings.Contains(line, "novu_subscribers=1") || !strings.Contains(line, "novu_subscriber_messages=2") || !strings.Contains(line, "notification_requests=2") {
		t.Fatalf("log = %q, want the provider and local counts together", line)
	}
}

// A failed provider step keeps every local row in place, so the retry can find the memberships again. No local store is
// touched, and the message retries.
func TestOrgErasure_ProviderFailure_LeavesEveryLocalStoreUntouched(t *testing.T) {
	order := &orderLog{}
	provider := &recordingPurger{name: "provider", order: order, err: errors.New("novu unavailable")}
	rows := &recordingPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 2}}
	pushes := &recordingPurger{name: "push", order: order}
	s := NewOrgErasureSubscriber(nil, rows, pushes).WithProviderSteps(provider)
	logged := captureLog(t)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureRetry {
		t.Fatalf("outcome = %v, want orgErasureRetry for a provider-side failure", got)
	}
	if got := order.snapshot(); len(got) != 1 || got[0] != "provider" {
		t.Fatalf("purge order = %v, want only the provider step: a local store ran while the provider step failed", got)
	}
	if !strings.Contains(logged.String(), "local rows kept for retry") {
		t.Fatalf("log = %q, want the provider-failure line", logged.String())
	}
}

// Redelivery after a provider step has succeeded and a local store has failed runs the provider step again. Each
// provider step is idempotent, so the second run removes nothing and fails nothing, and the message then acks.
func TestOrgErasure_LocalFailureAfterProvider_RetryRerunsTheProviderStep(t *testing.T) {
	order := &orderLog{}
	provider := &recordingPurger{name: "provider", order: order, counts: map[string]int64{"novu_subscribers": 1}}
	rows := &recordingPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 1}, err: errors.New("database unavailable")}
	s := NewOrgErasureSubscriber(nil, rows).WithProviderSteps(provider)
	captureLog(t)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureRetry {
		t.Fatalf("first outcome = %v, want orgErasureRetry for the local failure", got)
	}
	rows.setErr(nil)
	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureAck {
		t.Fatalf("redelivered outcome = %v, want orgErasureAck", got)
	}
	want := []string{"provider", "rows", "provider", "rows"}
	if got := order.snapshot(); strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("purge order = %v, want %v: the provider step must run again on redelivery", got, want)
	}
}

// The provider step is told the erased organization, the same as a local store.
func TestOrgErasure_ProviderStepReceivesTheErasedOrganization(t *testing.T) {
	provider := &fakeOrgPurger{}
	s := NewOrgErasureSubscriber(nil, &fakeOrgPurger{}).WithProviderSteps(provider)
	captureLog(t)

	if got := s.process(context.Background(), organizationErasure("org-7")); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck", got)
	}
	if calls := provider.calls(); len(calls) != 1 || calls[0] != "org-7" {
		t.Fatalf("provider calls = %v, want exactly one for org-7", calls)
	}
}

// Without provider steps the subscriber behaves as it did before: every local store runs, and a failure retries.
func TestOrgErasure_WithoutProviderSteps_LocalStoresRunAsBefore(t *testing.T) {
	order := &orderLog{}
	rows := &recordingPurger{name: "rows", order: order}
	pushes := &recordingPurger{name: "push", order: order, err: errors.New("push store unavailable")}
	s := NewOrgErasureSubscriber(nil, rows, pushes).WithProviderSteps()
	captureLog(t)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureRetry {
		t.Fatalf("outcome = %v, want orgErasureRetry", got)
	}
	if got, want := order.snapshot(), []string{"rows", "push"}; strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("purge order = %v, want %v with no provider step", got, want)
	}
}
