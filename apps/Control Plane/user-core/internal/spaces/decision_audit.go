package spaces

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// WS05-S5: Space decision audit events.
//
// Control already refuses or issues every Space decision; this file turns that
// outcome into a content-free audit row on the user_audit_outbox (the seam
// WS03-S1 created), so it reaches audit-core in the shape the other Control
// producers use: verevon.audit.v2.control.user-core.<event>.
//
// Volume rule: denials for every issuer route, issued events only for the
// effect-bound kinds. A thread read, a retrieval or an append is never audited
// as "issued" (the transcript is the record). Details never hold the token, the
// payload digest, the nonce, recipient lists or any content.

const (
	// AuditSubjectSpaceDecisionDenied and AuditSubjectSpaceDecisionIssued are
	// JetStream subjects inside user-core's authority prefix.
	AuditSubjectSpaceDecisionDenied = "verevon.audit.v2.control.user-core.space_decision_denied"
	AuditSubjectSpaceDecisionIssued = "verevon.audit.v2.control.user-core.space_decision_issued"

	auditEventSpaceDecisionDenied = "space_decision_denied"
	auditEventSpaceDecisionIssued = "space_decision_issued"

	// DecisionAuditKindDenied and DecisionAuditKindIssued are the two tokens of
	// SPACE_AUDIT_DECISION_KINDS.
	DecisionAuditKindDenied = "denied"
	DecisionAuditKindIssued = "issued"

	maxAuditDetailString = 128
)

// DecisionAuditConfig is the rollout switch pair. Both are rollout controls,
// never authorization inputs: they never grant a decision, and never change
// whether Control's authority checks issue or refuse one. One availability
// effect is deliberate (B-REC-03): with the producer on and "issued" listed, an
// effect-bound decision whose issued row cannot be written to user_audit_outbox
// is withheld behind a typed 503 (space_decision_audit_unavailable), so an
// outbox outage then blocks effect-bound Space actions. With "issued" off no
// answer is held or changed.
//
//   - SPACE_AUDIT_DECISION_EVENTS: strict literal "true" turns the producer on.
//   - SPACE_AUDIT_DECISION_KINDS: comma list of "denied" and "issued". Unset or
//     empty means "denied" only (denials first, then effect-bound kinds).
//     Unknown tokens enable nothing.
type DecisionAuditConfig struct {
	Enabled bool
	Denied  bool
	Issued  bool
}

// LoadDecisionAuditConfig reads the two flags.
func LoadDecisionAuditConfig(getenv func(string) string) DecisionAuditConfig {
	cfg := DecisionAuditConfig{Enabled: strings.TrimSpace(getenv("SPACE_AUDIT_DECISION_EVENTS")) == "true"}
	if !cfg.Enabled {
		return DecisionAuditConfig{}
	}
	raw := strings.TrimSpace(getenv("SPACE_AUDIT_DECISION_KINDS"))
	if raw == "" {
		cfg.Denied = true
		return cfg
	}
	for _, token := range strings.Split(raw, ",") {
		switch strings.ToLower(strings.TrimSpace(token)) {
		case DecisionAuditKindDenied:
			cfg.Denied = true
		case DecisionAuditKindIssued:
			cfg.Issued = true
		}
	}
	return cfg
}

// DecisionAuditOutbox inserts one user_audit_outbox row. users.SpaceAuditEnqueuer
// implements it (no transaction: a refusal has no state change to join).
type DecisionAuditOutbox interface {
	EnqueueAudit(ctx context.Context, eventID, subject string, payload []byte) error
}

// DecisionAuditRoute is one issuer route. Name is the decision_kind carried in
// details; EffectBound says an issued decision of this route is audited (and,
// B-REC-03, that the issue fails closed when its audit row cannot be written);
// Batch says a 200 carries per-row results whose denied rows are audited.
type DecisionAuditRoute struct {
	Name        string
	EffectBound bool
	Batch       bool
}

