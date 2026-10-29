package runwatch

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
)

// fakeStore is an in-memory SubscriptionStore double: lookups() records every
// (orgID, runID) queried and returns the queued watchers for it; notified()
// records every MarkNotified call so a test can assert exactly which rows
// were flipped.
type fakeStore struct {
	mu        sync.Mutex
	watchers  map[string][]Watcher // key: orgID+"/"+runID
	lookupErr error
	markErr   map[string]error // keyed by watcher id
	notified  []string         // watcher ids marked notified, in call order
	lookups   int
}

func (s *fakeStore) key(orgID, runID string) string { return orgID + "/" + runID }

func (s *fakeStore) PendingWatchers(_ context.Context, orgID, runID string) ([]Watcher, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.lookups++
	if s.lookupErr != nil {
		return nil, s.lookupErr
	}
	return append([]Watcher(nil), s.watchers[s.key(orgID, runID)]...), nil
}

func (s *fakeStore) MarkNotified(_ context.Context, watcherID, _ string, _ time.Time) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.markErr[watcherID]; err != nil {
		return err
	}
	s.notified = append(s.notified, watcherID)
	return nil
}

// fakeNotifyClient is an in-memory NotifyClient double: accepted() records
// every request it was asked to send, and failFor lets a test inject an
// error for a specific recipient user id.
type fakeNotifyClient struct {
	mu       sync.Mutex
	accepted []notifyclient.Request
	failFor  map[string]error // keyed by recipient user id
}

func (c *fakeNotifyClient) Accept(_ context.Context, req notifyclient.Request) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := c.failFor[req.Recipient.ID]; err != nil {
		return err
	}
	c.accepted = append(c.accepted, req)
	return nil
}

func testEnvelope(t *testing.T, eventType, orgID, runID string) []byte {
	t.Helper()
	env := envelope.Envelope{
		EventID:        "evt-1",
		EventType:      eventType,
		SchemaVersion:  1,
		Ts:             time.Date(2026, 8, 18, 0, 0, 0, 0, time.UTC),
		Producer:       "orchestrator-core",
		CorrelationID:  runID,
		OrgID:          orgID,
		UserID:         "user-system",
		ResourceRef:    "run/" + runID,
		IdempotencyKey: runID + ":" + eventType,
		Payload:        json.RawMessage(`{"run_id":"` + runID + `"}`),
	}
	data, err := env.Encode()
	if err != nil {
		t.Fatalf("encode envelope: %v", err)
	}
	return data
}

func TestProcess_NonTerminalEventIsAckedWithoutLookup(t *testing.T) {
	store := &fakeStore{}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	for _, eventType := range []string{"RUN_STARTED", "RUN_STEP", "SOMETHING_ELSE"} {
		data := testEnvelope(t, eventType, "org-1", "run-1")
		if got := n.process(context.Background(), data); got != outcomeAck {
			t.Fatalf("event_type=%s outcome = %v, want outcomeAck", eventType, got)
		}
	}
	if store.lookups != 0 {
		t.Fatalf("non-terminal events must never query the store, got %d lookups", store.lookups)
	}
}

func TestProcess_MalformedEnvelopeIsAcked(t *testing.T) {
	store := &fakeStore{}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	if got := n.process(context.Background(), []byte("not json")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck for a poison message", got)
	}
	if store.lookups != 0 {
		t.Fatal("malformed envelope must never reach the store")
	}
}

func TestProcess_NoWatchers_AcksWithoutNotifying(t *testing.T) {
	store := &fakeStore{watchers: map[string][]Watcher{}}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	data := testEnvelope(t, "RUN_COMPLETED", "org-1", "run-1")
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if store.lookups != 1 {
		t.Fatalf("lookups = %d, want exactly 1", store.lookups)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("no watchers means no notify calls")
	}
}

