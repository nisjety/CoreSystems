// org_purge.go implements the Data-Plane half of ORGANIZATION hard-erasure.
//
// org-core publishes ErasureRequestedSubject ("verevon.gdpr.erasure.requested")
// — the SAME subject subscriber.go's per-user ownership-transfer consumer
// reads — when an organization is erased, from two trigger paths that share
// one payload shape (apps/Control Plane/org-core/internal/org/
// gdpr_erasure_fanout.go's PublishGDPRErasureFanout):
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
// This payload carries no event_id/operation_id — PublishGDPRErasureFanout
// never sets one. Reusing subscriber.go's decodeErasureEvent (which requires
// a bounded event_id) would poison every organization erasure on delivery, so
// this file decodes independently and only acts on subject_type ==
// "organization". Every other subject_type (user, user_anonymize, …) is a
// deliberate no-op here: it's subscriber.go's concern, multiplexed onto the
// same subject.
//
// Scope: HandleOrgErasure hard-deletes (not soft-deletes) every row this
// service owns for the org via repo.HardPurgeByOrg — documents, the
// documents outbox, source objects, and the per-org cache-version counter.
// It does NOT purge chunks, embeddings, graph data, wiki data, or retrieval
// traces for the org: those tables live in sibling Data Plane v2 services
// with their own database access code that documents-api-go cannot reach:
//
//   - embedding-engine-rs / index-engine-rs — knowledge_units (chunks/embeddings)
//   - graph-index-rs                        — graph_exports, knowledge_units graph edges
//   - wiki-store-go                         — wiki_pages, wiki_source_logs,
//     wiki_maintenance_logs, operating_map*
//   - retrieval-engine-rs                   — retrieval_runs, access_audit_log,
//     admin_audit_log, agent_retrieval_configs, context_pins
//   - data-quality-go / data-orchestrator-go — quality_eval_runs,
//     data_orchestrator_jobs, eval_golden_judgments, cost_events
//   - quickwit-adapter-rs                   — quickwit_admin_jobs*
//
// FOLLOW-UP (out of this service's reach, not silently dropped): each of
// those services needs its own consumer on ErasureRequestedSubject (or an
// equivalent purge path) to fully close org erasure for Data Plane v2. This
// package only closes the documents-api-go slice of that work.
package gdpr

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
)

// OrgPurger is the slice of the document repository HandleOrgErasure needs.
// *repo.DocumentRepo satisfies this via HardPurgeByOrg.
type OrgPurger interface {
	HardPurgeByOrg(ctx context.Context, orgID string) error
}

// poisonOrgEventError marks a decode failure that can never succeed on
// redelivery (malformed JSON, or an "organization" event missing its
// required scope). org_purge_consumer.go's processOrgPurgeDelivery routes
// these straight to the DLQ on the first attempt instead of retrying —
// mirrors subscriber.go's poisonEventError for the per-user erasure path.
type poisonOrgEventError struct{ reason string }

func (e *poisonOrgEventError) Error() string { return e.reason }

// orgErasureEvent mirrors org-core's PublishGDPRErasureFanout payload.
// Decoded leniently (extra/unknown JSON fields are ignored) because this
// consumer shares its subject with subscriber.go's per-user erasure event,
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
// subject_type other than "organization", e.g. a per-user erasure fan-out).
// A non-nil error means the payload was unparsable JSON, or claimed to be an
// organization erasure but was missing/oversized its scope — that is poison,
// not a skip, and the caller must route it to the DLQ rather than silently
// dropping it.
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
