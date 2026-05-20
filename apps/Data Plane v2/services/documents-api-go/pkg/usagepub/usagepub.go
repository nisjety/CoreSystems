// Package usagepub is the Phase A · A1.5 helper that every Go service
// uses to emit usage + audit events onto the shared `velion-nats` bus.
//
// The contract matches the subscriber-side struct in
// `apps/Control Plane/audit-core/internal/events`. The subjects are:
//
//   - `velion.usage.v1.<plane>.<op>`  — billable resource usage
//   - `velion.audit.v1.<plane>.<event>` — operational / security audit
//
// Failures (no connection, serialization error, NATS publish error)
// are logged but never propagate back to the caller. Telemetry events
// are observability noise, not state changes — a missed publish
// degrades the audit log but should never fail a user-facing request.
//
// Usage:
//
//	pub := usagepub.New(nc, "data-plane")
//	pub.Usage(ctx, "documents.create", usagepub.Usage{
//	    OrgID:    claims.OrgID,
//	    UserID:   claims.UserID,
//	    BytesIn:  size,
//	    BytesOut: 0,
//	})
//	pub.Audit(ctx, "document.deleted", usagepub.Audit{
//	    OrgID:    claims.OrgID,
//	    UserID:   claims.UserID,
//	    Subject:  "documents",
//	    ResourceID: docID,
//	    Outcome:  "ok",
//	})
package usagepub

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

// Publisher wraps the NATS connection with the two convenience methods.
// Construct one per service via [New] and stash it on the handler
// struct alongside the existing repo / publisher dependencies.
type Publisher struct {
	nc    *nats.Conn
	plane string
}

// New returns a Publisher bound to the supplied plane name (e.g.
// "data-plane", "model-plane", "ingestion"). The plane is embedded in
// the subject as well as the payload so subscribers can subject-filter
// when they only care about one upstream.
func New(nc *nats.Conn, plane string) *Publisher {
	return &Publisher{nc: nc, plane: plane}
}

// Usage is the payload shape for billable usage events. Mirrors
// `events.UsageEvent` on the subscriber side; keep them in sync.
type Usage struct {
	OrgID     string         `json:"org_id"`
	UserID    string         `json:"user_id,omitempty"`
	TokensIn  int64          `json:"tokens_in,omitempty"`
	TokensOut int64          `json:"tokens_out,omitempty"`
	BytesIn   int64          `json:"bytes_in,omitempty"`
	BytesOut  int64          `json:"bytes_out,omitempty"`
	CostCents float64        `json:"cost_cents,omitempty"`
	RequestID string         `json:"request_id,omitempty"`
	Metadata  map[string]any `json:"metadata,omitempty"`
}

// Audit is the payload shape for operational / security audit events.
type Audit struct {
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	ActorRole  string         `json:"actor_role,omitempty"`
	Subject    string         `json:"subject,omitempty"`
	ResourceID string         `json:"resource_id,omitempty"`
	Outcome    string         `json:"outcome,omitempty"`
	Details    map[string]any `json:"details,omitempty"`
	RequestID  string         `json:"request_id,omitempty"`
	IPAddress  string         `json:"ip_address,omitempty"`
	UserAgent  string         `json:"user_agent,omitempty"`
}

// Usage emits a usage event. `op` is the operation tag (e.g.
// "documents.list", "retrieval.search") and becomes the trailing
// subject segment.
func (p *Publisher) Usage(_ context.Context, op string, u Usage) {
	if p == nil || p.nc == nil {
		return
	}
	if u.OrgID == "" || op == "" {
		log.Warn().Str("plane", p.plane).Str("op", op).Msg("usagepub: drop event with empty org_id or op")
		return
	}
	subject := fmt.Sprintf("velion.usage.v1.%s.%s", p.plane, op)
	body := struct {
		OccurredAt time.Time `json:"occurred_at"`
		Plane      string    `json:"plane"`
		Op         string    `json:"op"`
		Usage
	}{
		OccurredAt: time.Now().UTC(),
		Plane:      p.plane,
		Op:         op,
		Usage:      u,
	}
	data, err := json.Marshal(body)
	if err != nil {
		log.Warn().Err(err).Msg("usagepub: marshal failed")
		return
	}
	if err := p.nc.Publish(subject, data); err != nil {
		log.Warn().Err(err).Str("subject", subject).Msg("usagepub: publish failed")
	}
}

// Audit emits an audit event. `event` is the event name (e.g.
// "document.deleted", "session.created") and becomes the trailing
// subject segment.
func (p *Publisher) Audit(_ context.Context, event string, a Audit) {
	if p == nil || p.nc == nil {
		return
	}
	if a.OrgID == "" || event == "" {
		log.Warn().Str("plane", p.plane).Str("event", event).Msg("usagepub: drop audit with empty org_id or event")
		return
	}
	if a.Outcome == "" {
		a.Outcome = "ok"
	}
	subject := fmt.Sprintf("velion.audit.v1.%s.%s", p.plane, event)
	body := struct {
		OccurredAt time.Time `json:"occurred_at"`
		Plane      string    `json:"plane"`
		Event      string    `json:"event"`
		Audit
	}{
		OccurredAt: time.Now().UTC(),
		Plane:      p.plane,
		Event:      event,
		Audit:      a,
	}
	data, err := json.Marshal(body)
	if err != nil {
		log.Warn().Err(err).Msg("usagepub: marshal audit failed")
		return
	}
	if err := p.nc.Publish(subject, data); err != nil {
		log.Warn().Err(err).Str("subject", subject).Msg("usagepub: audit publish failed")
	}
}
