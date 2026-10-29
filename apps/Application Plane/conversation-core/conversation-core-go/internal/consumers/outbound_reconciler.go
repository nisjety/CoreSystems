package consumers

import (
	"context"
	"log"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

// OutboundIntentReconcilerService is the narrow surface the sweep needs.
// *conversation.Service satisfies it.
type OutboundIntentReconcilerService interface {
	ReconcileStaleOutboundIntents(ctx context.Context, staleAfter time.Duration) ([]conversation.OutboundIntent, error)
}

// OutboundIntentReconciler is the stuck-send sweep for the review-approve-send
// path: on a fixed interval it flips every outbound intent that has sat in
// `sending` longer than staleAfter to `unknown` (see
// conversation.Service.ReconcileStaleOutboundIntents /
// PGRepository.ReconcileStaleOutboundIntents), so a claim whose process
// crashed, was redeployed, or was OOM-killed before recording an outcome
// never sits invisible forever. It never talks to a provider and never
// retransmits a message — purely a status/alert sweep over conversation-core's
// own ledger — so it needs no new provider integration and carries no
// double-send risk.
type OutboundIntentReconciler struct {
	service    OutboundIntentReconcilerService
	interval   time.Duration
	staleAfter time.Duration
	runTimeout time.Duration
	stop       chan struct{}
	done       chan struct{}
}

// NewOutboundIntentReconciler wires the sweep. interval is how often it runs;
// staleAfter is how long a `sending` row must be untouched before it is
// reconciled. Both must be positive — the caller (main) is responsible for
// applying sane defaults from config.
func NewOutboundIntentReconciler(service OutboundIntentReconcilerService, interval, staleAfter time.Duration) *OutboundIntentReconciler {
	return &OutboundIntentReconciler{
		service:    service,
		interval:   interval,
		staleAfter: staleAfter,
		runTimeout: 30 * time.Second,
		stop:       make(chan struct{}),
		done:       make(chan struct{}),
	}
}

// Start runs the sweep in the background until Stop is called. It sweeps once
// immediately (so a crash-recovery window right after a deploy is not left
// waiting a full interval) and then on every tick of interval.
func (o *OutboundIntentReconciler) Start(ctx context.Context) {
	go func() {
		defer close(o.done)
		o.run(ctx)
		ticker := time.NewTicker(o.interval)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				o.run(ctx)
			case <-o.stop:
				return
			}
		}
	}()
}

// Stop signals the sweep loop to exit and waits for the in-flight run (if
// any) to finish.
func (o *OutboundIntentReconciler) Stop() {
	close(o.stop)
	<-o.done
}

func (o *OutboundIntentReconciler) run(ctx context.Context) {
	runCtx, cancel := context.WithTimeout(ctx, o.runTimeout)
	defer cancel()
	reconciled, err := o.service.ReconcileStaleOutboundIntents(runCtx, o.staleAfter)
	if err != nil {
		log.Printf("[cc-go/outbound-reconciler] sweep failed: %v", err)
		return
	}
	if len(reconciled) > 0 {
		log.Printf("[cc-go/outbound-reconciler] reconciled %d stale outbound intent(s)", len(reconciled))
	}
}
