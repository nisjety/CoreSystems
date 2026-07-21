// Package gdpr implements data-orchestrator-go's slice of the cross-plane
// GDPR erasure fan-out for the Per-Org Data Ownership & Erasure phase.
//
// org-core publishes `velion.gdpr.erasure.requested` on the shared broker
// (control-shared-nats) when an organization is hard-erased, from two
// trigger paths that share one payload shape (see apps/Control Plane/org-core
// /internal/org/gdpr_erasure_fanout.go's PublishGDPRErasureFanout):
//
//   - the explicit, immediate owner-gated hard-delete
//     (DELETE /orgs/:id/gdpr/erase)
//   - the 30-day soft-delete retention cron (PurgeDeletedOrganizations)
//
// Both publish:
//
//	{"subject_type":"organization","subject_id":"<org_id>","org_id":"<org_id>",
//	 "requested_by":"<actor_id>","ts":"<RFC3339Nano>"}
//
// The SAME subject also carries per-user erasure/anonymization fan-out
// (subject_type == "user" | "user_anonymize", where org_id is routing-only,
// not an instruction to purge the whole org). This package must gate
// strictly on subject_type == "organization" and treat every other
// subject_type as a deliberate no-op: ack, don't purge, don't error. See
// decodeOrgErasureEvent below.
//
// Scope: HandleOrgErasure hard-deletes (not soft-deletes) every row this
// service's own runtime code writes for the org — confirmed by auditing
// this service's Go source against its own migrations
// (apps/Data Plane v2/infra/postgres/migrations):
//
//   - data_orchestrator_jobs (20260711160000_quality_orchestrator_durability.sql)
//   - cost_events            (20260508140000_add_cost_events.sql,
//     20260508180000_add_user_id_to_cost_events.sql)
//
// It does NOT purge quality_eval_runs (co-located in the same durability
// migration file, but owned and queried exclusively by data-quality-go's
// internal/eval/store.go — no data-orchestrator-go code reads or writes that
// table), knowledge_units, or data_plane_audit_log: those are out of this
// consumer's authority and are each some other service's concern (or, for
// the audit log, a deliberate exception to erasure).
package gdpr

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
)

// ErasureRequestedSubject mirrors org-core's GDPRErasureRequestedSubject
// constant. It is redefined here (rather than imported across a service
// boundary Go modules don't share) exactly as every other GDPR-consuming
// service in this program does.
const ErasureRequestedSubject = "velion.gdpr.erasure.requested"

// OrgPurger is the slice of persistence HandleOrgErasure needs. *PurgeRepo
// (repo.go) satisfies this via HardPurgeByOrg.
type OrgPurger interface {
	HardPurgeByOrg(ctx context.Context, orgID string) error
}

// poisonOrgEventError marks a decode failure that can never succeed on
// redelivery (malformed JSON, or an "organization" event missing its
// required scope). consumer.go's processOrgPurgeDelivery routes these
// straight to the DLQ on the first attempt instead of retrying forever.
type poisonOrgEventError struct{ reason string }

func (e *poisonOrgEventError) Error() string { return e.reason }

// orgErasureEvent mirrors org-core's PublishGDPRErasureFanout payload.
// Decoded leniently (extra/unknown JSON fields ignored) because this
// consumer shares its subject with the per-user erasure/anonymize fan-out,
// whose payload carries fields (event_id, operation_id, mode) this struct
// doesn't know about — that is expected multiplexing, not malformed input.
type orgErasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Timestamp   string `json:"ts"`
}

// decodeOrgErasureEvent parses payload and reports whether it should be
// skipped: a well-formed event this consumer simply doesn't own (any
// subject_type other than "organization", e.g. a per-user erasure or
// anonymize fan-out sharing the same subject). A non-nil error means the
// payload was unparsable JSON, or claimed to be an organization erasure but
// was missing/oversized its scope — that is poison, not a skip, and the
// caller must route it to the DLQ rather than silently dropping it.
func decodeOrgErasureEvent(payload []byte) (evt orgErasureEvent, skip bool, err error) {
	if err := json.Unmarshal(payload, &evt); err != nil {
		return orgErasureEvent{}, false, &poisonOrgEventError{reason: "decode org erasure event: " + err.Error()}
	}
	evt.SubjectType = strings.TrimSpace(evt.SubjectType)
	if evt.SubjectType != "organization" {
		return orgErasureEvent{}, true, nil
	}
	evt.SubjectID = strings.TrimSpace(evt.SubjectID)
	evt.OrgID = strings.TrimSpace(evt.OrgID)
	if evt.OrgID == "" || len(evt.OrgID) > 255 {
		return orgErasureEvent{}, false, &poisonOrgEventError{reason: "organization erasure event requires a bounded org_id"}
	}
	if evt.SubjectID == "" || len(evt.SubjectID) > 255 {
		return orgErasureEvent{}, false, &poisonOrgEventError{reason: "organization erasure event requires a bounded subject_id"}
	}
	return evt, false, nil
}

// HandleOrgErasure hard-purges every row this service owns for the erased
// organization named in the event payload's org_id field (never subject_id,
// never anything client-derived — the safety contract requires scoping
// strictly by the payload's org_id). Idempotent: a redelivery after the
// first successful purge matches zero rows on every table and returns nil,
// matching NATS at-least-once delivery. Non-organization subjects and
// malformed scope are resolved by decodeOrgErasureEvent above.
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
