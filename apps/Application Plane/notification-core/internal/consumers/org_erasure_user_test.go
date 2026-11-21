package consumers

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
)

// fakeUserPurger records every user it is asked to erase, and returns the counts and error a test sets.
type fakeUserPurger struct {
	mu      sync.Mutex
	userIDs []string
	counts  map[string]int64
	err     error
}

func (f *fakeUserPurger) PurgeUser(_ context.Context, userID string) (map[string]int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.userIDs = append(f.userIDs, userID)
	if f.err != nil {
		return nil, f.err
	}
	return f.counts, nil
}

func (f *fakeUserPurger) calls() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]string(nil), f.userIDs...)
}

// recordingUserPurger notes its name in a shared orderLog every time it is called, as recordingPurger does for organizations.
// It returns the counts and error a test sets.
type recordingUserPurger struct {
	name   string
	order  *orderLog
	mu     sync.Mutex
	counts map[string]int64
	err    error
}

func (r *recordingUserPurger) PurgeUser(_ context.Context, _ string) (map[string]int64, error) {
	r.order.add(r.name)
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.err != nil {
		return nil, r.err
	}
	return r.counts, nil
}

func (r *recordingUserPurger) setErr(err error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.err = err
}

// userErasure is the wire shape user-core publishes for one fan-out child of a user erasure.
func userErasure(subjectType, userID, orgID string) orgErasureEvent {
	return orgErasureEvent{SubjectType: subjectType, SubjectID: userID, OrgID: orgID, RequestedBy: "actor-owner"}
}

// Both per-user subject types erase the user's rows through the user store, and no organization store runs.
func TestOrgErasure_UserEvent_PurgesTheUserAndAcks(t *testing.T) {
	for _, subjectType := range []string{"user", "user_anonymize"} {
		orgs := &fakeOrgPurger{}
		users := &fakeUserPurger{counts: map[string]int64{"notification_requests": 2}}
		s := NewOrgErasureSubscriber(nil, orgs).WithUserErasure("disabled", nil, users)
		captureLog(t)

		if got := s.process(context.Background(), userErasure(subjectType, "user-1", "org-1")); got != orgErasureAck {
			t.Fatalf("%s outcome = %v, want orgErasureAck", subjectType, got)
		}
		if calls := users.calls(); len(calls) != 1 || calls[0] != "user-1" {
			t.Fatalf("%s user purge calls = %v, want exactly one for user-1", subjectType, calls)
		}
		if calls := orgs.calls(); len(calls) != 0 {
			t.Fatalf("%s erasure purged organization %v, want no organization purge", subjectType, calls)
		}
	}
}

// The user is named by subject_id. The org_id names the fan-out child, not the user, so it is never purged as a user, and a
// user event with no subject_id acks without a purge rather than falling back to org_id.
func TestOrgErasure_UserEvent_IsKeyedByTheUserNotTheOrganization(t *testing.T) {
	users := &fakeUserPurger{}
	s := NewOrgErasureSubscriber(nil).WithUserErasure("disabled", nil, users)
	logged := captureLog(t)

	if got := s.process(context.Background(), orgErasureEvent{SubjectType: "user", OrgID: "org-9"}); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck for a user event with no user id", got)
	}
	if got := s.process(context.Background(), orgErasureEvent{SubjectType: "user_anonymize", SubjectID: "   ", OrgID: "org-9"}); got != orgErasureAck {
		t.Fatalf("blank-subject outcome = %v, want orgErasureAck", got)
	}
	if calls := users.calls(); len(calls) != 0 {
		t.Fatalf("user purge calls = %v, want none: the organization id must never stand in for a user", calls)
	}
	if !strings.Contains(logged.String(), "user erasure without a user id") {
		t.Fatalf("log = %q, want the no-user-id line", logged.String())
	}
}

// user-core sends one event per organization the user belongs to. Each child erases the user everywhere, the org_id does not
// scope the purge, and a replayed child removes nothing and still acks.
func TestOrgErasure_UserFanoutChildren_EachAcksAndTheOrgIdDoesNotScopeThePurge(t *testing.T) {
	users := &fakeUserPurger{}
	s := NewOrgErasureSubscriber(nil).WithUserErasure("disabled", nil, users)
	captureLog(t)

	for _, orgID := range []string{"org-a", "org-b", "org-a"} {
		if got := s.process(context.Background(), userErasure("user", "user-1", orgID)); got != orgErasureAck {
			t.Fatalf("child for %s outcome = %v, want orgErasureAck", orgID, got)
		}
	}
	calls := users.calls()
	if len(calls) != 3 || calls[0] != "user-1" || calls[1] != "user-1" || calls[2] != "user-1" {
		t.Fatalf("user purge calls = %v, want each of the three children to purge user-1", calls)
	}
}