// decisionAuditRoutes maps the path under /api/v1/internal/spaces to its row.
// Reads, retrieval and appends are issuer routes (their denials are audited)
// but never effect-bound. The batch thread-read-decisions route reports a
// per-row status in a 200 body: B-REC-03 audits each denied row of it, plus its
// own whole-batch refusals, like any other issuer route.
var decisionAuditRoutes = map[string]DecisionAuditRoute{
	"/personal-thread-decision":         {Name: "thread_create"},
	"/thread-decision":                  {Name: "thread_create"},
	"/thread-append-decision":           {Name: "thread_append"},
	"/run-wake-decision":                {Name: "run_wake"},
	"/run-control-decision":             {Name: "run_control", EffectBound: true},
	"/memory-decision":                  {Name: "space_memory", EffectBound: true},
	"/canvas-decision":                  {Name: "space_canvas", EffectBound: true},
	"/browser-run-decision":             {Name: "space_browser_run", EffectBound: true},
	"/browser-run-stop-decision":        {Name: "space_browser_run_stop", EffectBound: true},
	"/memory-declassify-decision":       {Name: "memory_declassify", EffectBound: true},
	"/memory-personal-context-decision": {Name: "memory_personal_context"},
	"/thread-read-decision":             {Name: "thread_read"},
	"/personal-retrieval-decision":      {Name: "retrieval"},
	"/retrieval-decision":               {Name: "retrieval"},
	"/watch-observe-decision":           {Name: "watch_observe"},
	"/personal-import-decision":         {Name: "import", EffectBound: true},
	"/import-decision":                  {Name: "import", EffectBound: true},
	"/import-execution-decision":        {Name: "import", EffectBound: true},
	"/schedule-create-decision":         {Name: "schedule_create", EffectBound: true},
	"/schedule-fire-decision":           {Name: "schedule_fire", EffectBound: true},
	"/watch-create-decision":            {Name: "watch_create", EffectBound: true},
	"/owner-grant-decision":             {Name: "owner_grant", EffectBound: true},
	"/sandbox-capability-decision":      {Name: "sandbox_capability", EffectBound: true},
	"/run-action-decision":              {Name: "run_action", EffectBound: true},
	"/scheduled-run-decision":           {Name: "scheduled_run", EffectBound: true},
	"/scheduled-run-execution-decision": {Name: "scheduled_run", EffectBound: true},
	"/scheduled-step-decision":          {Name: "scheduled_step", EffectBound: true},
	"/thread-read-decisions":            {Name: "thread_read", Batch: true},

	// WS10-S2: Control-held approval authority. A standing rule, a rule use, the
	// continuation fence and the gate view each record or authorize an effect, so
	// each is effect-bound: its issued row is written before the answer is released
	// (B-REC-03), and its refusals are audited like every issuer route's.
	"/approval-grants":             {Name: "approval_grant", EffectBound: true},
	"/approval-grants/use":         {Name: "approval_grant_use", EffectBound: true},
	"/approval-continuation-check": {Name: "approval_continuation", EffectBound: true},
	"/tool-gate-view":              {Name: "tool_gate_view", EffectBound: true},
}

// DecisionAuditRouteFor resolves a request path (full path or route pattern)
// to its issuer row. It matches the suffix after "/internal/spaces".
func DecisionAuditRouteFor(path string) (route string, row DecisionAuditRoute, ok bool) {
	const marker = "/internal/spaces"
	idx := strings.Index(path, marker)
	if idx < 0 {
		return "", DecisionAuditRoute{}, false
	}
	route = path[idx+len(marker):]
	row, ok = decisionAuditRoutes[route]
	return route, row, ok
}

// DeniedDecisionAudit is what the denial event is built from. Every field is an
// identifier or a closed-vocabulary word; none is content.
type DeniedDecisionAudit struct {
	Route        string
	Kind         string
	OrgID        string
	SubjectID    string
	SpaceRef     string
	ActionID     string
	Status       int
	Reason       string // the DenialCode, empty for an untyped refusal
	ResolvableBy string
	// ActorRole is the closed actor word (see the DecisionAuditActor* consts).
	ActorRole string
	// BatchRow marks one denied row of a 200 batch answer (Status is then 200).
	BatchRow   bool
	OccurredAt time.Time
}

// Closed actor_role words for decision audit rows (B-REC-03). The issuer
// routes are called by service principals; the role is who the call speaks for,
// never an authorization input.
const (
	DecisionAuditActorDelegatedUser = "delegated_user"
	DecisionAuditActorService       = "service"
	DecisionAuditActorUser          = "user"
)

// Batch row reasons that mean "Control could not decide", not "Control refused".
const batchRowReasonUnavailable = "unavailable"

// deniedOutcome maps a refusal onto audit-core's outcome words: an availability
// refusal (503, or a batch row Control could not resolve) is "error"; a policy
// or conflict refusal (403, 409, a denied batch row) is "denied".
func (in DeniedDecisionAudit) deniedOutcome() string {
	if in.Status == 503 || (in.BatchRow && in.Reason == batchRowReasonUnavailable) {
		return "error"
	}
	return "denied"
}

