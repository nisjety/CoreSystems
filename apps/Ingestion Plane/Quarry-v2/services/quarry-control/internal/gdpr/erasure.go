// Package gdpr subscribes quarry-control to the cross-plane GDPR erasure
// fan-out (velion.gdpr.erasure.requested) and hard-purges this service's
// org-scoped crawl data when an organization is erased.
//
// Producers: org-core publishes this exact subject + payload shape on BOTH
// the explicit, immediate hard-delete path (DELETE /orgs/:id/gdpr/soft-delete's
// eventual hard-delete) and the 30-day retention cron
// (PurgeDeletedOrganizations) — see
// apps/Control Plane/org-core/internal/org/gdpr_erasure_fanout.go, which is
// the single implementation shared by both trigger paths so subscribers
// never special-case which one fired. user-core ALSO publishes this same
// subject for "user"/"user_anonymize" erasure with a different, richer
// payload shape; this package ignores those (see decodeErasureEvent) since
// a per-user erasure is not this service's org-scoped purge's concern.
package gdpr

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// ErasureRequestedSubject mirrors org-core's GDPRErasureFanoutSubject.
const ErasureRequestedSubject = "velion.gdpr.erasure.requested"

// OrgPurger is the store slice this consumer needs. store.DB satisfies it
// via PurgeOrg (internal/store/gdpr_purge.go). Kept as its own narrow
// interface — rather than this package depending on store.DB wholesale —
// so HandleErasure is unit-testable against a fake with no real store or
// NATS wiring, mirroring how documents-api-go's gdpr package tests
// HandleErasure against a fake OwnershipTransferrer.
type OrgPurger interface {
	PurgeOrg(orgID string) (store.PurgeResult, error)
}

// erasureEvent is the wire shape org-core (and, for its own subject_type
// values, user-core) publish on ErasureRequestedSubject. Fields beyond what
// this package reads are ignored rather than rejected: an
// unknown-fields-strict decode would break the moment user-core's richer
// "user" payload (event_id, operation_id, mode, ...) arrives on the same
// subject, and this consumer has no business rejecting a message it simply
// doesn't act on.
type erasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Timestamp   string `json:"ts"`
}

func decodeErasureEvent(payload []byte) (erasureEvent, error) {
	var evt erasureEvent
	if err := json.Unmarshal(payload, &evt); err != nil {
		return erasureEvent{}, &poisonEventError{reason: fmt.Sprintf("gdpr: decode erasure event: %v", err)}
	}
	evt.SubjectType = strings.TrimSpace(evt.SubjectType)
	evt.SubjectID = strings.TrimSpace(evt.SubjectID)
	evt.OrgID = strings.TrimSpace(evt.OrgID)
	evt.RequestedBy = strings.TrimSpace(evt.RequestedBy)
	return evt, nil
}

// poisonEventError marks an erasure delivery as fundamentally
// unprocessable — no amount of NATS redelivery turns a malformed payload
// into a valid one. The subscriber (subscriber.go) acks poison deliveries
// immediately instead of nak-ing them for retry, mirroring the sibling
// org-purge consumers' poison handling (documents-api-go's
// poisonEventError, conversation-core-go's immediate-ack-on-decode-error).
type poisonEventError struct{ reason string }

func (e *poisonEventError) Error() string { return e.reason }

// HandleErasure purges quarry-control's org-scoped crawl data for one
// "organization" erasure event. Anything else — a malformed payload aside —
// is a deliberate no-op (zero-value PurgeResult, nil error), never an
// error: a "user"/"user_anonymize" subject_type, an event missing org_id,
// or a redelivered/duplicate event for an org already purged must not make
// the caller treat the message as failed, which would either retry a
// message that will never become actionable or mask a genuine failure
// behind noise. Idempotent regardless: purger.PurgeOrg's underlying
// `DELETE ... WHERE org_id = $1` statements match zero rows on a second
// delivery of the same event.
func HandleErasure(purger OrgPurger, payload []byte) (store.PurgeResult, error) {
	evt, err := decodeErasureEvent(payload)
	if err != nil {
		return store.PurgeResult{}, err
	}
	if evt.SubjectType != "organization" {
		return store.PurgeResult{}, nil
	}
	// org_id and subject_id are always the same value in org-core's actual
	// publisher (PublishGDPRErasureFanout stamps both from orgID), but fall
	// back to subject_id defensively — mirrors conversation-core-go's
	// org-erasure consumer's same fallback for this exact event.
	orgID := evt.OrgID
	if orgID == "" {
		orgID = evt.SubjectID
	}
	if orgID == "" {
		return store.PurgeResult{}, nil
	}
	return purger.PurgeOrg(orgID)
}