// In novu mode the provider-side step runs before every local store, so the memberships that name the provider subscribers are
// still there when it reads them.
func TestOrgErasure_UserNovuMode_ProviderStepRunsBeforeEveryLocalPurge(t *testing.T) {
	logged := captureLog(t)
	order := &orderLog{}
	provider := &recordingUserPurger{name: "provider", order: order, counts: map[string]int64{"novu_subscribers": 2, "novu_subscriber_messages": 5}}
	rows := &recordingUserPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 2}}
	pushes := &recordingUserPurger{name: "push", order: order, counts: map[string]int64{"notification_web_push_subscriptions": 1}}
	s := NewOrgErasureSubscriber(nil).WithUserErasure("novu", provider, rows, pushes)

	if got := s.process(context.Background(), userErasure("user", "user-1", "org-1")); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck", got)
	}
	if got, want := order.snapshot(), []string{"provider", "rows", "push"}; strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("purge order = %v, want %v: the provider step must run first", got, want)
	}
	line := logged.String()
	if !strings.Contains(line, "novu_subscribers=2") || !strings.Contains(line, "notification_requests=2") {
		t.Fatalf("log = %q, want the provider and local counts together", line)
	}
}

// A failed provider step keeps every local row in place, so the retry can still find the memberships. No local store is
// touched, and the message retries.
func TestOrgErasure_UserNovuMode_ProviderFailureLeavesEveryLocalStoreUntouched(t *testing.T) {
	logged := captureLog(t)
	order := &orderLog{}
	provider := &recordingUserPurger{name: "provider", order: order, err: errors.New("novu unavailable")}
	rows := &recordingUserPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 2}}
	pushes := &recordingUserPurger{name: "push", order: order}
	s := NewOrgErasureSubscriber(nil).WithUserErasure("novu", provider, rows, pushes)

	if got := s.process(context.Background(), userErasure("user", "user-1", "org-1")); got != orgErasureRetry {
		t.Fatalf("outcome = %v, want orgErasureRetry for a provider-side failure", got)
	}
	if got := order.snapshot(); len(got) != 1 || got[0] != "provider" {
		t.Fatalf("purge order = %v, want only the provider step: a local store ran while the provider step failed", got)
	}
	if !strings.Contains(logged.String(), "local rows kept for retry") {
		t.Fatalf("log = %q, want the provider-failure line", logged.String())
	}
}

// Redelivery after the provider step succeeded and a local store failed runs the provider step again. Both steps are idempotent,
// so the second pass removes nothing that the first left and the message acks.
func TestOrgErasure_UserLocalFailure_RetryReRunsTheProviderStep(t *testing.T) {
	captureLog(t)
	order := &orderLog{}
	provider := &recordingUserPurger{name: "provider", order: order, counts: map[string]int64{"novu_subscribers": 1}}
	rows := &recordingUserPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 1}, err: errors.New("database unavailable")}
	s := NewOrgErasureSubscriber(nil).WithUserErasure("novu", provider, rows)

	if got := s.process(context.Background(), userErasure("user", "user-1", "org-1")); got != orgErasureRetry {
		t.Fatalf("first outcome = %v, want orgErasureRetry for the local failure", got)
	}
	rows.setErr(nil)
	if got := s.process(context.Background(), userErasure("user", "user-1", "org-1")); got != orgErasureAck {
		t.Fatalf("redelivered outcome = %v, want orgErasureAck", got)
	}
	want := []string{"provider", "rows", "provider", "rows"}
	if got := order.snapshot(); strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("purge order = %v, want %v: the provider step must run again on redelivery", got, want)
	}
}

