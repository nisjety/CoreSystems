package social

import (
	"context"
	"log"
	"time"
)

type Worker struct {
	service  *Service
	id       string
	interval time.Duration
	batch    int
}

func NewWorker(service *Service, id string, interval time.Duration, batch int) *Worker {
	if interval <= 0 {
		interval = 5 * time.Second
	}
	if batch < 1 || batch > 100 {
		batch = 10
	}
	return &Worker{
		service:  service,
		id:       id,
		interval: interval,
		batch:    batch,
	}
}

func (w *Worker) Start(ctx context.Context) {
	if w == nil || w.service == nil {
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
	count, err := w.service.ProcessDuePublishJobs(ctx, w.id, w.batch)
	if err != nil {
		log.Printf("social-core: publish worker tick failed: %v", err)
		return
	}
	if count > 0 {
		log.Printf("social-core: publish worker processed %d job(s)", count)
	}
}
