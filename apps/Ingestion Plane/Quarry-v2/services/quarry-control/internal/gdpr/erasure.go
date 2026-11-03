// Package gdpr subscribes quarry-control to the cross-plane GDPR erasure
// fan-out (verevon.gdpr.erasure.requested) and hard-purges this service's
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
//
// W2-U49 (X-19 condition c): with QUARRY_CONTROL_GDPR_USER_RECEIPTS on, a
// "user" or "user_anonymize" event is no longer a no-op: quarry-control calls
// quarry-edge DELETE /v1/internal/orgs/{org}/users/{user}/receipts, which
// deletes the step receipts and browser timeline events that user initiated in
// that org. user-core's saga publishes one such event per org the user
// belonged to (user_erasure_fanout snapshots the memberships), so no
// registration on the Control Plane side is needed. Off by default.
package gdpr

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// UserReceiptsFlagEnv switches on the user-level receipt erasure hop (W2-U49).
// Unset or anything but 1/true/on/yes leaves user events the no-op they were.
const UserReceiptsFlagEnv = "QUARRY_CONTROL_GDPR_USER_RECEIPTS"

// UserReceiptPurger deletes one user's step receipts in one org, held by
// quarry-edge. HTTPEdgePurger implements it. A nil UserReceiptPurger means the
// hop is off.
type UserReceiptPurger interface {
	PurgeEdgeUserReceipts(orgID, userID string) (EdgePurgeResult, error)
}

// ErasureRequestedSubject mirrors org-core's GDPRErasureFanoutSubject.
const ErasureRequestedSubject = "verevon.gdpr.erasure.requested"

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
	result, _, err := HandleErasureWithEdge(purger, nil, payload)
	return result, err
}

// HandleErasureWithEdge is HandleErasure plus the quarry-edge hop (WS17.E3). For
// an "organization" event it purges the control store first, then calls the edge
// (when edge is not nil). A failure in either step is returned as a plain error,
// so the subscriber NAKs and JetStream redelivers; both steps are idempotent, so
// a redelivery repeats them safely. Other events are the same deliberate no-ops
// as HandleErasure.
func HandleErasureWithEdge(purger OrgPurger, edge EdgePurger, payload []byte) (store.PurgeResult, EdgePurgeResult, error) {
	return HandleErasureWithUsers(purger, edge, nil, payload)
}

// isUserSubject is true for the two subject types user-core's saga publishes.
func isUserSubject(subjectType string) bool {
	return subjectType == "user" || subjectType == "user_anonymize"
}

// HandleErasureWithUsers is HandleErasureWithEdge plus the user-level receipt
// hop (W2-U49). A "user" or "user_anonymize" event with users not nil deletes
// that user's step receipts in the event's org through quarry-edge; both ids
// are required, and an event missing either is a deliberate no-op (acked), like
// an org event missing its org. An anonymize erasure deletes the receipts too:
// a receipt holds what the user typed, which cannot be anonymised in place. A
// hop failure is a plain error (NAK and redelivery); the delete is idempotent.
// With users nil, user events stay the no-op they were.
func HandleErasureWithUsers(purger OrgPurger, edge EdgePurger, users UserReceiptPurger, payload []byte) (store.PurgeResult, EdgePurgeResult, error) {
	evt, err := decodeErasureEvent(payload)
	if err != nil {
		return store.PurgeResult{}, EdgePurgeResult{}, err
	}
	if isUserSubject(evt.SubjectType) {
		if users == nil || evt.OrgID == "" || evt.SubjectID == "" {
			return store.PurgeResult{}, EdgePurgeResult{}, nil
		}
		edgeResult, err := users.PurgeEdgeUserReceipts(evt.OrgID, evt.SubjectID)
		if err != nil {
			return store.PurgeResult{}, EdgePurgeResult{}, err
		}
		return store.PurgeResult{}, edgeResult, nil
	}
	if evt.SubjectType != "organization" {
		return store.PurgeResult{}, EdgePurgeResult{}, nil
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
		return store.PurgeResult{}, EdgePurgeResult{}, nil
	}
	result, err := purger.PurgeOrg(orgID)
	if err != nil {
		return result, EdgePurgeResult{}, err
	}
	if edge == nil {
		return result, EdgePurgeResult{}, nil
	}
	edgeResult, err := edge.PurgeEdgeOrg(orgID)
	if err != nil {
		return result, EdgePurgeResult{}, err
	}
	return result, edgeResult, nil
}

