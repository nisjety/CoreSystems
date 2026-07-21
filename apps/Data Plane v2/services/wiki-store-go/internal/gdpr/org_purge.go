// Package gdpr subscribes wiki-store-go to the cross-plane GDPR erasure
// fan-out (velion.gdpr.erasure.requested) and hard-purges this service's
// org-scoped wiki and Operating Map data when an organization is erased.
//
// Producers: org-core publishes this exact subject + payload shape on BOTH
// the explicit, immediate hard-delete path (DELETE /orgs/:id/gdpr/erase) and
// the 30-day soft-delete retention cron (PurgeDeletedOrganizations) — see
// apps/Control Plane/org-core/internal/org/gdpr_erasure_fanout.go's
// PublishGDPRErasureFanout, the single implementation shared by both trigger
// paths so subscribers never special-case which one fired. Both publish:
//
//	{"subject_type":"organization","subject_id":"<org_id>","org_id":"<org_id>",
//	 "requested_by":"<actor_id>","ts":"<RFC3339Nano>"}
//
// user-core ALSO publishes this same subject for "user"/"user_anonymize"
// erasure, with a richer payload (event_id, operation_id, mode, ...) this
// package doesn't know about. That is expected multiplexing onto one
// subject, not malformed input: decodeOrgErasureEvent below decodes
// leniently and treats every subject_type other than "organization" as a
// deliberate no-op — never an error, never a purge.
//
// Scope: see internal/repo/org_purge.go's package doc for the exact table
// list HardPurgeByOrg purges (directly and transitively via FK cascade) and
// the sibling Data Plane v2 services that own the tables this consumer does
// NOT reach (documents, chunks, embeddings, graph, retrieval traces).
package gdpr

import (
	"context"
	"encoding/json"
	"strings"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/repo"
)

// ErasureRequestedSubject mirrors org-core's GDPRErasureFanoutSubject /
// user-core's GDPRErasureFanoutSubject — the single cross-plane subject both
// org- and user-scoped erasure fan-out share.
const ErasureRequestedSubject = "velion.gdpr.erasure.requested"

// OrgPurger is the repository slice this consumer needs. *repo.WikiRepo
// satisfies it via HardPurgeByOrg (internal/repo/org_purge.go).
type OrgPurger interface {
	HardPurgeByOrg(ctx context.Context, orgID string) (repo.PurgeResult, error)
}

// poisonEventError marks an erasure delivery as fundamentally
// unprocessable — no amount of NATS redelivery turns a malformed payload
// into a valid one. The subscriber (subscriber.go) acks poison deliveries
// immediately instead of nak-ing them for retry, mirroring the sibling
// org-purge consumers' poison handling (documents-api-go's
// poisonOrgEventError, conversation-core-go's / quarry-control's
// immediate-ack-on-decode-error).
type poisonEventError struct{ reason string }

func (e *poisonEventError) Error() string { return e.reason }

// orgErasureEvent is the wire shape org-core (and, for its own
// subject_type values, user-core) publish on ErasureRequestedSubject.
// Decoded leniently — extra/unknown JSON fields are ignored — because this
// consumer shares its subject with the per-user erasure fan-out, whose
// payload carries fields this struct doesn't know about. That is expected
// multiplexing, not malformed input.
type orgErasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Timestamp   string `json:"ts"`
}

// decodeOrgErasureEvent parses payload and reports whether it should be
// skipped: a well-formed event this consumer simply doesn't own (any
// subject_type other than "organization", e.g. a per-user erasure
// fan-out). A non-nil error means the payload was unparsable JSON, or
// claimed to be an organization erasure but was missing/oversized its
// scope — that is poison, not a skip, and the caller must ack it (never
// retry) rather than silently dropping it forever via infinite redelivery.
func decodeOrgErasureEvent(payload []byte) (evt orgErasureEvent, skip bool, err error) {
	if err := json.Unmarshal(payload, &evt); err != nil {
		return orgErasureEvent{}, false, &poisonEventError{reason: "decode org erasure event: " + err.Error()}
	}
	evt.SubjectType = strings.TrimSpace(evt.SubjectType)
	if evt.SubjectType != "organization" {
		return orgErasureEvent{}, true, nil
	}
	evt.SubjectID = strings.TrimSpace(evt.SubjectID)
	evt.OrgID = strings.TrimSpace(evt.OrgID)
	// org_id and subject_id are always the same value in org-core's actual
	// publisher (PublishGDPRErasureFanout stamps both from orgID), but fall
	// back to subject_id defensively — mirrors conversation-core-go's and
	// quarry-control's org-erasure consumers' same fallback for this event.
	if evt.OrgID == "" {
		evt.OrgID = evt.SubjectID
	}
	if evt.OrgID == "" || len(evt.OrgID) > 255 {
		return orgErasureEvent{}, false, &poisonEventError{reason: "organization erasure event requires a bounded org_id"}
	}
	return evt, false, nil
}

// HandleOrgErasure hard-purges every row this service owns for the erased
// organization named in the event payload's org_id field (never subject_id
// alone, never anything client-derived — the safety contract requires
// scoping strictly by the payload's resolved org_id). Idempotent:
// redelivery after the first successful purge matches zero rows on every
// table and returns a zero-value PurgeResult, nil error, matching NATS
// at-least-once delivery. Non-organization subjects and malformed scope are
// resolved by decodeOrgErasureEvent above and returned as a no-op, not an
// error.
func HandleOrgErasure(ctx context.Context, purger OrgPurger, payload []byte) (repo.PurgeResult, error) {
	evt, skip, err := decodeOrgErasureEvent(payload)
	if err != nil {
		return repo.PurgeResult{}, err
	}
	if skip {
		return repo.PurgeResult{}, nil
	}
	return purger.HardPurgeByOrg(ctx, evt.OrgID)
}
