package consumers

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

// fakeReconcilerService records every call it receives and returns a canned
// result/error, so the sweep loop can be tested without a real repository.
type fakeReconcilerService struct {
	mu         sync.Mutex
	calls      []time.Duration
	result     []conversation.OutboundIntent
	err        error
	callSignal chan struct{}
}

func (f *fakeReconcilerService) ReconcileStaleOutboundIntents(_ context.Context, staleAfter time.Duration) ([]conversation.OutboundIntent, error) {
	f.mu.Lock()
	f.calls = append(f.calls, staleAfter)
	result, err := f.result, f.err
	f.mu.Unlock()
	if f.callSignal != nil {
		f.callSignal <- struct{}{}
	}
	return result, err
}

func (f *fakeReconcilerService) callCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.calls)
}

func (f *fakeReconcilerService) lastStaleAfter() time.Duration {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls[len(f.calls)-1]
}

// Start must run an immediate sweep (not wait a full interval) so a
// crash-recovery window right after a deploy is caught without delay, and it
// must forward the exact configured staleAfter.
func TestOutboundIntentReconcilerRunsImmediatelyOnStart(t *testing.T) {
	service := &fakeReconcilerService{callSignal: make(chan struct{}, 1)}
	reconciler := NewOutboundIntentReconciler(service, time.Hour, 15*time.Minute)

	reconciler.Start(context.Background())
	defer reconciler.Stop()

	select {
	case <-service.callSignal:
	case <-time.After(2 * time.Second):
		t.Fatal("sweep did not run immediately on Start")
	}
	if service.callCount() != 1 {
		t.Fatalf("call count = %d, want 1", service.callCount())
	}
	if service.lastStaleAfter() != 15*time.Minute {
		t.Fatalf("staleAfter = %v, want 15m", service.lastStaleAfter())
	}
}

// The loop must keep ticking (and therefore keep sweeping) even when a run
// fails, so one transient DB error does not permanently disable the sweep.
func TestOutboundIntentReconcilerContinuesAfterRunError(t *testing.T) {
	service := &fakeReconcilerService{err: errors.New("db unavailable"), callSignal: make(chan struct{}, 1)}
	reconciler := NewOutboundIntentReconciler(service, 10*time.Millisecond, time.Minute)

	reconciler.Start(context.Background())
	defer reconciler.Stop()

	seen := 0
	for seen < 3 {
		select {
		case <-service.callSignal:
			seen++
		case <-time.After(2 * time.Second):
			t.Fatalf("sweep stopped ticking after %d run(s) despite repository errors", seen)
		}
	}
}

// Stop must return only once the loop has actually exited, so a caller never
// races a shutdown against an in-flight sweep.
func TestOutboundIntentReconcilerStopWaitsForLoopExit(t *testing.T) {
	service := &fakeReconcilerService{}
	reconciler := NewOutboundIntentReconciler(service, time.Hour, time.Minute)

	reconciler.Start(context.Background())
	reconciler.Stop()

	select {
	case <-reconciler.done:
	default:
		t.Fatal("Stop returned before the loop signalled done")
	}
}
