package consumers

import (
	"context"
	"errors"
	"sync"
	"testing"
)

type fakeConversationDraftPurger struct {
	mu     sync.Mutex
	orgIDs []string
	err    error
}

func (f *fakeConversationDraftPurger) PurgeConversationDraftsByOrg(_ context.Context, orgID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return f.err
	}
	f.orgIDs = append(f.orgIDs, orgID)
	return nil
}

func (f *fakeConversationDraftPurger) calls() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]string(nil), f.orgIDs...)
}

func TestInteractiveRetention_EnabledEventPurgesOnlyConversationDrafts(t *testing.T) {
	purger := &fakeConversationDraftPurger{}
	consumer := &InteractiveRetentionConsumer{purger: purger}

	if got := consumer.process(t.Context(), interactiveRetentionEvent{OrgID: "  org-1 ", ZDR: true}); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if got := purger.calls(); len(got) != 1 || got[0] != "org-1" {
		t.Fatalf("purge calls = %#v, want [org-1]", got)
	}
}

func TestInteractiveRetention_IgnoresDisabledOrMalformedEvents(t *testing.T) {
	for _, event := range []interactiveRetentionEvent{
		{OrgID: "org-1", ZDR: false},
		{OrgID: "   ", ZDR: true},
	} {
		purger := &fakeConversationDraftPurger{}
		consumer := &InteractiveRetentionConsumer{purger: purger}
		if got := consumer.process(t.Context(), event); got != outcomeAck {
			t.Fatalf("event %#v outcome = %v, want outcomeAck", event, got)
		}
		if got := purger.calls(); len(got) != 0 {
			t.Fatalf("event %#v purge calls = %#v, want none", event, got)
		}
	}
}

func TestInteractiveRetention_DraftPurgeFailureRetries(t *testing.T) {
	consumer := &InteractiveRetentionConsumer{purger: &fakeConversationDraftPurger{err: errors.New("database unavailable")}}

	if got := consumer.process(t.Context(), interactiveRetentionEvent{OrgID: "org-1", ZDR: true}); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry", got)
	}
}
