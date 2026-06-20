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
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
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
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
}

// HandleErasure transfers the erased user's owned documents to the org system
// account and emits the ownership-transferred event. Idempotent (a re-delivery
// after the first transfer matches zero rows). Returns the number transferred.
// Non-user subjects and events missing scope are a no-op (returns 0, nil).
func HandleErasure(ctx context.Context, repo OwnershipTransferrer, pub Publisher, payload []byte) (int64, error) {
	var evt erasureEvent
	if err := json.Unmarshal(payload, &evt); err != nil {
		return 0, fmt.Errorf("decode erasure event: %w", err)
	}
	if evt.SubjectType != "user" || evt.SubjectID == "" || evt.OrgID == "" {
		return 0, nil
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
		if mErr == nil {
			_ = pub.Publish(OwnershipTransferredSubject, body)
		}
	}
	return n, nil
}

// StartSubscriber subscribes to the GDPR erasure fan-out on the (shared) NATS
// connection and transfers ownership for each erased user. Best-effort: handler
// failures are logged, never fatal.
func StartSubscriber(nc *nats.Conn, repo OwnershipTransferrer) error {
	_, err := nc.Subscribe(ErasureRequestedSubject, func(msg *nats.Msg) {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		n, hErr := HandleErasure(ctx, repo, nc, msg.Data)
		if hErr != nil {
			log.Error().Err(hErr).Msg("gdpr erasure: ownership transfer failed")
			return
		}
		if n > 0 {
			log.Info().Int64("documents_transferred", n).Msg("gdpr erasure: transferred owned documents to system account")
		}
	})
	if err != nil {
		return fmt.Errorf("subscribe %s: %w", ErasureRequestedSubject, err)
	}
	log.Info().Str("subject", ErasureRequestedSubject).Msg("gdpr erasure ownership-transfer subscriber started")
	return nil
}
