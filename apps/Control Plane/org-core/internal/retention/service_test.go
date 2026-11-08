package retention

import (
	"context"
	"errors"
	"testing"
)

// recordingStore is a Store that records the hold it was asked to place and
// serves canned audit rows; the rest answer nothing.
type recordingStore struct {
	placed    *Hold
	audit     []AuditRow
	published []string
	failed    []string
}

func (s *recordingStore) GetPolicy(context.Context, string) (*Policy, error) { return &Policy{}, nil }
func (s *recordingStore) PutPolicy(_ context.Context, orgID string, actor Actor, _ int64, days *int) (*Policy, error) {
	return &Policy{OrganizationID: orgID, ThreadDays: days, Revision: 1, UpdatedBy: actor.ID}, nil
}
func (s *recordingStore) ListHolds(context.Context, string) ([]Hold, error) { return nil, nil }
func (s *recordingStore) PlaceHold(_ context.Context, _ string, _ Actor, hold Hold) (PlaceResult, error) {
	s.placed = &hold
	return PlaceResult{Hold: hold}, nil
}
func (s *recordingStore) ReleaseHold(context.Context, string, string, Actor) (Hold, error) {
	return Hold{}, ErrHoldNotFound
}
func (s *recordingStore) Check(context.Context, string, []string) (CheckResult, error) {
	return CheckResult{}, nil
}
func (s *recordingStore) ClaimAudit(context.Context, int) ([]AuditRow, error) { return s.audit, nil }
func (s *recordingStore) MarkAuditPublished(_ context.Context, eventID string) error {
	s.published = append(s.published, eventID)
	return nil
}
func (s *recordingStore) MarkAuditFailed(_ context.Context, eventID string, _ error) (bool, error) {
	s.failed = append(s.failed, eventID)
	return false, nil
}

type fixedThreads struct {
	nothing bool
	err     error
	asked   []string
}

func (f *fixedThreads) NothingRetained(_ context.Context, _, threadID string) (bool, error) {
	f.asked = append(f.asked, threadID)
	return f.nothing, f.err
}

var owner = Actor{ID: "u-owner", Role: "owner"}

func TestThreadHoldOnANothingRetainedThreadIsRefused(t *testing.T) {
	store := &recordingStore{}
	threads := &fixedThreads{nothing: true}
	service := NewService(store, threads, nil)

	_, err := service.PlaceHold(context.Background(), "org-1", owner, PlaceHoldRequest{Scope: ScopeThread, ThreadID: "thr-zdr", HoldRef: "Matter 1"})
	if !errors.Is(err, ErrNothingRetained) {
		t.Fatalf("a ZDR or waived thread has nothing to hold: err=%v", err)
	}
	if store.placed != nil {
		t.Fatal("a refused hold must write nothing")
	}

	// An org hold on a ZDR organisation is allowed: it holds whatever non-ZDR
	// content exists, so the thread reader is not asked.
	threads.asked = nil
	if _, err := service.PlaceHold(context.Background(), "org-1", owner, PlaceHoldRequest{Scope: ScopeOrg, HoldRef: "Matter 1"}); err != nil {
		t.Fatalf("org hold: %v", err)
	}
	if len(threads.asked) != 0 || store.placed == nil || store.placed.TargetRef != "org-1" {
		t.Fatalf("an org hold targets the organisation without a thread lookup: asked=%v placed=%+v", threads.asked, store.placed)
	}
}

func TestThreadHoldIsPlacedWhenRetentionIsUnknown(t *testing.T) {
	for name, threads := range map[string]ThreadRetention{
		"reader error": &fixedThreads{err: errors.New("session-core unreachable")},
		"no reader":    nil,
		"retained":     &fixedThreads{nothing: false},
	} {
		store := &recordingStore{}
		result, err := NewService(store, threads, nil).PlaceHold(context.Background(), "org-1", owner, PlaceHoldRequest{Scope: ScopeThread, ThreadID: "thr-1", HoldRef: "Matter 1"})
		if err != nil || store.placed == nil || store.placed.TargetRef != "thr-1" || !validHoldID(result.Hold.ID) {
			t.Fatalf("%s: a hold whose retention is not known to be empty must be placed: result=%+v err=%v", name, result, err)
		}
	}
}

func TestWritesNeedAnActorWithARole(t *testing.T) {
	service := NewService(&recordingStore{}, nil, nil)
	if _, err := service.PutPolicy(context.Background(), "org-1", Actor{ID: "u"}, PutRequest{ExpectedRevision: revPtr(0)}); err == nil {
		t.Fatal("a policy write without the actor's role must be refused: the audit row records it")
	}
	if _, err := service.PlaceHold(context.Background(), "org-1", Actor{Role: "owner"}, PlaceHoldRequest{Scope: ScopeOrg, HoldRef: "m"}); err == nil {
		t.Fatal("a hold without an actor must be refused")
	}
	if _, err := service.ReleaseHold(context.Background(), "org-1", "not-a-hold-id", owner); !errors.Is(err, ErrHoldNotFound) {
		t.Fatalf("a malformed hold id is not found, never sent to the store: %v", err)
	}
}

type flakyPublisher struct{ fail map[string]bool }

func (p flakyPublisher) PublishAudit(_ context.Context, _, eventID string, _ map[string]any) error {
	if p.fail[eventID] {
		return errors.New("nats down")
	}
	return nil
}

func TestFlushPublishesAndRecordsFailures(t *testing.T) {
	store := &recordingStore{audit: []AuditRow{{EventID: "e1", Subject: AuditSubject}, {EventID: "e2", Subject: AuditSubject}}}
	service := NewService(store, nil, flakyPublisher{fail: map[string]bool{"e2": true}})
	result, err := service.FlushAuditOutbox(context.Background(), 10)
	if err == nil || result.Published != 1 || len(store.published) != 1 || store.published[0] != "e1" || len(store.failed) != 1 || store.failed[0] != "e2" {
		t.Fatalf("flush: result=%+v published=%v failed=%v err=%v", result, store.published, store.failed, err)
	}
	if _, err := NewService(store, nil, nil).FlushAuditOutbox(context.Background(), 10); err == nil {
		t.Fatal("a flush without a publisher must fail, not drop rows")
	}
}
