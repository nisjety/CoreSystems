package main

import (
	"context"
	"errors"
	"os"
	"strings"
	"testing"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
)

// fakeOrgFlagAuditFlusher stands in for the org flag outbox. Its first `failures` flushes fail the
// way a publish does; the first flush after them succeeds and closes recovered. Only the loop calls
// it, so calls is read only after the loop has stopped.
type fakeOrgFlagAuditFlusher struct {
	failures  int
	calls     int
	succeeded bool
	recovered chan struct{}
}

func (f *fakeOrgFlagAuditFlusher) FlushOrgFlagAuditOutbox(context.Context, int) (orgcore.GDPRAuditFlushResult, error) {
	f.calls++
	if f.failures > 0 {
		f.failures--
		return orgcore.GDPRAuditFlushResult{}, errors.New("fixture publish failure")
	}
	if !f.succeeded {
		f.succeeded = true
		close(f.recovered)
	}
	return orgcore.GDPRAuditFlushResult{}, nil
}

// P2-cp-06-rel1: a publish that fails in the inline flush stays pending. The org flag outbox had no
// caller but that inline flush, so only the next flag write in any organisation retried it. The loop
// must flush on its own ticks, retrying a failed flush with no flag write in between, and it must
// stop when its context ends.
func TestRunOrgFlagAuditOutboxRetriesAFailedFlushWithoutAFlagWrite(t *testing.T) {
	flusher := &fakeOrgFlagAuditFlusher{failures: 1, recovered: make(chan struct{})}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan struct{})
	go func() {
		defer close(done)
		runOrgFlagAuditOutbox(ctx, flusher, time.Millisecond)
	}()

	select {
	case <-flusher.recovered:
	case <-time.After(10 * time.Second):
		t.Fatal("the loop never retried the failed flush")
	}
	cancel()
	select {
	case <-done:
	case <-time.After(10 * time.Second):
		t.Fatal("the loop kept running after its context was cancelled")
	}
	if flusher.calls < 2 {
		t.Fatalf("flush calls=%d; want the failed flush and its retry", flusher.calls)
	}
}

// The loop is the only periodic caller, so main must start it. A flush without a publisher only
// fails, so the start must be a real statement, at the indentation of the block that wires the
// publisher, after SetAuditPublisher. This reads main.go, the package's own source: a removed or
// commented-out start line fails here, and so does one moved above SetAuditPublisher.
func TestOrgFlagAuditOutboxLoopIsStartedWhereThePublisherIsWired(t *testing.T) {
	src, err := os.ReadFile("main.go")
	if err != nil {
		t.Fatalf("read main.go: %v", err)
	}
	const publisher = "\n\t\torgService.SetAuditPublisher(natsClient)\n"
	const start = "\n\t\tgo runOrgFlagAuditOutbox(ctx, orgService, 5*time.Second)\n"
	text := string(src)
	wiredAt := strings.Index(text, publisher)
	startedAt := strings.Index(text, start)
	if wiredAt < 0 || startedAt < 0 || startedAt < wiredAt {
		t.Fatalf("main.go must start runOrgFlagAuditOutbox after orgService.SetAuditPublisher(natsClient) in the same block (publisher at %d, start at %d)", wiredAt, startedAt)
	}
}
