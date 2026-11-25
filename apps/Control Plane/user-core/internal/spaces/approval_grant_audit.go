package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// WS10-S2: standing-rule grant events are exported to audit-core. Each row
// written to space_approval_grant_events is paired, on the same transaction,
// with one user_audit_outbox row whose event id is space-approval-grant:<event
// id>. A rule life-cycle change and its audit intent therefore commit or roll
// back together, and a retry cannot produce a second row. The row is
// content-free: identifiers, the closed event kind and a reason code. It never
// carries a tool argument, a schema hash, a token or a nonce.

// AuditSubjectApprovalGrantEvent is the user_audit_outbox subject of one
// standing-rule grant event, inside user-core's authority prefix.
const AuditSubjectApprovalGrantEvent = "verevon.audit.v2.control.user-core.space_approval_grant_event"

const (
	auditEventApprovalGrantEvent = "space_approval_grant_event"
	// approvalGrantSystemActor marks a change no person made (an agent left the room).
	approvalGrantSystemActor = "system"
)

// ApprovalGrantEventAudit is the content-free projection of one grant event row.
type ApprovalGrantEventAudit struct {
	EventID        int64
	RuleID         string
	OrgID          string
	SpaceRef       string
	Kind           string
	ActorSubjectID string
	RunID          string
	ReasonCode     string
	OccurredAt     time.Time
}

// BuildApprovalGrantEventAudit renders the audit row of one grant event. The
// event id is derived from the row's own sequence number, so it is unique per
// event and a replayed write is a no-op.
func BuildApprovalGrantEventAudit(in ApprovalGrantEventAudit) (eventID, subject string, payload []byte, err error) {
	if in.EventID <= 0 || strings.TrimSpace(in.RuleID) == "" ||
		strings.TrimSpace(in.OrgID) == "" || strings.TrimSpace(in.SpaceRef) == "" {
		return "", "", nil, fmt.Errorf("approval grant event audit needs an event, a rule, an org and a Space")
	}
	eventID = fmt.Sprintf("space-approval-grant:%d", in.EventID)
	outcome := "ok"
	if in.Kind == approvalEventUseDenied {
		outcome = "denied"
	}
	actor := strings.TrimSpace(in.ActorSubjectID)
	details := map[string]any{
		"grant_kind": clampAuditString(in.Kind),
		"rule_id":    clampAuditString(in.RuleID),
	}
	if actor == approvalGrantSystemActor {
		details["actor"] = approvalGrantSystemActor
		actor = ""
	}
	if in.RunID != "" {
		details["run_id"] = clampAuditString(in.RunID)
	}
	if in.ReasonCode != "" {
		details["reason_code"] = clampAuditString(in.ReasonCode)
	}
	payload, err = encodeDecisionAudit(eventID, in.OccurredAt.UTC(), in.OrgID, actor, "",
		auditEventApprovalGrantEvent, in.SpaceRef, outcome, details)
	return eventID, AuditSubjectApprovalGrantEvent, payload, err
}

// enqueueApprovalGrantAudit writes the audit row of one grant event on the
// caller's transaction. A nil outbox refuses the write: a standing-rule change
// that cannot be exported to audit-core is not made.
func enqueueApprovalGrantAudit(ctx context.Context, tx pgx.Tx, outbox PolicyAuditOutbox, in ApprovalGrantEventAudit) error {
	if outbox == nil {
		return errors.New("approval grant audit outbox is not wired")
	}
	eventID, subject, payload, err := BuildApprovalGrantEventAudit(in)
	if err != nil {
		return err
	}
	if err := outbox.EnqueueAuditTx(ctx, tx, eventID, subject, payload); err != nil {
		return fmt.Errorf("export approval grant event to audit: %w", err)
	}
	return nil
}
