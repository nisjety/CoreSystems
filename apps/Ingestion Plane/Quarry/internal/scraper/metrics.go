package scraper

import (
	"sync"

	"github.com/triodelab/quarry/internal/models"
)

type MetricsStore struct {
	mu      sync.RWMutex
	metrics []*models.EnrichmentMetrics
}

func NewMetricsStore() *MetricsStore {
	return &MetricsStore{
		metrics: make([]*models.EnrichmentMetrics, 0),
	}
}

func (m *MetricsStore) AddMetric(metric *models.EnrichmentMetrics) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.metrics = append(m.metrics, metric)
}

func (m *MetricsStore) GetMetrics() []*models.EnrichmentMetrics {
	m.mu.RLock()
	defer m.mu.RUnlock()

	// Return last 20 metrics
	start := 0
	if len(m.metrics) > 20 {
		start = len(m.metrics) - 20
	}

	result := make([]*models.EnrichmentMetrics, len(m.metrics[start:]))
	copy(result, m.metrics[start:])
	return result
}

func (m *MetricsStore) GetAllMetrics() []*models.EnrichmentMetrics {
	m.mu.RLock()
	defer m.mu.RUnlock()

	result := make([]*models.EnrichmentMetrics, len(m.metrics))
	copy(result, m.metrics)
	return result
}

func (m *MetricsStore) GetLatest() *models.EnrichmentMetrics {
	m.mu.RLock()
	defer m.mu.RUnlock()

	if len(m.metrics) == 0 {
		return nil
	}
	return m.metrics[len(m.metrics)-1]
}
