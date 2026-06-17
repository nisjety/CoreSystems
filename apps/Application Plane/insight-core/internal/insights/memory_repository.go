package insights

import (
	"context"
	"sync"
)

type MemoryRepository struct {
	mu         sync.RWMutex
	events     []MetricEvent
	connectors []ConnectorSlot
}

func NewMemoryRepository(connectors []ConnectorSlot) *MemoryRepository {
	return &MemoryRepository{
		events:     []MetricEvent{},
		connectors: copyConnectorSlots(connectors),
	}
}

func (r *MemoryRepository) RecordMetricEvent(_ context.Context, event MetricEvent) (*MetricEvent, error) {
	r.mu.Lock()
	defer r.mu.Unlock()

	stored := copyMetricEvent(event)
	next := make([]MetricEvent, 0, len(r.events)+1)
	next = append(next, r.events...)
	next = append(next, stored)
	r.events = next

	result := copyMetricEvent(stored)
	return &result, nil
}

func (r *MemoryRepository) ListMetricEvents(_ context.Context, query OverviewQuery) ([]MetricEvent, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()

	selected := make([]MetricEvent, 0, len(r.events))
	surfaces := surfaceSet(query.Surfaces)
	for _, event := range r.events {
		if event.OrgID != query.OrgID {
			continue
		}
		if _, ok := surfaces[event.Surface]; !ok {
			continue
		}
		if query.From != nil && event.OccurredAt.Before(*query.From) {
			continue
		}
		if query.To != nil && event.OccurredAt.After(*query.To) {
			continue
		}
		selected = append(selected, copyMetricEvent(event))
	}
	return selected, nil
}

func (r *MemoryRepository) ListConnectorSlots(_ context.Context, _ string) ([]ConnectorSlot, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	return copyConnectorSlots(r.connectors), nil
}
