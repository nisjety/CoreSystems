package spaces

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"
)

// WS04-S10: pure parts of the durable membership publisher. The claim,
// concurrency and re-announce legs are in membership_events_outbox_live_test.go.

type recordedPublish struct {
	subject, msgID string
	payload        map[string]any
}

// fakeMembershipPublisher records every publish and fails the ones in failIDs.
type fakeMembershipPublisher struct {
	mu      sync.Mutex
	calls   []recordedPublish
	failIDs map[string]error
	failAll error
}

func (f *fakeMembershipPublisher) PublishWithAck(_ context.Context, subject, msgID string, payload map[string]any) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, recordedPublish{subject, msgID, payload})
	if f.failAll != nil {
		return f.failAll
	}
	return f.failIDs[msgID]
}

func (f *fakeMembershipPublisher) published() []recordedPublish {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]recordedPublish(nil), f.calls...)
}

func TestMembershipEventsOutboxConfigDefaults(t *testing.T) {
	got := MembershipEventsOutboxConfig{}.withDefaults()
	if got.PollEvery != 2*time.Second || got.BatchSize != 100 || got.Lease != 60*time.Second ||
		got.ReannounceEvery != 10*time.Minute || got.ReannounceWindow != 24*time.Hour {
		t.Fatalf("defaults = %+v", got)
	}
	set := MembershipEventsOutboxConfig{PollEvery: time.Second, BatchSize: 5, Lease: time.Minute, ReannounceEvery: time.Hour, ReannounceWindow: time.Hour}.withDefaults()
	if set.PollEvery != time.Second || set.BatchSize != 5 || set.Lease != time.Minute || set.ReannounceEvery != time.Hour || set.ReannounceWindow != time.Hour {
		t.Fatalf("explicit values were overridden: %+v", set)
	}
	if neg := (MembershipEventsOutboxConfig{BatchSize: -1, Lease: -time.Second}).withDefaults(); neg.BatchSize != 100 || neg.Lease != 60*time.Second {
		t.Fatalf("negative values must take defaults: %+v", neg)
	}
}

func TestOutboxBackoffGrowsQuadraticallyAndIsCapped(t *testing.T) {
	for attempt, want := range map[int]time.Duration{
		-3: time.Second, 0: time.Second, 1: time.Second, 2: 4 * time.Second, 5: 25 * time.Second, 17: 289 * time.Second,
		18: 300 * time.Second, 1000: 300 * time.Second,
	} {
		if got := outboxBackoff(attempt); got != want {
			t.Errorf("outboxBackoff(%d) = %v, want %v", attempt, got, want)
		}
	}
}

func TestTruncateOutboxErrorKeepsItShortAndTrimmed(t *testing.T) {
	if got := truncateOutboxError(errors.New("  broker down \n")); got != "broker down" {
		t.Fatalf("got %q", got)
	}
	long := truncateOutboxError(errors.New(strings.Repeat("x", 1000)))
	if len(long) != outboxErrorMax {
		t.Fatalf("len = %d, want %d", len(long), outboxErrorMax)
	}
}

