package insights

import (
	"context"
	"fmt"
	"math"
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

// RecordMetricSnapshot replaces deterministic provider snapshot rows when a
// source corrects its data. Lifecycle events remain append-only.
func (s *Service) RecordMetricSnapshot(ctx context.Context, input IngestMetricEventInput) (*MetricEvent, error) {
	event, err := normalizeMetricEvent(input, s.now)
	if err != nil {
		return nil, err
	}
	if event.Surface != SurfaceExternalAnalytics || event.Source != "social-core" {
		return nil, fmt.Errorf("%w: replaceable snapshots must come from social-core external analytics", ErrInvalidInput)
	}
	return s.repository.RecordMetricSnapshot(ctx, event)
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
		ContractVersion: "2.0",
		OrgID:           normalized.OrgID,
		Plane:           OwnershipDecision(),
		Surfaces:        surfaces,
		Scorecards:      buildScorecards(surfaces),
		Connectors:      connectors,
		GeneratedAt:     s.now().UTC(),
		Window:          TimeWindow{From: normalized.From, To: normalized.To},
		ConnectorLag:    connectorGaps(connectors),
		Coverage:        "unknown",
		CoverageDetail:  "Source delivery checkpoints are not yet available. Recorded rows show observed activity, but do not certify that every source event was delivered.",
	}, nil
}

func (s *Service) ListConnectorSlots(ctx context.Context, orgID string) ([]ConnectorSlot, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListConnectorSlots(ctx, orgID)
}

// OrgsWithRecentMetrics returns the org_ids that recorded at least one metric
// event within the trailing `window` (measured from the service clock). It is
// the server-side org discovery the scheduled-brief delivery iterates: orgs are
// derived from real recorded data only, never from caller input.
func (s *Service) OrgsWithRecentMetrics(ctx context.Context, window time.Duration) ([]string, error) {
	if window <= 0 {
		return nil, fmt.Errorf("%w: window must be positive", ErrInvalidInput)
	}
	since := s.now().UTC().Add(-window)
	return s.repository.ListOrgIDsWithMetricsSince(ctx, since)
}

func normalizeMetricEvent(input IngestMetricEventInput, now func() time.Time) (MetricEvent, error) {
	orgID := strings.TrimSpace(input.OrgID)
	surface := normalizeSurface(input.Surface)
	metric := normalizeToken(input.Metric)
	if orgID == "" || surface == "" || metric == "" {
		return MetricEvent{}, fmt.Errorf("%w: org_id, surface, and metric are required", ErrInvalidInput)
	}
	if math.IsNaN(input.Value) || math.IsInf(input.Value, 0) {
		return MetricEvent{}, fmt.Errorf("%w: metric value must be finite", ErrInvalidInput)
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
		ActorUserID:   strings.TrimSpace(input.ActorUserID),
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
		OrgID:       orgID,
		ActorUserID: strings.TrimSpace(query.ActorUserID),
		Surfaces:    surfaces,
		From:        query.From,
		To:          query.To,
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
		value   float64
		unit    string
		sources map[string]struct{}
		count   int
	}
	aggregates := map[string]aggregate{}
	for _, event := range events {
		unit := event.Unit
		if event.Surface == SurfaceExternalAnalytics && event.Source == "social-core" && unit == "count" {
			// Older ingestion mislabeled every provider metric as count. Keep
			// those historical rows available but prevent false numeric claims.
			unit = "unverified"
		}
		key := event.Metric + "\x00" + unit
		current, ok := aggregates[key]
		if !ok {
			current.sources = map[string]struct{}{}
		}
		current.value += event.Value
		current.count++
		if current.unit == "" {
			current.unit = unit
		}
		// Cite the real producer(s) behind the events — never invent attribution.
		if source := strings.TrimSpace(event.Source); source != "" {
			current.sources[source] = struct{}{}
		}
		aggregates[key] = current
	}
	metrics := make([]MetricRollup, 0, len(aggregates))
	for key, aggregate := range aggregates {
		metric := strings.SplitN(key, "\x00", 2)[0]
		aggregation := "sum_of_recorded_events"
		if aggregate.unit == "unverified" {
			aggregation = "unverified"
		}
		metrics = append(metrics, MetricRollup{
			Metric:      metric,
			Value:       aggregate.value,
			Unit:        aggregate.unit,
			Source:      joinSources(aggregate.sources),
			EventCount:  aggregate.count,
			Aggregation: aggregation,
		})
	}
	sort.Slice(metrics, func(i, j int) bool {
		if metrics[i].Metric == metrics[j].Metric {
			return metrics[i].Unit < metrics[j].Unit
		}
		return metrics[i].Metric < metrics[j].Metric
	})
	return metrics
}

