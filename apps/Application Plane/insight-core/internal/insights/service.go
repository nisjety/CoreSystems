package insights

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"
)

type Service struct {
	repository Repository
	now        func() time.Time
}

type Option func(*Service)

func WithNow(now func() time.Time) Option {
	return func(s *Service) {
		if now != nil {
			s.now = now
		}
	}
}

func NewService(repository Repository, opts ...Option) *Service {
	service := &Service{
		repository: repository,
		now:        time.Now,
	}
	for _, opt := range opts {
		opt(service)
	}
	return service
}

func (s *Service) RecordMetricEvent(ctx context.Context, input IngestMetricEventInput) (*MetricEvent, error) {
	event, err := normalizeMetricEvent(input, s.now)
	if err != nil {
		return nil, err
	}
	return s.repository.RecordMetricEvent(ctx, event)
}

func (s *Service) Overview(ctx context.Context, query OverviewQuery) (*Overview, error) {
	normalized, err := normalizeOverviewQuery(query)
	if err != nil {
		return nil, err
	}
	events, err := s.repository.ListMetricEvents(ctx, normalized)
	if err != nil {
		return nil, err
	}
	connectors, err := s.repository.ListConnectorSlots(ctx, normalized.OrgID)
	if err != nil {
		return nil, err
	}
	surfaces := buildSurfaceOverviews(normalized.Surfaces, events)
	return &Overview{
		OrgID:        normalized.OrgID,
		Plane:        OwnershipDecision(),
		Surfaces:     surfaces,
		Scorecards:   buildScorecards(surfaces),
		Connectors:   connectors,
		GeneratedAt:  s.now().UTC(),
		Window:       TimeWindow{From: normalized.From, To: normalized.To},
		ConnectorLag: connectorGaps(connectors),
	}, nil
}

func (s *Service) ListConnectorSlots(ctx context.Context, orgID string) ([]ConnectorSlot, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListConnectorSlots(ctx, orgID)
}

func normalizeMetricEvent(input IngestMetricEventInput, now func() time.Time) (MetricEvent, error) {
	orgID := strings.TrimSpace(input.OrgID)
	surface := normalizeSurface(input.Surface)
	metric := normalizeToken(input.Metric)
	if orgID == "" || surface == "" || metric == "" {
		return MetricEvent{}, fmt.Errorf("%w: org_id, surface, and metric are required", ErrInvalidInput)
	}
	if !isSupportedSurface(surface) {
		return MetricEvent{}, fmt.Errorf("%w: unsupported surface %q", ErrInvalidInput, surface)
	}
	occurredAt := input.OccurredAt
	if occurredAt.IsZero() {
		occurredAt = now().UTC()
	} else {
		occurredAt = occurredAt.UTC()
	}
	event := MetricEvent{
		ID:            strings.TrimSpace(input.ID),
		OrgID:         orgID,
		Surface:       surface,
		Metric:        metric,
		Value:         input.Value,
		Unit:          normalizeToken(input.Unit),
		Source:        strings.TrimSpace(input.Source),
		ConnectorType: normalizeToken(input.ConnectorType),
		Dimensions:    copyMap(input.Dimensions),
		OccurredAt:    occurredAt,
	}
	if event.ID == "" {
		event.ID = stableEventID(event)
	}
	return event, nil
}

func normalizeOverviewQuery(query OverviewQuery) (OverviewQuery, error) {
	orgID := strings.TrimSpace(query.OrgID)
	if orgID == "" {
		return OverviewQuery{}, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	surfaces := normalizeSurfaceList(query.Surfaces)
	for _, surface := range surfaces {
		if !isSupportedSurface(surface) {
			return OverviewQuery{}, fmt.Errorf("%w: unsupported surface %q", ErrInvalidInput, surface)
		}
	}
	if query.From != nil && query.To != nil && query.From.After(*query.To) {
		return OverviewQuery{}, fmt.Errorf("%w: from must be before to", ErrInvalidInput)
	}
	return OverviewQuery{
		OrgID:    orgID,
		Surfaces: surfaces,
		From:     query.From,
		To:       query.To,
	}, nil
}

func buildSurfaceOverviews(surfaces []string, events []MetricEvent) []SurfaceOverview {
	bySurface := make(map[string][]MetricEvent, len(surfaces))
	for _, surface := range surfaces {
		bySurface[surface] = []MetricEvent{}
	}
	for _, event := range events {
		if _, ok := bySurface[event.Surface]; ok {
			bySurface[event.Surface] = append(bySurface[event.Surface], event)
		}
	}
	overviews := make([]SurfaceOverview, 0, len(surfaces))
	for _, surface := range surfaces {
		surfaceEvents := bySurface[surface]
		overviews = append(overviews, SurfaceOverview{
			Surface:     surface,
			TotalEvents: len(surfaceEvents),
			Metrics:     rollupMetrics(surfaceEvents),
			LastEventAt: lastEventAt(surfaceEvents),
		})
	}
	return overviews
}

func rollupMetrics(events []MetricEvent) []MetricRollup {
	type aggregate struct {
		value float64
		unit  string
	}
	aggregates := map[string]aggregate{}
	for _, event := range events {
		current := aggregates[event.Metric]
		current.value += event.Value
		if current.unit == "" {
			current.unit = event.Unit
		}
		aggregates[event.Metric] = current
	}
	metrics := make([]MetricRollup, 0, len(aggregates))
	for metric, aggregate := range aggregates {
		metrics = append(metrics, MetricRollup{Metric: metric, Value: aggregate.value, Unit: aggregate.unit})
	}
	sort.Slice(metrics, func(i, j int) bool {
		return metrics[i].Metric < metrics[j].Metric
	})
	return metrics
}

func lastEventAt(events []MetricEvent) *time.Time {
	var latest *time.Time
	for _, event := range events {
		if latest == nil || event.OccurredAt.After(*latest) {
			value := event.OccurredAt
			latest = &value
		}
	}
	return latest
}

func buildScorecards(surfaces []SurfaceOverview) []Scorecard {
	scorecards := []Scorecard{}
	for _, surface := range surfaces {
		for _, metric := range surface.Metrics {
			scorecards = append(scorecards, Scorecard{
				ID:      surface.Surface + "." + metric.Metric,
				Label:   scorecardLabel(surface.Surface, metric.Metric),
				Surface: surface.Surface,
				Metric:  metric.Metric,
				Value:   metric.Value,
				Unit:    metric.Unit,
			})
		}
	}
	sort.Slice(scorecards, func(i, j int) bool {
		return scorecards[i].ID < scorecards[j].ID
	})
	return scorecards
}

func connectorGaps(connectors []ConnectorSlot) []ConnectorGap {
	gaps := []ConnectorGap{}
	for _, connector := range connectors {
		switch connector.Status {
		case ConnectorStatusRequiresTokenLease:
			gaps = append(gaps, ConnectorGap{
				Type:      connector.Type,
				Reason:    "connector slot is defined but requires integration-core token leasing before provider calls are allowed",
				NextOwner: "integration-corev2 token lease plus provider worker",
			})
		case ConnectorStatusDisabled:
			gaps = append(gaps, ConnectorGap{
				Type:      connector.Type,
				Reason:    "connector contract is reserved but no provider adapter is selected",
				NextOwner: "provider-specific analytics connector",
			})
		}
	}
	return gaps
}

func scorecardLabel(surface, metric string) string {
	return strings.ReplaceAll(surface+" "+metric, "_", " ")
}
