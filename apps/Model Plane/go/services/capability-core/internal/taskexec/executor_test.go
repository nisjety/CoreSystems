package taskexec

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
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

func TestExecutorClaimQueryRefusesTasksFromDeletedCronSchedules(t *testing.T) {
	source, err := os.ReadFile(filepath.Join("executor.go"))
	if err != nil {
		t.Fatalf("read executor source: %v", err)
	}
	for _, required := range []string{
		"FROM cron_fires AS cf",
		"JOIN cron_schedules AS cs ON cs.id = cf.schedule_id",
		"cf.task_id = t.id AND cs.deleted_at IS NOT NULL",
	} {
		if !strings.Contains(string(source), required) {
			t.Fatalf("claim query must retain deleted-cron task fence %q", required)
		}
	}
}

// TestNatsDispatcherPublishesDispatchEvent covers AUTO-3: without an
// acknowledged external consumer, NatsDispatcher must still publish (so
// anything watching the subject sees the attempt) but must fail the
// dispatch, so the executor marks the task failed with a clear reason
// instead of leaving it stranded in `running` forever. Acknowledging an
// external consumer restores the original publish-and-succeed behavior for a
// deployment that has actually built one.
func TestNatsDispatcherPublishesDispatchEvent(t *testing.T) {
	t.Run("no acknowledged consumer: publishes then fails", func(t *testing.T) {
		pub := publisher.NewInMemoryPublisher()
		d := NewNatsDispatcher(pub, false)
		err := d.Dispatch(context.Background(), TaskRef{ID: "task_1", OrgID: "org_1", Kind: "cron"})
		if err == nil {
			t.Fatal("expected Dispatch to fail without an acknowledged external consumer")
		}
		records := pub.Drain()
		if len(records) != 1 {
			t.Fatalf("expected 1 published record even on failure, got %d", len(records))
		}
		want := reconcile.Subject(reconcile.KindTask, reconcile.ActionDispatched)
		if records[0].Subject != want {
			t.Fatalf("dispatch subject = %q, want %q", records[0].Subject, want)
		}
	})

	t.Run("acknowledged consumer: publishes and succeeds", func(t *testing.T) {
		pub := publisher.NewInMemoryPublisher()
		d := NewNatsDispatcher(pub, true)
		if err := d.Dispatch(context.Background(), TaskRef{ID: "task_2", OrgID: "org_1", Kind: "cron"}); err != nil {
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
	})
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
