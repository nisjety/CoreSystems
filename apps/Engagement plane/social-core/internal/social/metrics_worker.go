package social

import (
	"context"
	"log"
	"strings"
	"time"
)

type MetricsWorker struct {
	service  *Service
	id       string
	interval time.Duration
	orgID    string
}

func NewMetricsWorker(service *Service, id string, interval time.Duration, orgID string) *MetricsWorker {
	if interval <= 0 {
		interval = 6 * time.Hour
	}
	return &MetricsWorker{
		service:  service,
		id:       id,
		interval: interval,
		orgID:    strings.TrimSpace(orgID),
	}
}

func (w *MetricsWorker) Start(ctx context.Context) {
	if w == nil || w.service == nil || w.orgID == "" {
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

func (w *MetricsWorker) tick(ctx context.Context) {
	if w == nil || w.service == nil || w.orgID == "" {
		return
	}
	summary, err := w.service.SnapshotProviderMetrics(ctx, w.orgID)
	if err != nil {
		log.Printf("social-core: metrics worker tick failed: %v", err)
		return
	}
	if len(summary.Failures) > 0 {
		log.Printf("social-core: metrics worker: %d account snapshot(s) failed: %s",
			len(summary.Failures), strings.Join(summary.Failures, "; "))
	}
	if summary.Metrics > 0 {
		log.Printf("social-core: metrics worker snapshotted %d metric(s) across %d account(s) in %d org(s)",
			summary.Metrics, summary.Accounts, summary.Orgs)
	}
}