func TestProcess_RunCompleted_NotifiesEachPendingWatcherAndMarksNotified(t *testing.T) {
	store := &fakeStore{watchers: map[string][]Watcher{
		"org-1/run-1": {{ID: "watch-a", UserID: "user-a"}, {ID: "watch-b", UserID: "user-b"}},
	}}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: func() time.Time { return time.Date(2026, 8, 18, 1, 0, 0, 0, time.UTC) }}

	data := testEnvelope(t, "RUN_COMPLETED", "org-1", "run-1")
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 2 {
		t.Fatalf("notify calls = %d, want 2", len(notify.accepted))
	}
	for _, req := range notify.accepted {
		if req.OrganizationID != "org-1" {
			t.Fatalf("request org = %q, want org-1", req.OrganizationID)
		}
		if req.Type != notifyTypeRunCompleted {
			t.Fatalf("request type = %q, want %q", req.Type, notifyTypeRunCompleted)
		}
		if req.Recipient.Kind != notifyclient.RecipientKindUser {
			t.Fatalf("recipient kind = %q, want user", req.Recipient.Kind)
		}
		if runID, _ := req.Payload["run_id"].(string); runID != "run-1" {
			t.Fatalf("payload run_id = %v, want run-1", req.Payload["run_id"])
		}
	}
	wantKeyA := "run-watch:org-1:run-1:user-a:RUN_COMPLETED"
	wantKeyB := "run-watch:org-1:run-1:user-b:RUN_COMPLETED"
	gotKeys := map[string]bool{notify.accepted[0].IdempotencyKey: true, notify.accepted[1].IdempotencyKey: true}
	if !gotKeys[wantKeyA] || !gotKeys[wantKeyB] {
		t.Fatalf("idempotency keys = %v, want %q and %q", gotKeys, wantKeyA, wantKeyB)
	}
	if len(store.notified) != 2 {
		t.Fatalf("marked-notified rows = %d, want 2", len(store.notified))
	}
}

func TestProcess_RunFailed_UsesFailedNotificationType(t *testing.T) {
	store := &fakeStore{watchers: map[string][]Watcher{
		"org-1/run-2": {{ID: "watch-a", UserID: "user-a"}},
	}}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	data := testEnvelope(t, "RUN_FAILED", "org-1", "run-2")
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 1 || notify.accepted[0].Type != notifyTypeRunFailed {
		t.Fatalf("accepted = %+v, want one modelplane.run_failed request", notify.accepted)
	}
}

func TestProcess_StoreLookupError_Retries(t *testing.T) {
	store := &fakeStore{lookupErr: errors.New("db unavailable")}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	data := testEnvelope(t, "RUN_COMPLETED", "org-1", "run-1")
	if got := n.process(context.Background(), data); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry on a transient lookup failure", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("a failed lookup must never reach the notify client")
	}
}

// TestProcess_OneNotifyFailure_RetriesWholeMessageButNotDuplicates is the
// critical safety property of the manual-ack design: if one watcher's notify
// call fails, the message is Nak'd so JetStream redelivers it — but the
// watcher that already succeeded must not be notified a second time on
// redelivery, because [fakeStore] (standing in for the real 'pending' guard)
// only returns still-pending rows.
func TestProcess_OneNotifyFailure_RetriesWholeMessageButNotDuplicates(t *testing.T) {
	store := &fakeStore{watchers: map[string][]Watcher{
		"org-1/run-1": {{ID: "watch-a", UserID: "user-a"}, {ID: "watch-b", UserID: "user-b"}},
	}}
	notify := &fakeNotifyClient{failFor: map[string]error{"user-b": errors.New("notification-core unreachable")}}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	data := testEnvelope(t, "RUN_COMPLETED", "org-1", "run-1")
	if got := n.process(context.Background(), data); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry when one watcher's notify fails", got)
	}
	if len(notify.accepted) != 1 || notify.accepted[0].Recipient.ID != "user-a" {
		t.Fatalf("accepted = %+v, want exactly user-a's notification to have gone out", notify.accepted)
	}
	if len(store.notified) != 1 || store.notified[0] != "watch-a" {
		t.Fatalf("notified rows = %v, want exactly watch-a marked", store.notified)
	}

	// Simulate JetStream redelivery: the real store's status='pending' guard
	// means a redelivered lookup would no longer return watch-a (already
	// notified). Model that here by removing it from the fake before the
	// second process() call, then confirm only watch-b gets notified.
	store.watchers["org-1/run-1"] = []Watcher{{ID: "watch-b", UserID: "user-b"}}
	notify.failFor = nil
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("redelivery outcome = %v, want outcomeAck once the transient failure clears", got)
	}
	if len(notify.accepted) != 2 || notify.accepted[1].Recipient.ID != "user-b" {
		t.Fatalf("accepted after redelivery = %+v, want user-a (once) then user-b", notify.accepted)
	}
}

