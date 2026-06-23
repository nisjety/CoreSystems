package insights

import (
	"context"
	"errors"
	"time"
)

const (
	SurfaceSocial            = "social"
	SurfaceInbox             = "inbox"
	SurfaceAgents            = "agents"
	SurfaceCampaigns         = "campaigns"
	SurfaceExternalAnalytics = "external_analytics"

	ConnectorStatusNative             = "native"
	ConnectorStatusPlanned            = "planned"
	ConnectorStatusRequiresTokenLease = "requires_token_lease"
	ConnectorStatusDisabled           = "disabled"
)

var ErrInvalidInput = errors.New("invalid input")

type Repository interface {
	RecordMetricEvent(ctx context.Context, event MetricEvent) (*MetricEvent, error)
	ListMetricEvents(ctx context.Context, query OverviewQuery) ([]MetricEvent, error)
	ListConnectorSlots(ctx context.Context, orgID string) ([]ConnectorSlot, error)
	// ListOrgIDsWithMetricsSince returns the distinct org_ids that have at least
	// one recorded metric event at or after `since`. It is the server-side org
	// discovery the scheduled-brief delivery iterates — orgs are derived from the
	// real recorded data, never from client input, so brief delivery stays
	// IDOR-clean (no cross-tenant fan-out, no fabricated recipients).
	ListOrgIDsWithMetricsSince(ctx context.Context, since time.Time) ([]string, error)
}

type OverviewQuery struct {
	OrgID    string
	Surfaces []string
	From     *time.Time
	To       *time.Time
}

type IngestMetricEventInput struct {
	ID            string         `json:"id"`
	OrgID         string         `json:"org_id"`
	Surface       string         `json:"surface"`
	Metric        string         `json:"metric"`
	Value         float64        `json:"value"`
	Unit          string         `json:"unit,omitempty"`
	Source        string         `json:"source,omitempty"`
	ConnectorType string         `json:"connector_type,omitempty"`
	Dimensions    map[string]any `json:"dimensions,omitempty"`
	OccurredAt    time.Time      `json:"occurred_at"`
}

type MetricEvent struct {
	ID            string         `json:"id"`
	OrgID         string         `json:"org_id"`
	Surface       string         `json:"surface"`
	Metric        string         `json:"metric"`
	Value         float64        `json:"value"`
	Unit          string         `json:"unit,omitempty"`
	Source        string         `json:"source,omitempty"`
	ConnectorType string         `json:"connector_type,omitempty"`
	Dimensions    map[string]any `json:"dimensions,omitempty"`
	OccurredAt    time.Time      `json:"occurred_at"`
}

type Overview struct {
	OrgID        string            `json:"org_id"`
	Plane        PlanePlacement    `json:"plane"`
	Surfaces     []SurfaceOverview `json:"surfaces"`
	Scorecards   []Scorecard       `json:"scorecards"`
	Connectors   []ConnectorSlot   `json:"connectors"`
	GeneratedAt  time.Time         `json:"generated_at"`
	Window       TimeWindow        `json:"window"`
	ConnectorLag []ConnectorGap    `json:"connector_lag"`
}

type TimeWindow struct {
	From *time.Time `json:"from,omitempty"`
	To   *time.Time `json:"to,omitempty"`
}

type SurfaceOverview struct {
	Surface     string         `json:"surface"`
	TotalEvents int            `json:"total_events"`
	Metrics     []MetricRollup `json:"metrics"`
	LastEventAt *time.Time     `json:"last_event_at,omitempty"`
}

type MetricRollup struct {
	Metric string  `json:"metric"`
	Value  float64 `json:"value"`
	Unit   string  `json:"unit,omitempty"`
	// Source is the real producer(s) that emitted the underlying events for this
	// metric (e.g. "conversation-core"), for honest citation. Multiple distinct
	// producers are joined with ", " (sorted). Empty when no event carried a
	// source — never fabricated.
	Source string `json:"source,omitempty"`
}

type Scorecard struct {
	ID      string  `json:"id"`
	Label   string  `json:"label"`
	Surface string  `json:"surface"`
	Metric  string  `json:"metric"`
	Value   float64 `json:"value"`
	Unit    string  `json:"unit,omitempty"`
	// Source cites the real producer(s) behind this scorecard's value, threaded
	// from the rolled-up metric. Empty when unattributed — never fabricated.
	Source string `json:"source,omitempty"`
}

type ConnectorSlot struct {
	Type               string              `json:"type"`
	DisplayName        string              `json:"display_name"`
	Surface            string              `json:"surface"`
	Status             string              `json:"status"`
	Authorization      string              `json:"authorization"`
	TokenLeaseAudience string              `json:"token_lease_audience,omitempty"`
	RequiredEnv        []string            `json:"required_env,omitempty"`
	ReferenceURLs      []string            `json:"reference_urls,omitempty"`
	Contracts          []ConnectorContract `json:"contracts"`
}

type ConnectorContract struct {
	Name              string   `json:"name"`
	EndpointTemplate  string   `json:"endpoint_template"`
	Method            string   `json:"method"`
	RequestShape      string   `json:"request_shape"`
	ResponseShape     string   `json:"response_shape"`
	RequiredScopes    []string `json:"required_scopes,omitempty"`
	DimensionExamples []string `json:"dimension_examples,omitempty"`
	MetricExamples    []string `json:"metric_examples,omitempty"`
}

type ConnectorGap struct {
	Type      string `json:"type"`
	Reason    string `json:"reason"`
	NextOwner string `json:"next_owner"`
}

type PlanePlacement struct {
	ServicePlane     string   `json:"service_plane"`
	ControlPlaneRole string   `json:"control_plane_role"`
	Rules            []string `json:"rules"`
}
