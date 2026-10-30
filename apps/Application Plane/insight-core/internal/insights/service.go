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

// ListGroundableMarketingEvidence returns only marketing observations that
// meet marketing_evidence.v1. It is the sole query surface Model Plane code
// should use for marketing grounding; dashboards retain legacy observations
// for historical continuity without treating them as AI evidence.
func (s *Service) ListGroundableMarketingEvidence(ctx context.Context, query OverviewQuery) ([]MetricEvent, error) {
	normalized, err := normalizeOverviewQuery(query)
	if err != nil {
		return nil, err
	}
	normalized.Surfaces = []string{SurfaceExternalAnalytics}
	events, err := s.repository.ListMetricEvents(ctx, normalized)
	if err != nil {
		return nil, err
	}
	evidence := make([]MetricEvent, 0, len(events))
	for _, event := range events {
		if !isGroundableMarketingEvidence(event) {
			continue
		}
		evidence = append(evidence, copyMetricEvent(event))
	}
	return evidence, nil
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
	if !isSupportedSurface(surface) {
		return MetricEvent{}, fmt.Errorf("%w: unsupported surface %q", ErrInvalidInput, surface)
	}
	occurredAt := input.OccurredAt
	if occurredAt.IsZero() {
		occurredAt = now().UTC()
	} else {
		occurredAt = occurredAt.UTC()
	}
	observedAt := input.ObservedAt
	if observedAt.IsZero() {
		observedAt = occurredAt
	} else {
		observedAt = observedAt.UTC()
	}
	timeWindow, err := normalizeMetricTimeWindow(input.TimeWindow)
	if err != nil {
		return MetricEvent{}, err
	}
	event := MetricEvent{
		ID:             strings.TrimSpace(input.ID),
		OrgID:          orgID,
		ActorUserID:    strings.TrimSpace(input.ActorUserID),
		Surface:        surface,
		Metric:         metric,
		Value:          input.Value,
		Unit:           normalizeToken(input.Unit),
		Source:         strings.TrimSpace(input.Source),
		ConnectorType:  normalizeToken(input.ConnectorType),
		Dimensions:     copyMap(input.Dimensions),
		TimeWindow:     timeWindow,
		ObservedAt:     observedAt,
		SourceTraceRef: strings.TrimSpace(input.SourceTraceRef),
		RetentionClass: normalizeToken(input.RetentionClass),
		OccurredAt:     occurredAt,
	}
	if requiresMarketingEvidence(event.ConnectorType) {
		if event.TimeWindow == nil || event.TimeWindow.From == nil || event.TimeWindow.To == nil || event.SourceTraceRef == "" || event.RetentionClass != "durable" {
			return MetricEvent{}, fmt.Errorf("%w: marketing metrics require a time_window, observed_at, source_trace_ref, and durable retention_class", ErrInvalidInput)
		}
		if len(event.SourceTraceRef) > 512 || strings.ContainsAny(event.SourceTraceRef, "\r\n") {
			return MetricEvent{}, fmt.Errorf("%w: source_trace_ref is invalid", ErrInvalidInput)
		}
	}
	if event.ID == "" {
		event.ID = stableEventID(event)
	}
	return event, nil
}

func normalizeMetricTimeWindow(input *TimeWindow) (*TimeWindow, error) {
	if input == nil {
		return nil, nil
	}
	if input.From == nil || input.To == nil {
		return nil, fmt.Errorf("%w: time_window requires from and to", ErrInvalidInput)
	}
	from := input.From.UTC()
	to := input.To.UTC()
	if from.After(to) {
		return nil, fmt.Errorf("%w: time_window from must be before to", ErrInvalidInput)
	}
	return &TimeWindow{From: &from, To: &to}, nil
}

func requiresMarketingEvidence(connectorType string) bool {
	switch connectorType {
	case "google_analytics_4", "google_search_console", "google_ads", "meta", "meta_ads", "semrush", "first_party_seo":
		return true
	default:
		return false
	}
}

func isGroundableMarketingEvidence(event MetricEvent) bool {
	return event.Surface == SurfaceExternalAnalytics &&
		requiresMarketingEvidence(event.ConnectorType) &&
		event.TimeWindow != nil && event.TimeWindow.From != nil && event.TimeWindow.To != nil &&
		!event.ObservedAt.IsZero() &&
		strings.TrimSpace(event.SourceTraceRef) != "" &&
		event.RetentionClass == "durable"
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
	}
	aggregates := map[string]aggregate{}
	for _, event := range events {
		current, ok := aggregates[event.Metric]
		if !ok {
			current.sources = map[string]struct{}{}
		}
		current.value += event.Value
		if current.unit == "" {
			current.unit = event.Unit
		}
		// Cite the real producer(s) behind the events — never invent attribution.
		if source := strings.TrimSpace(event.Source); source != "" {
			current.sources[source] = struct{}{}
		}
		aggregates[event.Metric] = current
	}
	metrics := make([]MetricRollup, 0, len(aggregates))
	for metric, aggregate := range aggregates {
		metrics = append(metrics, MetricRollup{
			Metric: metric,
			Value:  aggregate.value,
			Unit:   aggregate.unit,
			Source: joinSources(aggregate.sources),
		})
	}
	sort.Slice(metrics, func(i, j int) bool {
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
		for _, metric := range surface.Metrics {
			scorecards = append(scorecards, Scorecard{
				ID:      surface.Surface + "." + metric.Metric,
				Label:   scorecardLabel(surface.Surface, metric.Metric),
				Surface: surface.Surface,
				Metric:  metric.Metric,
				Value:   metric.Value,
				Unit:    metric.Unit,
				Source:  metric.Source,
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
		case ConnectorStatusRequiresGovernedAction:
			gaps = append(gaps, ConnectorGap{
				Type:      connector.Type,
				Reason:    "connector requires an allowlisted integration-core governed action before provider data can be projected",
				NextOwner: "integration-corev2 provider action adapter",
			})
		case ConnectorStatusRequiresCredentialBroker:
			gaps = append(gaps, ConnectorGap{
				Type:      connector.Type,
				Reason:    "connector requires encrypted credential storage and a brokered API-key lease before use",
				NextOwner: "integration-corev2 credential broker",
			})
		case ConnectorStatusRequiresEvidenceAdapter:
			gaps = append(gaps, ConnectorGap{
				Type:      connector.Type,
				Reason:    "connector requires an authorized Ingestion/Data Plane evidence adapter; insight-core cannot persist crawls",
				NextOwner: "Quarry-v2 and Data Plane v2 evidence contract",
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
	// "Conversations handled" is the pilot's first headline number (see
	// verevon-feature-map.md's Insights "done-enough gate"): the real count of
	// conversation-core tickets marked resolved.
	"inbox.tickets_resolved": "Conversations handled",
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