// BuildDeniedDecisionAudit renders the denial event. The id collapses retries:
// the same route, org, subject, Space and minute is one row. A 403 keeps the
// original id; any other status (409, 503, a batch row) folds the status in, so
// an availability refusal never collapses into a policy refusal of that minute.
func BuildDeniedDecisionAudit(in DeniedDecisionAudit) (eventID, subject string, payload []byte, err error) {
	if strings.TrimSpace(in.OrgID) == "" {
		return "", "", nil, fmt.Errorf("space decision denial audit needs an org")
	}
	at := in.OccurredAt.UTC()
	parts := []string{in.Route, in.OrgID, in.SubjectID, in.SpaceRef, fmt.Sprint(at.Unix() / 60)}
	if in.Status != 403 {
		parts = append(parts, fmt.Sprint(in.Status))
	}
	sum := sha256.Sum256([]byte(strings.Join(parts, "|")))
	eventID = "space-denial:" + hex.EncodeToString(sum[:])
	details := map[string]any{
		"decision_kind": in.Kind,
		"route":         in.Route,
		"status":        in.Status,
	}
	if in.ActionID != "" {
		details["action_id"] = clampAuditString(in.ActionID)
	}
	if in.Reason != "" {
		details["reason"] = clampAuditString(in.Reason)
	}
	if in.ResolvableBy != "" {
		details["resolvable_by"] = clampAuditString(in.ResolvableBy)
	}
	if in.BatchRow {
		details["batch_row"] = true
	}
	payload, err = encodeDecisionAudit(eventID, at, in.OrgID, in.SubjectID, in.ActorRole, auditEventSpaceDecisionDenied, in.SpaceRef, in.deniedOutcome(), details)
	return eventID, AuditSubjectSpaceDecisionDenied, payload, err
}

// IssuedDecisionAudit is the content-free projection of a signed decision.
type IssuedDecisionAudit struct {
	Route             string
	Kind              string
	DecisionRef       string
	OrgID             string
	SubjectID         string
	SpaceRef          string
	ActionID          string
	ServiceAudience   string
	AuthorityRevision int64
	ExpiresAt         time.Time
	ZeroDataRetention bool
	ActorRole         string
	OccurredAt        time.Time
}

// BuildIssuedDecisionAudit renders the issued event. The id is the decision
// reference, so a replayed response cannot produce a second row. It carries no
// token, payload digest, nonce, idempotency key or recipient reference.
func BuildIssuedDecisionAudit(in IssuedDecisionAudit) (eventID, subject string, payload []byte, err error) {
	if strings.TrimSpace(in.OrgID) == "" || strings.TrimSpace(in.DecisionRef) == "" {
		return "", "", nil, fmt.Errorf("space decision issued audit needs an org and a decision reference")
	}
	eventID = "space-decision:" + in.DecisionRef
	if len(eventID) > 128 {
		return "", "", nil, fmt.Errorf("space decision reference is too long for an audit event id")
	}
	details := map[string]any{
		"decision_kind":       in.Kind,
		"route":               in.Route,
		"authority_revision":  in.AuthorityRevision,
		"zero_data_retention": in.ZeroDataRetention,
	}
	if in.ActionID != "" {
		details["action_id"] = clampAuditString(in.ActionID)
	}
	if in.ServiceAudience != "" {
		details["service_audience"] = clampAuditString(in.ServiceAudience)
	}
	if !in.ExpiresAt.IsZero() {
		details["expires_at"] = in.ExpiresAt.UTC().Format(time.RFC3339)
	}
	at := in.OccurredAt.UTC()
	payload, err = encodeDecisionAudit(eventID, at, in.OrgID, in.SubjectID, in.ActorRole, auditEventSpaceDecisionIssued, in.SpaceRef, "ok", details)
	return eventID, AuditSubjectSpaceDecisionIssued, payload, err
}

func encodeDecisionAudit(eventID string, at time.Time, orgID, userID, actorRole, event, spaceRef, outcome string, details map[string]any) ([]byte, error) {
	body := map[string]any{
		"event_id":    eventID,
		"occurred_at": at.Format(time.RFC3339Nano),
		"org_id":      orgID,
		"plane":       "control",
		"producer":    "user-core",
		"event":       event,
		"outcome":     outcome,
		"details":     details,
	}
	if userID != "" {
		body["user_id"] = userID
	}
	if actorRole != "" {
		body["actor_role"] = clampAuditString(actorRole)
	}
	if spaceRef != "" {
		body["subject"] = "space:" + clampAuditString(spaceRef)
		body["resource_id"] = clampAuditString(spaceRef)
	}
	encoded, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("encode Space decision audit event: %w", err)
	}
	return encoded, nil
}

func clampAuditString(value string) string {
	value = strings.TrimSpace(value)
	if len(value) > maxAuditDetailString {
		return value[:maxAuditDetailString]
	}
	return value
}