func TestClaimedMembershipEventStatesTheCurrentFactNotTheLedgerAction(t *testing.T) {
	base := claimedMembershipEvent{
		eventID: "sme_1", spaceRef: "s1", orgID: "o1", spaceSeq: 9, subjectID: "u1", actorKind: ActorKindUser, toRole: "editor",
	}
	cases := []struct {
		name       string
		mutate     func(*claimedMembershipEvent)
		wantActive bool
		wantRole   string
	}{
		{"added, row still active", func(e *claimedMembershipEvent) {
			e.action, e.hasCurrent, e.curActive, e.currentRole = ActionMemberAdded, true, true, "manager"
		}, true, "manager"},
		{"added, but removed since", func(e *claimedMembershipEvent) {
			e.action, e.hasCurrent, e.curActive, e.currentRole = ActionMemberAdded, true, false, "viewer"
		}, false, "editor"},
		{"removed, but re-added since", func(e *claimedMembershipEvent) {
			e.action, e.hasCurrent, e.curActive, e.currentRole, e.toRole = ActionMemberRemoved, true, true, "viewer", ""
		}, true, "viewer"},
		{"added, row gone (no current)", func(e *claimedMembershipEvent) { e.action = ActionMemberAdded }, true, "editor"},
		{"joined, row gone", func(e *claimedMembershipEvent) { e.action = ActionMemberJoined }, true, "editor"},
		{"removed, row gone", func(e *claimedMembershipEvent) { e.action, e.toRole = ActionMemberRemoved, "" }, false, ""},
		{"left, row inactive", func(e *claimedMembershipEvent) {
			e.action, e.hasCurrent, e.curActive, e.currentRole, e.toRole = ActionMemberLeft, true, false, "editor", ""
		}, false, ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			e := base
			tc.mutate(&e)
			payload := e.payload()
			if payload["active"] != tc.wantActive || payload["role"] != tc.wantRole {
				t.Fatalf("payload = %v, want active=%v role=%q", payload, tc.wantActive, tc.wantRole)
			}
			for key, want := range map[string]any{
				"space_ref": "s1", "org_id": "o1", "subject_id": "u1", "event_id": "sme_1", "space_seq": int64(9),
				"action": e.action, "actor_kind": ActorKindUser,
			} {
				if payload[key] != want {
					t.Errorf("payload[%s] = %v, want %v", key, payload[key], want)
				}
			}
		})
	}
}

// The payload is ids, roles and counts only: no actor id, names, topic or content.
func TestMembershipEventPayloadCarriesNoActorOrContent(t *testing.T) {
	e := claimedMembershipEvent{eventID: "sme_1", spaceRef: "s1", orgID: "o1", subjectID: "u1", action: ActionMemberAdded, actorKind: ActorKindUser}
	allowed := map[string]bool{
		"space_ref": true, "org_id": true, "subject_id": true, "active": true, "event_id": true,
		"space_seq": true, "action": true, "role": true, "actor_kind": true,
	}
	for key := range e.payload() {
		if !allowed[key] {
			t.Errorf("unexpected payload key %q", key)
		}
	}
}

func TestReannounceMsgIDIsStablePerBucketAndDiffersAcrossThem(t *testing.T) {
	a := reannounceMsgID("s1", "u1", 100)
	if a != reannounceMsgID("s1", "u1", 100) {
		t.Fatal("not stable within a bucket")
	}
	for name, other := range map[string]string{
		"next bucket":   reannounceMsgID("s1", "u1", 101),
		"other subject": reannounceMsgID("s1", "u2", 100),
		"other space":   reannounceMsgID("s2", "u1", 100),
		// The separator keeps ("s1","u1x") apart from ("s1x","u1").
		"shifted boundary": reannounceMsgID("s1u", "1", 100),
	} {
		if other == a {
			t.Errorf("%s produced the same message id", name)
		}
	}
	if !strings.HasPrefix(a, "sma_") || len(a) != len("sma_")+32 {
		t.Fatalf("message id %q has the wrong shape", a)
	}
}

func TestMembershipChangedSubjectAndReannounceAction(t *testing.T) {
	if MembershipChangedSubject != "controlplane.space.membership_changed" {
		t.Fatalf("subject = %q: Session Core subscribes to this exact subject", MembershipChangedSubject)
	}
	// A re-announcement is never a ledger action: the ledger CHECK would refuse it.
	for _, ledger := range []string{ActionMemberAdded, ActionMemberJoined, ActionMemberRemoved, ActionMemberLeft} {
		if ledger == ActionReannounce {
			t.Fatal("reannounce collides with a ledger action")
		}
	}
}

func TestRunMembershipEventsOutboxReturnsAtOnceWithoutAPublisherOrRepository(t *testing.T) {
	done := make(chan struct{})
	go func() {
		defer close(done)
		(*Repository)(nil).RunMembershipEventsOutbox(context.Background(), &fakeMembershipPublisher{}, MembershipEventsOutboxConfig{})
		(&Repository{}).RunMembershipEventsOutbox(context.Background(), nil, MembershipEventsOutboxConfig{})
	}()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("the worker did not return without a publisher or a repository")
	}
}
