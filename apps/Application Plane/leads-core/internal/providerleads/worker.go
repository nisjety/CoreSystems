package providerleads

import (
	"context"
	"log"
	"time"
)

// Worker runs the provider-lead sync on an interval (mirrors social-core's
// publish worker shape: immediate tick, then ticker until ctx is done).
type Worker struct {
	syncer   *Syncer
	interval time.Duration
}

func NewWorker(syncer *Syncer, interval time.Duration) *Worker {
	if interval <= 0 {
		interval = time.Hour
	}
	return &Worker{syncer: syncer, interval: interval}
}

func (w *Worker) Start(ctx context.Context) {
	if w == nil || w.syncer == nil {
		return
	}
	w.tick(ctx)
	ticker := time.NewTicker(w.interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			w.tick(ctx)
		}
	}
}

func (w *Worker) tick(ctx context.Context) {
	result, err := w.syncer.Sync(ctx, "", "")
	if err != nil {
		log.Printf("leads-core: provider-lead sync tick failed: %v", err)
		return
	}
	if result.Connections > 0 || result.LeadsUpserted > 0 {
		log.Printf("leads-core: provider-lead sync: %d connection(s), %d form(s), %d lead(s) fetched, %d upserted",
			result.Connections, result.Forms, result.LeadsFetched, result.LeadsUpserted)
	}
}
