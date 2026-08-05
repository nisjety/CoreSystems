// Package events defines the wire shapes for audit + usage events.
// Mirrored on the producing side by every plane that emits to NATS
// (`verevon.audit.v2.<plane>.<producer>.<event>` and
// `verevon.usage.v2.<plane>.<producer>.<op>`).
//
// Keeping the structs in one shared package — even if it's only consumed
// by audit-core today — means future Go services that produce events
// can import it directly to avoid the wire-shape drift that ate the
// quarry-orchestrator schedules reconciler.
package events

import (
	"encoding/json"
	"fmt"
	"regexp"
	"time"
)

const (
	MaxEventIDLength      = 128
	MaxUsageEventIDLength = MaxEventIDLength
	MaxAuthorityLength    = 64
)

var (
	stableEventIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$`)
	authorityPattern     = regexp.MustCompile(`^[a-z0-9][a-z0-9_-]{0,63}$`)
)

// AuditEvent represents a single security/operational audit record
// emitted by a plane. The plane is responsible for filling these fields
// with verified data — i.e. `OrgID` MUST come from the auth-core JWT
// claim, never from a client-supplied header.
type AuditEvent struct {
	OccurredAt time.Time      `json:"occurred_at"`
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	ActorRole  string         `json:"actor_role,omitempty"`
	Plane      string         `json:"plane"`
	Producer   string         `json:"producer"`
	Event      string         `json:"event"`
	Subject    string         `json:"subject,omitempty"`
	ResourceID string         `json:"resource_id,omitempty"`
	Outcome    string         `json:"outcome,omitempty"` // ok | denied | error
	Details    map[string]any `json:"details,omitempty"`
	EventID    string         `json:"event_id"`
	RequestID  string         `json:"request_id,omitempty"`
	IPAddress  string         `json:"ip_address,omitempty"`
	UserAgent  string         `json:"user_agent,omitempty"`
}

// UsageEvent represents a single billable / observable resource usage
// record. Cost-core aggregates these by (org_id, plane, op, period) and
// surfaces totals to the verevon usage dashboard.
type UsageEvent struct {
	EventID    string         `json:"event_id"`
	OccurredAt time.Time      `json:"occurred_at"`
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	Plane      string         `json:"plane"`
	Producer   string         `json:"producer"`
	Op         string         `json:"op"`
	TokensIn   int64          `json:"tokens_in,omitempty"`
	TokensOut  int64          `json:"tokens_out,omitempty"`
	BytesIn    int64          `json:"bytes_in,omitempty"`
	BytesOut   int64          `json:"bytes_out,omitempty"`
	CostCents  float64        `json:"cost_cents,omitempty"`
	RequestID  string         `json:"request_id,omitempty"`
	Metadata   map[string]any `json:"metadata,omitempty"`
}

// Validate enforces the minimal invariants every event must satisfy.
// Returns an error so the subscriber can drop malformed payloads with
// a clear log message instead of writing partial rows.
func (e *AuditEvent) Validate() error {
	if err := validateStableEventID(e.EventID); err != nil {
		return err
	}
	if e.OccurredAt.IsZero() {
		return errMissingField("occurred_at")
	}
	if e.OrgID == "" {
		return errMissingField("org_id")
	}
	if e.Plane == "" {
		return errMissingField("plane")
	}
	if err := validateAuthority("producer", e.Producer); err != nil {
		return err
	}
	if e.Event == "" {
		return errMissingField("event")
	}
	if e.Outcome == "" {
		e.Outcome = "ok"
	}
	return nil
}

func (e *UsageEvent) Validate() error {
	if err := validateStableEventID(e.EventID); err != nil {
		return err
	}
	if e.OrgID == "" {
		return errMissingField("org_id")
	}
	if e.Plane == "" {
		return errMissingField("plane")
	}
	if err := validateAuthority("producer", e.Producer); err != nil {
		return err
	}
	if e.Op == "" {
		return errMissingField("op")
	}
	if e.OccurredAt.IsZero() {
		return errMissingField("occurred_at")
	}
	return nil
}

func validateStableEventID(eventID string) error {
	if eventID == "" {
		return errMissingField("event_id")
	}
	if len(eventID) > MaxEventIDLength || !stableEventIDPattern.MatchString(eventID) {
		return fmt.Errorf("event_id must be a stable identifier of at most %d bytes", MaxEventIDLength)
	}
	return nil
}

func validateAuthority(field, value string) error {
	if value == "" {
		return errMissingField(field)
	}
	if len(value) > MaxAuthorityLength || !authorityPattern.MatchString(value) {
		return fmt.Errorf("%s must be a lowercase authority token of at most %d bytes", field, MaxAuthorityLength)
	}
	return nil
}

type missingFieldError struct{ field string }

func (e *missingFieldError) Error() string { return "missing required field: " + e.field }

func errMissingField(name string) error { return &missingFieldError{field: name} }

// DecodeAudit decodes a NATS message payload into an AuditEvent. Kept
// next to the struct so producers + consumers share the same JSON
// settings (which today is the stdlib default — explicit so future
// changes are made deliberately).
func DecodeAudit(data []byte) (*AuditEvent, error) {
	var ev AuditEvent
	if err := json.Unmarshal(data, &ev); err != nil {
		return nil, err
	}
	if err := ev.Validate(); err != nil {
		return nil, err
	}
	return &ev, nil
}

func DecodeUsage(data []byte) (*UsageEvent, error) {
	var ev UsageEvent
	if err := json.Unmarshal(data, &ev); err != nil {
		return nil, err
	}
	if err := ev.Validate(); err != nil {
		return nil, err
	}
	return &ev, nil
}
