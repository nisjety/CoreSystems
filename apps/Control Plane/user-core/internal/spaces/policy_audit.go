package spaces

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// AuditSubjectSpaceEffectPolicyUpdated is the JetStream subject (inside
// user-core's authority prefix, already allowed by nats.conf) for an effective
// change to an org's Space effect-policy.
const AuditSubjectSpaceEffectPolicyUpdated = "verevon.audit.v2.control.user-core.space_effect_policy_updated"

// Policy event sources and change kinds (mirrored by migration 033 CHECKs).
const (
	PolicyEventSourceOrgAdmin = "org_admin"
	PolicyEventSourceOperator = "operator"

	PolicyChangeEntitlementsSet = "entitlements_set"
	PolicyChangePresetApplied   = "preset_applied"
	PolicyChangeOperatorCreated = "operator_created"
	PolicyChangeOperatorUpdated = "operator_updated"
)

// EffectPolicyActor is the authenticated principal that made a policy change.
// Type is "user" or "service"; ID is the user id or the service principal id.
type EffectPolicyActor struct {
	Type string
	ID   string
	// OrgRole is the canonical org role for a user actor ("owner" or "admin");
	// empty for a service actor.
	OrgRole string
}

// Validate refuses an actor-less write: every policy change names its actor.
func (a EffectPolicyActor) Validate() error {
	if a.Type != "user" && a.Type != "service" {
		return fmt.Errorf("space effect policy actor type must be user or service")
	}
	if strings.TrimSpace(a.ID) == "" {
		return fmt.Errorf("space effect policy actor id is required")
	}
	return nil
}

// auditUserID renders the actor the way the audit event's user_id carries it.
func (a EffectPolicyActor) auditUserID() string {
	if a.Type == "service" {
		return "service:" + a.ID
	}
	return a.ID
}

func (a EffectPolicyActor) auditRole() string {
	if a.Type == "service" {
		return "operator"
	}
	return a.OrgRole
}

// PolicyAuditOutbox lets this package insert a user_audit_outbox row on the
// caller's transaction without importing internal/users. users.Repository
// implements it (see users.NewSpaceAuditEnqueuer).
type PolicyAuditOutbox interface {
	EnqueueAuditTx(ctx context.Context, tx pgx.Tx, eventID, subject string, payload []byte) error
}

// policyEvent is one row of space_effect_policy_events plus what the audit
// payload needs.
type policyEvent struct {
	OrgID          string
	IdempotencyKey string
	RequestDigest  string
	Source         string
	Actor          EffectPolicyActor
	ChangeKind     string
	PresetID       string
	Before         map[string]bool
	After          map[string]bool
	Changes        []EntitlementChange
	PrivacyChanged bool
	PolicyRevAfter int64
	EntRevBefore   int64
	EntRevAfter    int64
	SpacesFenced   int64
}

// insertPolicyEventTx appends the immutable event row and enqueues the audit
// outbox row on the same transaction, so a failure of either rolls the policy
// change back. It returns the new event id.
func insertPolicyEventTx(ctx context.Context, tx pgx.Tx, outbox PolicyAuditOutbox, ev policyEvent, now time.Time) (int64, error) {
	if outbox == nil {
		return 0, fmt.Errorf("space effect policy audit outbox is not wired")
	}
	before, err := json.Marshal(ev.Before)
	if err != nil {
		return 0, fmt.Errorf("encode before entitlements: %w", err)
	}
	after, err := json.Marshal(ev.After)
	if err != nil {
		return 0, fmt.Errorf("encode after entitlements: %w", err)
	}
	var eventID int64
	err = tx.QueryRow(ctx, `
		INSERT INTO space_effect_policy_events
		(org_id, idempotency_key, request_digest, source, actor_subject_type, actor_subject_id,
		 actor_org_role, change_kind, preset_id, before_entitlements, after_entitlements,
		 privacy_changed, policy_revision_after, entitlement_revision_after, spaces_fenced, occurred_at)
		VALUES ($1, NULLIF($2,''), NULLIF($3,''), $4, $5, $6, NULLIF($7,''), $8, NULLIF($9,''),
		        $10::jsonb, $11::jsonb, $12, $13, $14, $15, $16)
		RETURNING event_id`,
		ev.OrgID, ev.IdempotencyKey, ev.RequestDigest, ev.Source, ev.Actor.Type, ev.Actor.ID,
		ev.Actor.OrgRole, ev.ChangeKind, ev.PresetID, string(before), string(after),
		ev.PrivacyChanged, ev.PolicyRevAfter, ev.EntRevAfter, int32(ev.SpacesFenced), now,
	).Scan(&eventID)
	if err != nil {
		return 0, fmt.Errorf("insert Space effect policy event: %w", err)
	}
	payload, err := buildPolicyAuditPayload(eventID, ev, now)
	if err != nil {
		return 0, err
	}
	if err := outbox.EnqueueAuditTx(ctx, tx, fmt.Sprintf("space-effect-policy:%d", eventID),
		AuditSubjectSpaceEffectPolicyUpdated, payload); err != nil {
		return 0, fmt.Errorf("enqueue Space effect policy audit: %w", err)
	}
	return eventID, nil
}

// buildPolicyAuditPayload renders the audit-core event (events.AuditEvent
// shape). Booleans and key names only: no content, so Zero Data Retention
// needs no special handling.
func buildPolicyAuditPayload(eventID int64, ev policyEvent, now time.Time) ([]byte, error) {
	changed := make([]map[string]any, 0, len(ev.Changes))
	for _, ch := range ev.Changes {
		changed = append(changed, map[string]any{"key": ch.Key, "from": ch.From, "to": ch.To})
	}
	details := map[string]any{
		"source":                      ev.Source,
		"change_kind":                 ev.ChangeKind,
		"changed":                     changed,
		"privacy_changed":             ev.PrivacyChanged,
		"entitlement_revision_before": ev.EntRevBefore,
		"entitlement_revision_after":  ev.EntRevAfter,
		"spaces_fenced":               ev.SpacesFenced,
	}
	if ev.PresetID != "" {
		details["preset_id"] = ev.PresetID
	}
	payload := map[string]any{
		"event_id":    fmt.Sprintf("space-effect-policy:%d", eventID),
		"occurred_at": now.UTC().Format(time.RFC3339Nano),
		"org_id":      ev.OrgID,
		"user_id":     ev.Actor.auditUserID(),
		"actor_role":  ev.Actor.auditRole(),
		"plane":       "control",
		"producer":    "user-core",
		"event":       "space_effect_policy_updated",
		"subject":     "org:" + ev.OrgID,
		"resource_id": ev.OrgID,
		"outcome":     "ok",
		"details":     details,
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("encode Space effect policy audit event: %w", err)
	}
	return encoded, nil
}
