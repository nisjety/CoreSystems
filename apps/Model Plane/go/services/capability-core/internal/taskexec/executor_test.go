package taskexec

import (
	"context"
	"errors"
	"testing"

	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

func TestIsAutoExecutable(t *testing.T) {
	cases := map[string]bool{
		"agent":    true,
		"cron":     true,
		"workflow": true,
		"shell":    true,
		"manual":   false, // user tracking items are left alone
		"":         false, // unknown/empty never auto-runs
	}
	for kind, want := range cases {
		if got := isAutoExecutable(kind); got != want {
			t.Errorf("isAutoExecutable(%q) = %v, want %v", kind, got, want)
		}
	}
}

func TestNatsDispatcherPublishesDispatchEvent(t *testing.T) {
	pub := publisher.NewInMemoryPublisher()
	d := NewNatsDispatcher(pub)
	if err := d.Dispatch(context.Background(), TaskRef{ID: "task_1", OrgID: "org_1", Kind: "cron"}); err != nil {
		t.Fatalf("Dispatch error: %v", err)
	}
	records := pub.Drain()
	if len(records) != 1 {
		t.Fatalf("expected 1 published record, got %d", len(records))
	}
	want := reconcile.Subject(reconcile.KindTask, reconcile.ActionDispatched)
	if records[0].Subject != want {
		t.Fatalf("dispatch subject = %q, want %q", records[0].Subject, want)
	}
}

// failDispatcher always errors — documents the Dispatcher contract used by the
// executor to flip a task to failed when the hand-off cannot be made.
type failDispatcher struct{}

func (failDispatcher) Dispatch(context.Context, TaskRef) error {
	return errors.New("no runner")
}

func TestDispatcherContract(t *testing.T) {
	if err := (failDispatcher{}).Dispatch(context.Background(), TaskRef{ID: "t"}); err == nil {
		t.Fatal("failDispatcher must return an error")
	}
}
