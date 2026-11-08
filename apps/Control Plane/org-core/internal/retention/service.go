package retention

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"log"
	"strings"
)

// ErrOrganizationNotFound is returned for an organisation that does not exist (a
// write) or is not live (the check route). Reads of a live organisation that never
// wrote a policy return revision 0 and keep.
var ErrOrganizationNotFound = errors.New("organization not found")

// ErrHoldNotFound means the organisation has no hold with that id.
var ErrHoldNotFound = errors.New("legal hold not found")

// ErrHoldAlreadyReleased means the hold was released before; a release is never
// repeated, so the first releaser stays the one on record.
var ErrHoldAlreadyReleased = errors.New("legal hold already released")

// ErrNothingRetained refuses a thread hold on a thread whose content was never
// retained (CodeNothingRetained).
var ErrNothingRetained = errors.New("nothing is retained for this thread")

// RevisionConflictError means the write named a revision that is no longer the
// current one. Current is the revision the caller should re-read and retry from.
type RevisionConflictError struct {
	Current int64
}

func (e *RevisionConflictError) Error() string {
	return fmt.Sprintf("retention policy revision conflict: current revision is %d", e.Current)
}

// Actor is who made a write, as the audit row records it. Role is the caller's
// role from org-core's membership table, never a forwarded header.
type Actor struct {
	ID   string
	Role string
}

// AuditRow is one claimed outbox row.
type AuditRow struct {
	EventID string
	Subject string
	Payload map[string]any
}

// FlushResult counts one publisher pass.
type FlushResult struct {
	Published    int
	DeadLettered int
}

// Store is the persistence the service needs. *Repository satisfies it; the HTTP
// tests use an in-memory store so the real service and routes run without a
// database.
type Store interface {
	GetPolicy(ctx context.Context, orgID string) (*Policy, error)
	PutPolicy(ctx context.Context, orgID string, actor Actor, expectedRevision int64, threadDays *int) (*Policy, error)
	ListHolds(ctx context.Context, orgID string) ([]Hold, error)
	PlaceHold(ctx context.Context, orgID string, actor Actor, hold Hold) (PlaceResult, error)
	ReleaseHold(ctx context.Context, orgID, holdID string, actor Actor) (Hold, error)
	Check(ctx context.Context, orgID string, threadIDs []string) (CheckResult, error)
	ClaimAudit(ctx context.Context, limit int) ([]AuditRow, error)
	MarkAuditPublished(ctx context.Context, eventID string) error
	MarkAuditFailed(ctx context.Context, eventID string, publishErr error) (deadLettered bool, err error)
}

// ThreadRetention answers whether session-core retained any content for a thread.
// NothingRetained is true only for a thread whose retention receipt says Zero
// Data Retention or a waived run (DEC-32). session-core owns that receipt; org-core
// never reads its database.
type ThreadRetention interface {
	NothingRetained(ctx context.Context, orgID, threadID string) (bool, error)
}

// AuditPublisher is satisfied by org-core's NATS client (PublishAudit), the same
// seam as org.AuditPublisher; declared here so this package does not import org.
type AuditPublisher interface {
	PublishAudit(ctx context.Context, subject, eventID string, payload map[string]any) error
}

// Service is the one place the retention policy and holds are validated, written
// and answered.
type Service struct {
	store     Store
	threads   ThreadRetention
	publisher AuditPublisher
}

// NewService builds the service. threads may be nil: a thread hold is then placed
// without asking whether anything is retained, because an unneeded hold deletes
// nothing while a wrongly refused one could let held content be swept. publisher
// may be nil until the outbox loop is wired; writes still record their audit
// intent.
func NewService(store Store, threads ThreadRetention, publisher AuditPublisher) *Service {
	return &Service{store: store, threads: threads, publisher: publisher}
}

// Policy returns the stored policy for the admin read.
func (s *Service) Policy(ctx context.Context, orgID string) (*Policy, error) {
	return s.store.GetPolicy(ctx, orgID)
}

// PutPolicy validates the body and replaces the organisation's window. A body that
// fails validation is a *ValidationError and writes nothing.
func (s *Service) PutPolicy(ctx context.Context, orgID string, actor Actor, req PutRequest) (*Policy, error) {
	if strings.TrimSpace(actor.ID) == "" || strings.TrimSpace(actor.Role) == "" {
		return nil, errors.New("an actor id and role are required to write a retention policy")
	}
	if err := ValidatePut(req); err != nil {
		return nil, err
	}
	return s.store.PutPolicy(ctx, orgID, actor, *req.ExpectedRevision, req.ThreadDays)
}