// PurgeEdgeUserReceipts deletes one user's step receipts in one org in
// quarry-edge (W2-U49). Idempotent. Both ids are escaped as single path
// segments.
func (p *HTTPEdgePurger) PurgeEdgeUserReceipts(orgID, userID string) (EdgePurgeResult, error) {
	endpoint := p.baseURL + "/v1/internal/orgs/" + url.PathEscape(orgID) +
		"/users/" + url.PathEscape(userID) + "/receipts"
	return p.deleteForCounts(endpoint)
}

// deleteForCounts sends one DELETE with the internal token and reads the
// counts-only envelope. Errors carry fixed messages only: the endpoint holds
// the org and user ids.
func (p *HTTPEdgePurger) deleteForCounts(endpoint string) (EdgePurgeResult, error) {
	ctx, cancel := context.WithTimeout(context.Background(), EdgePurgeTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, endpoint, nil)
	if err != nil {
		return EdgePurgeResult{}, errEdgeRequest
	}
	req.Header.Set("Authorization", "Bearer "+p.token)
	req.Header.Set("Accept", "application/json")
	resp, err := p.client.Do(req)
	if err != nil {
		return EdgePurgeResult{}, errEdgeUnreachable
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxEdgeResponseBytes))
	if err != nil {
		return EdgePurgeResult{}, errEdgeResponse
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return EdgePurgeResult{}, fmt.Errorf("gdpr: edge purge refused with status %d", resp.StatusCode)
	}
	var envelope struct {
		Data struct {
			Families []EdgePurgeFamily `json:"families"`
		} `json:"data"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		return EdgePurgeResult{}, errEdgeResponse
	}
	return EdgePurgeResult{Families: envelope.Data.Families}, nil
}

// PurgeEdgeUserReceipts makes a misconfigured hop refuse user erasures too.
func (f failingEdgePurger) PurgeEdgeUserReceipts(string, string) (EdgePurgeResult, error) {
	return EdgePurgeResult{}, f.err
}

// flagOn is true for the usual spellings of an enabled boolean flag.
func flagOn(raw string) bool {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "1", "true", "on", "yes":
		return true
	}
	return false
}

// UserReceiptPurgerFromEnv builds the user-level hop (W2-U49). The flag off
// returns nil (user events stay no-ops). The flag on with the edge hop off or
// unable to erase users returns a purger that fails every call, so user
// erasures are retried and the gap stays visible until the config is fixed.
func UserReceiptPurgerFromEnv(getenv func(string) string, edge EdgePurger, logger zerolog.Logger) UserReceiptPurger {
	if !flagOn(getenv(UserReceiptsFlagEnv)) {
		return nil
	}
	if edge == nil {
		logger.Error().Msg("gdpr: " + UserReceiptsFlagEnv + " is on but QUARRY_EDGE_URL is unset; user erasures will be retried until fixed")
		return failingEdgePurger{err: errors.New("gdpr: user receipt erasure is not configured (edge)")}
	}
	users, ok := edge.(UserReceiptPurger)
	if !ok {
		logger.Error().Msg("gdpr: the edge hop cannot erase user receipts; user erasures will be retried until fixed")
		return failingEdgePurger{err: errors.New("gdpr: user receipt erasure is not configured (hop)")}
	}
	logger.Info().Msg("gdpr: user-level receipt erasure hop enabled")
	return users
}
