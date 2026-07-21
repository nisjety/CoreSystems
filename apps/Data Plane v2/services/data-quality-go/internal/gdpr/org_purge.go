// Package gdpr implements data-quality-go's slice of the cross-plane GDPR
// organization-erasure fan-out.
//
// org-core publishes ErasureRequestedSubject ("velion.gdpr.erasure.requested")
// on the shared Control-Plane bus (control-shared-nats, stream
// AQENCIA_CONTROLPLANE) from two trigger paths that share one payload shape
// (apps/Control Plane/org-core/internal/org/gdpr_erasure_fanout.go's
// PublishGDPRErasureFanout):
//
//   - the explicit, owner-gated hard-delete (DELETE /orgs/:id/gdpr/erase)
//   - the 30-day soft-delete retention cron (PurgeDeletedOrganizations)
//
// Both publish:
//
//	{"subject_type":"organization"|"user"|"user_anonymize","subject_id":"<id>",
//	 "org_id":"<org_id>","requested_by":"<actor_id>","ts":"<RFC3339Nano>"}
//
// The SAME subject also carries subject_type "user" and "user_anonymize" for
// the Per-User Data Ownership erasure fan-out (routing-only org_id in that
// case — must NOT trigger an org-wide purge). This package only acts on
// subject_type == "organization"; every other value is a deliberate no-op
// (see decodeOrgErasureEvent).
//
// Scope: HandleOrgErasure hard-deletes (not soft-deletes) every row this
// service owns for the org: quality_eval_runs and eval_golden_judgments (both
// org_id-scoped per infra/postgres/migrations/
// 20260711160000_quality_orchestrator_durability.sql and
// 20260717100000_eval_golden_judgments.sql). It does NOT purge
// data_orchestrator_jobs — that table is owned by the sibling
// data-orchestrator-go service and is out of reach from here; see
// documents-api-go's internal/gdpr/org_purge.go package doc for the full
// cross-service table inventory and follow-up.
package gdpr

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
)

// ErasureRequestedSubject mirrors org-core's GDPRErasureFanoutSubject /
// audit-core provisioner's GDPRErasureRequestedSubject constant.
const ErasureRequestedSubject = "velion.gdpr.erasure.requested"

// OrgPurger is the narrow surface HandleOrgErasure needs: hard-delete every
// org-scoped quality-evaluation row for one org. *PostgresOrgPurger satisfies
// this (see postgres_purger.go).
type OrgPurger interface {
	HardPurgeByOrg(ctx context.Context, orgID string) error
}

// poisonOrgEventError marks a decode failure that can never succeed on
// redelivery (malformed JSON, or an "organization" event missing its
// required scope). The consumer routes these straight to an ack (no retry)
// instead of NAK'ing, since retrying can never fix bad input.
type poisonOrgEventError struct{ reason string }

func (e *poisonOrgEventError) Error() string { return e.reason }

// orgErasureEvent mirrors org-core's PublishGDPRErasureFanout payload.
// Decoded leniently (extra/unknown JSON fields ignored) because this event
// is multiplexed with the Per-User Data Ownership erasure fan-out, whose
// payload may carry additional fields (event_id, operation_id, mode) this
// struct doesn't know about — that is expected multiplexing, not malformed
// input.
type orgErasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Timestamp   string `json:"ts"`
}

// decodeOrgErasureEvent parses payload and reports whether it should be
// skipped: a well-formed event this consumer simply doesn't own (any
// subject_type other than "organization", e.g. a per-user erasure fan-out —
// CRITICAL: this gate is what keeps a per-user event from ever triggering an
// org-wide purge). A non-nil error means the payload was unparsable JSON, or
// claimed to be an organization erasure but was missing bounded scope — that
// is poison, and the caller must ack (not retry) rather than looping forever.
func decodeOrgErasureEvent(payload []byte) (evt orgErasureEvent, skip bool, err error) {
	if err := json.Unmarshal(payload, &evt); err != nil {
		return orgErasureEvent{}, false, &poisonOrgEventError{reason: "decode org erasure event: " + err.Error()}
	}
	evt.SubjectType = strings.TrimSpace(evt.SubjectType)
	if evt.SubjectType != "organization" {
		return orgErasureEvent{}, true, nil
	}
	evt.OrgID = strings.TrimSpace(evt.OrgID)
	evt.SubjectID = strings.TrimSpace(evt.SubjectID)
	orgID := evt.OrgID
	if orgID == "" {
		orgID = evt.SubjectID
	}
	if orgID == "" || len(orgID) > 255 {
		return orgErasureEvent{}, false, &poisonOrgEventError{reason: "organization erasure event requires a bounded org_id"}
	}
	evt.OrgID = orgID
	return evt, false, nil
}

// HandleOrgErasure hard-purges every row this service owns for the erased
// organization named in the event payload's org_id (falling back to
// subject_id only when org_id is absent — never anything else client-derived
// or inferred). Idempotent: a redelivery after the first successful purge
// matches zero rows and returns nil, matching NATS at-least-once delivery.
// Non-organization subjects and malformed scope are resolved by
// decodeOrgErasureEvent above and return (nil, nil) or a *poisonOrgEventError
// respectively — never trigger a purge.
func HandleOrgErasure(ctx context.Context, repo OrgPurger, payload []byte) error {
	evt, skip, err := decodeOrgErasureEvent(payload)
	if err != nil {
		return err
	}
	if skip {
		return nil
	}
	if err := repo.HardPurgeByOrg(ctx, evt.OrgID); err != nil {
		return fmt.Errorf("hard purge org %s: %w", evt.OrgID, err)
	}
	return nil
}