// Holds returns the organisation's holds for the admin read.
func (s *Service) Holds(ctx context.Context, orgID string) (HoldList, error) {
	holds, err := s.store.ListHolds(ctx, orgID)
	if err != nil {
		return HoldList{}, err
	}
	if holds == nil {
		holds = []Hold{}
	}
	return HoldList{OrganizationID: orgID, Holds: holds}, nil
}

// PlaceHold validates and places a hold. A thread hold on a thread whose content
// was never retained is refused with ErrNothingRetained. When the retention answer
// is unavailable the hold is placed: holding too much is recoverable by a release,
// holding too little is not.
func (s *Service) PlaceHold(ctx context.Context, orgID string, actor Actor, req PlaceHoldRequest) (PlaceResult, error) {
	if strings.TrimSpace(actor.ID) == "" || strings.TrimSpace(actor.Role) == "" {
		return PlaceResult{}, errors.New("an actor id and role are required to place a legal hold")
	}
	scope, target, holdRef, err := NormalizePlace(orgID, req)
	if err != nil {
		return PlaceResult{}, err
	}
	if scope == ScopeThread && s.threads != nil {
		nothing, err := s.threads.NothingRetained(ctx, orgID, target)
		switch {
		case err != nil:
			log.Printf("retention: thread retention unknown for org=%s, placing the hold: %v", orgID, err)
		case nothing:
			return PlaceResult{}, ErrNothingRetained
		}
	}
	id, err := newHoldID()
	if err != nil {
		return PlaceResult{}, err
	}
	return s.store.PlaceHold(ctx, orgID, actor, Hold{
		ID:             id,
		OrganizationID: orgID,
		Scope:          scope,
		TargetRef:      target,
		HoldRef:        holdRef,
		CreatedBy:      actor.ID,
	})
}

// ReleaseHold releases one active hold.
func (s *Service) ReleaseHold(ctx context.Context, orgID, holdID string, actor Actor) (Hold, error) {
	if strings.TrimSpace(actor.ID) == "" || strings.TrimSpace(actor.Role) == "" {
		return Hold{}, errors.New("an actor id and role are required to release a legal hold")
	}
	holdID = strings.TrimSpace(holdID)
	if !validHoldID(holdID) {
		return Hold{}, ErrHoldNotFound
	}
	return s.store.ReleaseHold(ctx, orgID, holdID, actor)
}

// Check answers the sweeper: the window, the revision, whether the org is held,
// and which of the asked threads are held. Computed on every call; never cached.
func (s *Service) Check(ctx context.Context, orgID string, req CheckRequest) (CheckResult, error) {
	ids, err := NormalizeCheck(req)
	if err != nil {
		return CheckResult{}, err
	}
	result, err := s.store.Check(ctx, orgID, ids)
	if err != nil {
		return CheckResult{}, err
	}
	if result.HeldThreadIDs == nil {
		result.HeldThreadIDs = []string{}
	}
	result.OrganizationID = orgID
	return result, nil
}

// FlushAuditOutbox publishes pending retention audit intents, the same loop the
// SIEM and identity outboxes run.
func (s *Service) FlushAuditOutbox(ctx context.Context, limit int) (FlushResult, error) {
	if limit < 1 || limit > 1000 {
		return FlushResult{}, errors.New("retention audit outbox limit must be between 1 and 1000")
	}
	if s.publisher == nil {
		return FlushResult{}, errors.New("retention audit publisher is unavailable")
	}
	rows, err := s.store.ClaimAudit(ctx, limit)
	if err != nil {
		return FlushResult{}, err
	}
	result := FlushResult{}
	var publishErrors error
	for _, row := range rows {
		if err := s.publisher.PublishAudit(ctx, row.Subject, row.EventID, row.Payload); err != nil {
			deadLettered, markErr := s.store.MarkAuditFailed(ctx, row.EventID, err)
			if deadLettered {
				result.DeadLettered++
			}
			publishErrors = errors.Join(publishErrors, err, markErr)
			continue
		}
		if err := s.store.MarkAuditPublished(ctx, row.EventID); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		result.Published++
	}
	return result, publishErrors
}

// newHoldID is "lh_" and 32 hex characters, the shape migration 031 checks.
func newHoldID() (string, error) {
	var raw [16]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return "", fmt.Errorf("generate legal hold id: %w", err)
	}
	return "lh_" + hex.EncodeToString(raw[:]), nil
}

func validHoldID(id string) bool {
	if len(id) != 35 || !strings.HasPrefix(id, "lh_") {
		return false
	}
	for _, c := range id[3:] {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}
