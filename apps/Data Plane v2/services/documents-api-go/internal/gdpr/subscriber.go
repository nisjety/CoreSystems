// Package gdpr implements the Data-Plane half of the cross-plane GDPR erasure
// fan-out for the Per-User Data Ownership & Sharing phase.
//
// user-core publishes `velion.gdpr.erasure.requested` on the shared bus when a
// user is erased. This subscriber transfers that user's OWNED documents to the
// org system account (so no orphaned owner_id remains and no human silently
// inherits the erased user's private documents) and emits
// `velion.gdpr.ownership.transferred`. It is idempotent and best-effort.
//
// Scope split (per the phase plan): this phase owns ownership TRANSFER +
// per-user grant revocation (user-core revokes the grants); Phase 2 owns the
// content-byte purge. This subscriber therefore only re-owns rows — it does not
// delete document content.
package gdpr

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"time"
)

const (
	// ErasureRequestedSubject mirrors user-core's GDPRErasureFanoutSubject.
	ErasureRequestedSubject = "velion.gdpr.erasure.requested"
	// OwnershipTransferredSubject is emitted after a successful transfer.
	OwnershipTransferredSubject = "velion.gdpr.ownership.transferred"
	// systemAccount is the sentinel owner erased users' documents move to. It
	// matches documents.owner_id's column default; org-visible docs keep their
	// visibility, private docs become system-owned (visible to no human until an
	// admin re-shares them).
	systemAccount = "org-system-account"
)

// OwnershipTransferrer is the slice of the document repository the subscriber
// needs (an interface so the handler is unit-testable without a database).
type OwnershipTransferrer interface {
	TransferOwnership(ctx context.Context, orgID, fromOwner, toOwner string) (int64, error)
}

// Publisher emits the ownership-transferred event. *nats.Conn satisfies it.
type Publisher interface {
	Publish(subject string, data []byte) error
}

type erasureEvent struct {
	EventID     string `json:"event_id"`
	OperationID string `json:"operation_id"`
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Mode        string `json:"mode"`
	Timestamp   string `json:"ts"`
}

type poisonEventError struct{ reason string }

func (e *poisonEventError) Error() string { return e.reason }

func decodeErasureEvent(payload []byte) (erasureEvent, error) {
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	var event erasureEvent
	if err := decoder.Decode(&event); err != nil {
		return erasureEvent{}, &poisonEventError{reason: "decode erasure event: " + err.Error()}
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return erasureEvent{}, &poisonEventError{reason: "erasure event must contain one JSON object"}
	}
	event.EventID = strings.TrimSpace(event.EventID)
	event.SubjectType = strings.TrimSpace(event.SubjectType)
	event.SubjectID = strings.TrimSpace(event.SubjectID)
	event.OrgID = strings.TrimSpace(event.OrgID)
	if event.EventID == "" || len(event.EventID) > 128 || event.SubjectID == "" || len(event.SubjectID) > 255 || event.OrgID == "" || len(event.OrgID) > 255 {
		return erasureEvent{}, &poisonEventError{reason: "erasure event requires bounded event, subject, and organization IDs"}
	}
	if event.SubjectType != "user" && event.SubjectType != "user_anonymize" {
		return erasureEvent{}, &poisonEventError{reason: "erasure subject type is outside documents-api authority"}
	}
	return event, nil
}

// HandleErasure transfers the erased user's owned documents to the org system
// account and emits the ownership-transferred event. Idempotent (a re-delivery
// after the first transfer matches zero rows). Returns the number transferred.
// Non-user subjects and events missing scope are a no-op (returns 0, nil).
func HandleErasure(ctx context.Context, repo OwnershipTransferrer, pub Publisher, payload []byte) (int64, error) {
	evt, err := decodeErasureEvent(payload)
	if err != nil {
		return 0, err
	}

	n, err := repo.TransferOwnership(ctx, evt.OrgID, evt.SubjectID, systemAccount)
	if err != nil {
		return 0, err
	}

	if pub != nil {
		body, mErr := json.Marshal(map[string]any{
			"org_id":                evt.OrgID,
			"from_user":             evt.SubjectID,
			"to_owner":              systemAccount,
			"documents_transferred": n,
			"ts":                    time.Now().UTC().Format(time.RFC3339Nano),
		})
		if mErr != nil {
			return 0, fmt.Errorf("encode ownership transfer event: %w", mErr)
		}
		if err := pub.Publish(OwnershipTransferredSubject, body); err != nil {
			return 0, fmt.Errorf("publish ownership transfer event: %w", err)
		}
	}
	return n, nil
}
