package nats

import (
	"context"
	"errors"
	"fmt"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/nats-io/nats.go/jetstream"
)

// X-9: the ZDR-enabled event on a plan change.
//
// An org's effective Zero Data Retention is its stored intent AND its plan
// entitlement (orgcore.RetentionPosture.Effective). The explicit toggle already
// emits controlplane.org.interactive_retention.enabled through the
// interactive-retention outbox (SetInteractiveRetention, false-to-true intent).
// A plan change never did: an org whose intent stayed true through a
// downgrade got effective ZDR back silently when its plan became entitled
// again, and every consumer of the event (Conversation Core, quarry-control)
// missed that transition. The helpers below close that gap on the existing
// publisher, with the same subject and the same {org_id, zdr} payload (plus a
// "cause" field the existing consumers ignore), so no consumer changes.
//
// Two paths change a plan:
//   - the admin path (UpdatePlanWithOutbox, durable outbox): PublishPlanChanged
//     emits the signal after the plan event, from the outbox flush, so a failed
//     publish is retried with the plan row;
//   - the billing path (UpdatePlanFromBillingSync, BillingSyncSubscriber, core
//     NATS at-most-once): the subscriber compares the posture before and after
//     the sync and emits on a false-to-true transition. It has the same
//     delivery tier as that subscriber; see the depth note in the X-9 status.
//
// Both are inert until wired: SetRetentionPostureReader on the SharedPublisher
// and SetZDRSignal on the BillingSyncSubscriber (org-core cmd/server/main.go).

// SubjectInteractiveRetentionEnabled is the durable cross-plane ZDR-enabled
// subject on the shared CONTROLPLANE stream (controlplane.> namespace).
const SubjectInteractiveRetentionEnabled = "controlplane.org.interactive_retention.enabled"

// ZDR-enabled causes carried in the payload's "cause" field.
const (
	ZDRCausePlanChange  = "plan_change"
	ZDRCauseBillingSync = "billing_sync"
)

// RetentionPostureReader reads an org's current retention posture. It is
// satisfied by *orgcore.Service (GetRetentionPosture reads the database
// directly, uncached).
type RetentionPostureReader interface {
	GetRetentionPosture(ctx context.Context, id string) (*orgcore.RetentionPosture, error)
}

// SetRetentionPostureReader enables the plan-change ZDR signal on this
// publisher. Call once at startup, before the plan-change outbox worker runs.
func (sp *SharedPublisher) SetRetentionPostureReader(reader RetentionPostureReader) {
	if sp == nil {
		return
	}
	sp.postureReader = reader
}

// PublishZDREnabledForBillingSync emits the ZDR-enabled event for a billing
// sync that turned effective ZDR on. The billing path has no revision in hand,
// so the message id carries the plan and the emit time; consumers are
// idempotent per org, so a second emit is harmless.
func (sp *SharedPublisher) PublishZDREnabledForBillingSync(ctx context.Context, orgID, plan string) error {
	return sp.publishWithMsgID(ctx, SubjectInteractiveRetentionEnabled,
		billingSyncZDRMessageID(orgID, plan, time.Now().UTC()),
		zdrEnabledPayload(orgID, ZDRCauseBillingSync))
}

func (sp *SharedPublisher) publishWithMsgID(ctx context.Context, subject, msgID string, payload map[string]any) error {
	return sp.Publish(ctx, subject, payload, jetstream.WithMsgID(msgID))
}

// zdrPublishFunc publishes one JetStream message with a Nats-Msg-Id.
type zdrPublishFunc func(ctx context.Context, subject, msgID string, payload map[string]any) error

// zdrPlanTransitionPossible reports whether a plan change can turn effective
// ZDR on: the new plan is entitled and the previous one was not. Any other
// change leaves effective ZDR where it was (an entitled-to-entitled change
// keeps it, and a change to an unentitled plan can only turn it off).
func zdrPlanTransitionPossible(previousPlan, newPlan string) bool {
	return orgcore.PlanAllowsZeroDataRetention(newPlan) && !orgcore.PlanAllowsZeroDataRetention(previousPlan)
}

// emitZDREnabledOnPlanChange emits the ZDR-enabled event when the plan change
// previousPlan -> newPlan (revision) turned the org's effective ZDR on. It
// reads the posture at publish time: an intent cleared or a plan changed again
// in between is seen as it is now, so the event never claims ZDR that is not
// in effect. It returns whether it published. A deleted org is skipped (there
// is nothing to protect and its erasure has its own fan-out); any other read
// or publish error is returned so the caller retries.
func emitZDREnabledOnPlanChange(
	ctx context.Context,
	reader RetentionPostureReader,
	publish zdrPublishFunc,
	orgID, previousPlan, newPlan string,
	revision int64,
) (bool, error) {
	if reader == nil || publish == nil || orgID == "" {
		return false, nil
	}
	if !zdrPlanTransitionPossible(previousPlan, newPlan) {
		return false, nil
	}
	posture, err := reader.GetRetentionPosture(ctx, orgID)
	if err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			return false, nil
		}
		return false, fmt.Errorf("read retention posture: %w", err)
	}
	if posture == nil || !posture.Effective() {
		return false, nil
	}
	if err := publish(ctx, SubjectInteractiveRetentionEnabled,
		planChangeZDRMessageID(orgID, revision),
		zdrEnabledPayload(orgID, ZDRCausePlanChange)); err != nil {
		return false, err
	}
	return true, nil
}

// zdrTurnedOn reports a false-to-true transition of effective ZDR.
func zdrTurnedOn(before, after orgcore.RetentionPosture) bool {
	return !before.Effective() && after.Effective()
}

// zdrEnabledPayload is the content-free payload of the ZDR-enabled event: the
// org id, zdr=true (consumers treat absent or false as a no-op) and the cause.
// No actor, plan or customer content.
func zdrEnabledPayload(orgID, cause string) map[string]any {
	return map[string]any{
		"org_id": orgID,
		"zdr":    true,
		"cause":  cause,
	}
}

// planChangeZDRMessageID is unique per plan transition and stable across
// retries of the same outbox row. It shares the "interactive-retention:"
// prefix with the toggle's ids (interactive-retention:<outbox event id>, all
// digits) and cannot collide with them.
func planChangeZDRMessageID(orgID string, revision int64) string {
	return fmt.Sprintf("interactive-retention:plan:%s:%d", orgID, revision)
}

func billingSyncZDRMessageID(orgID, plan string, at time.Time) string {
	return fmt.Sprintf("interactive-retention:billing-sync:%s:%s:%d", orgID, plan, at.UnixNano())
}