func TestProcess_MarkNotifiedFailure_Retries(t *testing.T) {
	store := &fakeStore{
		watchers: map[string][]Watcher{"org-1/run-1": {{ID: "watch-a", UserID: "user-a"}}},
		markErr:  map[string]error{"watch-a": errors.New("db unavailable")},
	}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	data := testEnvelope(t, "RUN_COMPLETED", "org-1", "run-1")
	if got := n.process(context.Background(), data); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry when persisting the notified state fails", got)
	}
	if len(notify.accepted) != 1 {
		t.Fatal("the notification itself must still have been sent exactly once")
	}
}

func TestProcess_MissingOrgOrRunID_Acks(t *testing.T) {
	store := &fakeStore{}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	for _, test := range []struct {
		name string
		env  envelope.Envelope
	}{
		{"missing org_id", envelope.Envelope{EventType: "RUN_COMPLETED", ResourceRef: "run/run-1"}},
		{"missing run id anywhere", envelope.Envelope{EventType: "RUN_COMPLETED", OrgID: "org-1"}},
	} {
		t.Run(test.name, func(t *testing.T) {
			data, err := test.env.Encode()
			if err != nil {
				t.Fatal(err)
			}
			if got := n.process(context.Background(), data); got != outcomeAck {
				t.Fatalf("outcome = %v, want outcomeAck", got)
			}
		})
	}
	if store.lookups != 0 {
		t.Fatal("an envelope missing org_id/run_id must never reach the store")
	}
}

func TestRunIDFrom_FallsBackAcrossConventions(t *testing.T) {
	for _, test := range []struct {
		name string
		env  envelope.Envelope
		want string
	}{
		{
			name: "payload run_id wins",
			env:  envelope.Envelope{Payload: json.RawMessage(`{"run_id":"from-payload"}`), CorrelationID: "from-correlation", ResourceRef: "run/from-resource-ref"},
			want: "from-payload",
		},
		{
			name: "falls back to correlation_id",
			env:  envelope.Envelope{CorrelationID: "from-correlation", ResourceRef: "run/from-resource-ref"},
			want: "from-correlation",
		},
		{
			name: "falls back to resource_ref run/ prefix",
			env:  envelope.Envelope{ResourceRef: "run/from-resource-ref"},
			want: "from-resource-ref",
		},
		{
			name: "falls back to resource_ref run: prefix",
			env:  envelope.Envelope{ResourceRef: "run:from-resource-ref"},
			want: "from-resource-ref",
		},
		{
			name: "nothing present",
			env:  envelope.Envelope{},
			want: "",
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			if got := runIDFrom(&test.env); got != test.want {
				t.Fatalf("runIDFrom = %q, want %q", got, test.want)
			}
		})
	}
}

func TestNewNotifier_RequiresBothDependencies(t *testing.T) {
	if _, err := NewNotifier(nil, &fakeNotifyClient{}); err == nil {
		t.Fatal("NewNotifier(nil store, ...) should error")
	}
	if _, err := NewNotifier(&fakeStore{}, nil); err == nil {
		t.Fatal("NewNotifier(..., nil notify) should error")
	}
	n, err := NewNotifier(&fakeStore{}, &fakeNotifyClient{})
	if err != nil || n == nil {
		t.Fatalf("NewNotifier with both dependencies = (%v, %v), want a usable Notifier", n, err)
	}
}