// joinSources renders the distinct producer set as a stable, comma-separated
// citation ("conversation-core, social-core"). Empty when no event carried a
// source — the honest "unattributed" state, never a fabricated producer.
func joinSources(sources map[string]struct{}) string {
	if len(sources) == 0 {
		return ""
	}
	list := make([]string, 0, len(sources))
	for source := range sources {
		list = append(list, source)
	}
	sort.Strings(list)
	return strings.Join(list, ", ")
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
		metricCounts := map[string]int{}
		for _, metric := range surface.Metrics {
			metricCounts[metric.Metric]++
		}
		for _, metric := range surface.Metrics {
			id := surface.Surface + "." + metric.Metric
			if metricCounts[metric.Metric] > 1 {
				id += "." + strings.ReplaceAll(metric.Unit, " ", "_")
			}
			scorecards = append(scorecards, Scorecard{
				ID:          id,
				Label:       scorecardLabel(surface.Surface, metric.Metric),
				Definition:  scorecardDefinition(surface.Surface, metric.Metric),
				Surface:     surface.Surface,
				Metric:      metric.Metric,
				Value:       metric.Value,
				Unit:        metric.Unit,
				Source:      metric.Source,
				EventCount:  metric.EventCount,
				Aggregation: metric.Aggregation,
			})
		}
	}
	sort.Slice(scorecards, func(i, j int) bool {
		return scorecards[i].ID < scorecards[j].ID
	})
	return scorecards
}

// scorecardDefinition gives reports and model consumers the exact meaning of
// the small set of business-facing metrics. Unlisted event series remain
// described conservatively as recorded values rather than inferred outcomes.
func scorecardDefinition(surface, metric string) string {
	switch surface + "." + metric {
	case "inbox.tickets_resolved":
		return "Ticket-resolution events recorded by Conversation Core during the selected event-time window. This is not a distinct-ticket count and may include later resolutions after reopening."
	case "inbox.ai_actions_approved":
		return "AI action approval events recorded by Conversation Core during the selected event-time window; includes action types beyond reply drafts."
	case "inbox.ai_actions_rejected":
		return "AI action rejection events recorded by Conversation Core during the selected event-time window; includes action types beyond reply drafts."
	default:
		return "Sum of recorded source events for this metric and unit during the selected event-time window."
	}
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

// scorecardLabelOverrides gives a small set of scorecards a human-facing
// label distinct from the generic "surface metric" transform below. Keyed by
// "surface.metric" — the same key buildScorecards uses for Scorecard.ID — so
// an override only ever applies to the exact real metric it names. Metrics
// not listed here fall back to the generic transform; nothing here changes
// the underlying value, source, or unit — it is presentation only.
var scorecardLabelOverrides = map[string]string{
	"inbox.tickets_resolved": "Tickets resolved",
	// Feeds the "AI draft acceptance %" headline number as
	// approved / (approved + rejected); see metric_subscriber.go's
	// processAIActionReviewed for how these two metrics are recorded.
	"inbox.ai_actions_approved": "AI drafts approved",
	"inbox.ai_actions_rejected": "AI drafts rejected",
}

func scorecardLabel(surface, metric string) string {
	if label, ok := scorecardLabelOverrides[surface+"."+metric]; ok {
		return label
	}
	return strings.ReplaceAll(surface+" "+metric, "_", " ")
}