// One failing local store does not stop the others, and any failure makes the whole message retry.
func TestOrgErasure_UserPurgeError_RetriesAndStillRunsEveryLocalStore(t *testing.T) {
	failing := &fakeUserPurger{err: errors.New("database unavailable")}
	healthy := &fakeUserPurger{counts: map[string]int64{"notification_requests": 1}}
	s := NewOrgErasureSubscriber(nil).WithUserErasure("disabled", nil, failing, healthy)
	captureLog(t)

	if got := s.process(context.Background(), userErasure("user", "user-1", "org-1")); got != orgErasureRetry {
		t.Fatalf("outcome = %v, want orgErasureRetry for a transient purge failure", got)
	}
	if len(failing.calls()) != 1 || len(healthy.calls()) != 1 {
		t.Fatalf("failing calls = %v, healthy calls = %v, want both stores run once", failing.calls(), healthy.calls())
	}
}

// Disabled mode has no provider record, so its wiring runs no provider step and the local stores run alone.
func TestOrgErasure_UserDisabledMode_RunsTheLocalPurgeWithoutAProviderStep(t *testing.T) {
	captureLog(t)
	order := &orderLog{}
	provider := &recordingUserPurger{name: "provider", order: order, err: errors.New("delivery disabled")}
	rows := &recordingUserPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 2}}
	pushes := &recordingUserPurger{name: "push", order: order, counts: map[string]int64{"notification_web_push_subscriptions": 1}}
	s := NewOrgErasureSubscriber(nil).WithUserErasure("disabled", provider, rows, pushes)

	if got := s.process(context.Background(), userErasure("user", "user-1", "org-1")); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck: no provider step may run in disabled mode", got)
	}
	if got, want := order.snapshot(), []string{"rows", "push"}; strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("purge order = %v, want %v with no provider step", got, want)
	}
}

// The subscriber without user steps behaves as it did before per-user erasure existed: a user event acks, nothing is purged, and
// no organization store runs for it. This is the wiring when no user store is configured.
func TestOrgErasure_UserEventWithoutUserSteps_AcksWithoutPurging(t *testing.T) {
	orgs := &fakeOrgPurger{}
	s := NewOrgErasureSubscriber(nil, orgs)
	captureLog(t)

	for _, subjectType := range []string{"user", "user_anonymize"} {
		if got := s.process(context.Background(), userErasure(subjectType, "user-1", "org-1")); got != orgErasureAck {
			t.Fatalf("%s outcome = %v, want orgErasureAck", subjectType, got)
		}
	}
	if calls := orgs.calls(); len(calls) != 0 {
		t.Fatalf("user events purged organization %v, want no purge without user steps", calls)
	}
}

// The organization and user steps are kept apart: an organization event never reaches a user store, and a user event never
// reaches an organization store.
func TestOrgErasure_UserAndOrganizationStepsAreKeptApart(t *testing.T) {
	orgs := &fakeOrgPurger{}
	users := &fakeUserPurger{}
	s := NewOrgErasureSubscriber(nil, orgs).WithUserErasure("disabled", nil, users)
	captureLog(t)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureAck {
		t.Fatalf("organization outcome = %v, want orgErasureAck", got)
	}
	if calls := users.calls(); len(calls) != 0 {
		t.Fatalf("organization erasure reached the user stores: %v", calls)
	}
	if got := s.process(context.Background(), userErasure("user", "user-1", "org-1")); got != orgErasureAck {
		t.Fatalf("user outcome = %v, want orgErasureAck", got)
	}
	if calls := orgs.calls(); len(calls) != 1 || calls[0] != "org-1" {
		t.Fatalf("organization purge calls = %v, want only the organization event's org-1", calls)
	}
}

// The success log names the counts and never the user id. The counts are the only detail that leaves the purge.
func TestOrgErasure_UserEvent_LogCarriesCountsButNotTheUserId(t *testing.T) {
	users := &fakeUserPurger{counts: map[string]int64{"notification_requests": 2, "notification_feed_items": 3}}
	s := NewOrgErasureSubscriber(nil).WithUserErasure("disabled", nil, users)
	logged := captureLog(t)

	if got := s.process(context.Background(), userErasure("user", "user-secret-42", "org-1")); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck", got)
	}
	line := logged.String()
	if !strings.Contains(line, "notification_feed_items=3 notification_requests=2") {
		t.Fatalf("log = %q, want the counts in table order", line)
	}
	if strings.Contains(line, "user-secret-42") {
		t.Fatalf("log = %q, must not carry the user id", line)
	}
}
