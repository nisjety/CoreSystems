package users

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"strings"
	"sync"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

type ErasureMode string

const (
	ErasureModeHardDelete ErasureMode = "hard_delete"
	ErasureModeAnonymize  ErasureMode = "anonymize"
)

type ErasureOperation struct {
	OperationID       string
	UserID            string
	Mode              ErasureMode
	ActorID           string
	ActorRole         string
	OrgID             string
	Attempts          int
	NextAttemptAt     time.Time
	ProcessingAt      *time.Time
	AuthCompletedAt   *time.Time
	LocalCompletedAt  *time.Time
	LocalUserDeleted  bool
	AuditEnqueuedAt   *time.Time
	FanoutSnapshotAt  *time.Time
	FanoutPublishedAt *time.Time
	CompletedAt       *time.Time
	LastError         *string
	CreatedAt         time.Time
	UpdatedAt         time.Time
}

// ErasureReceipt deliberately exposes only durable stage evidence. The raw
// Auth receipt can contain deleted record identifiers and is never retained or
// returned by user-core.
type ErasureReceipt struct {
	OperationID     string      `json:"operation_id"`
	Success         bool        `json:"success"`
	UserID          string      `json:"user_id"`
	Mode            ErasureMode `json:"mode"`
	AuthCompleted   bool        `json:"auth_completed"`
	LocalDeleted    bool        `json:"local_user_deleted"`
	AuditEnqueued   bool        `json:"audit_enqueued"`
	FanoutPublished bool        `json:"fanout_published"`
	ErasedAt        *time.Time  `json:"erased_at,omitempty"`
}

type authErasureExecutor interface {
	Execute(context.Context, ErasureMode, string) ([]byte, error)
}

type ErasureFanoutPublisher interface {
	PublishGDPRErasure(context.Context, string, []byte) error
}

type pgxAuthErasureExecutor struct{ pool *pgxpool.Pool }

func (executor *pgxAuthErasureExecutor) Execute(ctx context.Context, mode ErasureMode, userID string) ([]byte, error) {
	var receipt []byte
	var err error
	switch mode {
	case ErasureModeHardDelete:
		err = executor.pool.QueryRow(ctx, `SELECT gdpr_hard_delete_user($1)`, userID).Scan(&receipt)
	case ErasureModeAnonymize:
		err = executor.pool.QueryRow(ctx, `SELECT gdpr_anonymize_user($1)`, userID).Scan(&receipt)
	default:
		return nil, fmt.Errorf("unsupported erasure mode")
	}
	if err != nil {
		return nil, fmt.Errorf("execute Auth erasure: %w", err)
	}
	return receipt, nil
}

type authErasureReceipt struct {
	Success *bool  `json:"success"`
	UserID  string `json:"user_id"`
	Error   string `json:"error"`
}

func validateAuthErasureReceipt(raw []byte, expectedUserID string) error {
	var receipt authErasureReceipt
	if err := json.Unmarshal(raw, &receipt); err != nil {
		return fmt.Errorf("decode Auth erasure receipt: %w", err)
	}
	if receipt.Success == nil || !*receipt.Success {
		if strings.TrimSpace(receipt.Error) != "" {
			return fmt.Errorf("auth erasure receipt reported failure")
		}
		return fmt.Errorf("auth erasure receipt success must be true")
	}
	if strings.TrimSpace(receipt.UserID) != expectedUserID {
		return fmt.Errorf("auth erasure receipt returned unexpected user")
	}
	return nil
}

func erasureOperationID(userID string, mode ErasureMode) string {
	digest := sha256.Sum256([]byte(string(mode) + "\x00" + userID))
	return fmt.Sprintf("gdpr:user-core:%s:%x", mode, digest)
}

func erasureFanoutChildID(operationID, orgID string) string {
	digest := sha256.Sum256([]byte(operationID + "\x00" + orgID))
	return fmt.Sprintf("gdpr:fanout:%x", digest)
}

func (s *Service) SetAuthPool(pool *pgxpool.Pool) {
	if pool == nil {
		s.authEraser = nil
		return
	}
	s.authEraser = &pgxAuthErasureExecutor{pool: pool}
}

func (s *Service) ErasureAvailable() bool {
	return s != nil && s.erasureStore != nil && s.authEraser != nil
}

// ResolveErasureAuditOrg derives audit scope only from the local canonical
// membership projection. A cryptographically verified delegated org is used
// only when the target has an active membership in it; otherwise the target's
// deterministic primary active membership is selected server-side.
func (s *Service) ResolveErasureAuditOrg(ctx context.Context, targetID, verifiedOrgHint string) (string, error) {
	if s == nil || s.repo == nil {
		return "", fmt.Errorf("user repository is unavailable")
	}
	targetID = strings.TrimSpace(targetID)
	if targetID == "" {
		return "", fmt.Errorf("user ID is required")
	}
	if verifiedOrgHint = strings.TrimSpace(verifiedOrgHint); verifiedOrgHint != "" {
		membership, err := s.repo.GetUserOrgMembership(ctx, targetID, verifiedOrgHint)
		if err != nil {
			return "", fmt.Errorf("verify erasure audit organization: %w", err)
		}
		if membership != nil {
			return membership.OrgID, nil
		}
	}
	membership, err := s.repo.GetPrimaryUserOrgMembership(ctx, targetID)
	if err != nil {
		return "", fmt.Errorf("resolve erasure audit organization: %w", err)
	}
	if membership == nil || strings.TrimSpace(membership.OrgID) == "" {
		return "", fmt.Errorf("active organization membership required for audit scope")
	}
	return membership.OrgID, nil
}

func (s *Service) HardEraseUser(ctx context.Context, userID, actorID, actorRole, orgID string) (*ErasureReceipt, error) {
	return s.executeErasure(ctx, ErasureOperation{
		OperationID: erasureOperationID(strings.TrimSpace(userID), ErasureModeHardDelete),
		UserID:      strings.TrimSpace(userID), Mode: ErasureModeHardDelete,
		ActorID: strings.TrimSpace(actorID), ActorRole: strings.TrimSpace(actorRole), OrgID: strings.TrimSpace(orgID),
	})
}

func (s *Service) AnonymizeUser(ctx context.Context, userID, actorID, actorRole, orgID string) (*ErasureReceipt, error) {
	return s.executeErasure(ctx, ErasureOperation{
		OperationID: erasureOperationID(strings.TrimSpace(userID), ErasureModeAnonymize),
		UserID:      strings.TrimSpace(userID), Mode: ErasureModeAnonymize,
		ActorID: strings.TrimSpace(actorID), ActorRole: strings.TrimSpace(actorRole), OrgID: strings.TrimSpace(orgID),
	})
}

func validateErasureOperation(operation ErasureOperation) error {
	if operation.UserID == "" || operation.ActorID == "" || operation.OrgID == "" {
		return fmt.Errorf("user, actor, and verified organization are required")
	}
	if operation.ActorRole != "self" && operation.ActorRole != "admin" {
		return fmt.Errorf("verified self or admin authority is required")
	}
	if operation.Mode != ErasureModeHardDelete && operation.Mode != ErasureModeAnonymize {
		return fmt.Errorf("invalid erasure mode")
	}
	return nil
}

func (s *Service) executeErasure(ctx context.Context, requested ErasureOperation) (*ErasureReceipt, error) {
	if err := validateErasureOperation(requested); err != nil {
		return nil, err
	}
	if !s.ErasureAvailable() {
		return nil, fmt.Errorf("auth database not configured (AUTH_DATABASE_URL); erasure saga unavailable")
	}
	operation, err := s.erasureStore.BeginErasureOperation(ctx, requested)
	if err != nil {
		return nil, err
	}
	if operation.CompletedAt != nil {
		receipt := erasureReceipt(operation)
		return &receipt, nil
	}
	operation, claimed, err := s.erasureStore.ClaimErasureOperation(ctx, operation.OperationID)
	if err != nil {
		return nil, err
	}
	if !claimed {
		receipt := erasureReceipt(operation)
		return &receipt, fmt.Errorf("erasure operation is pending retry")
	}
	processed, err := s.processClaimedErasure(ctx, operation)
	if err != nil {
		receipt := erasureReceipt(processed)
		nextAttempt := time.Now().UTC().Add(erasureRetryDelay(processed.Attempts))
		if recordErr := s.erasureStore.FailErasureOperation(ctx, processed.OperationID, processed.Attempts, nextAttempt, err.Error()); recordErr != nil {
			return &receipt, fmt.Errorf("%v; persist erasure retry: %w", err, recordErr)
		}
		return &receipt, err
	}
	receipt := erasureReceipt(processed)
	return &receipt, nil
}

func (s *Service) processClaimedErasure(ctx context.Context, operation ErasureOperation) (ErasureOperation, error) {
	if operation.AuthCompletedAt == nil {
		rawReceipt, err := s.authEraser.Execute(ctx, operation.Mode, operation.UserID)
		if err != nil {
			return operation, err
		}
		if err := validateAuthErasureReceipt(rawReceipt, operation.UserID); err != nil {
			return operation, err
		}
		if err := s.erasureStore.MarkErasureAuthCompleted(ctx, operation.OperationID, operation.Attempts); err != nil {
			return operation, err
		}
		refreshed, getErr := s.erasureStore.GetErasureOperation(ctx, operation.OperationID)
		if getErr != nil {
			return operation, getErr
		}
		operation = refreshed
	}

	if operation.LocalCompletedAt == nil {
		if err := s.erasureStore.CompleteErasureLocalStage(ctx, operation); err != nil {
			return operation, err
		}
		if s.cache != nil {
			_ = s.cache.Del(ctx, userIDKeyPrefix+operation.UserID)
		}
		var err error
		operation, err = s.erasureStore.GetErasureOperation(ctx, operation.OperationID)
		if err != nil {
			return operation, err
		}
	}

	if operation.AuditEnqueuedAt == nil {
		row, err := erasureAuditIntent(operation)
		if err != nil {
			return operation, err
		}
		err = s.auditOutbox.EnqueueAndDispatch(ctx, row)
		var deferred *auditDispatchDeferredError
		if err != nil && !errors.As(err, &deferred) {
			return operation, err
		}
		if err := s.erasureStore.MarkErasureAuditEnqueued(ctx, operation.OperationID, operation.Attempts); err != nil {
			return operation, err
		}
		operation, err = s.erasureStore.GetErasureOperation(ctx, operation.OperationID)
		if err != nil {
			return operation, err
		}
	}

	if operation.FanoutPublishedAt == nil {
		if s.erasureFanout == nil {
			return operation, fmt.Errorf("GDPR fan-out publisher is unavailable")
		}
		for {
			child, found, err := s.erasureStore.ClaimNextErasureFanout(ctx, operation.OperationID)
			if err != nil {
				return operation, err
			}
			if !found {
				break
			}
			payload, err := erasureFanoutPayload(operation, child)
			if err == nil {
				err = s.erasureFanout.PublishGDPRErasure(ctx, child.ChildEventID, payload)
			}
			if err != nil {
				terminal := child.Attempts >= maxErasureFanoutAttempts
				nextAttempt := time.Now().UTC().Add(erasureRetryDelay(child.Attempts))
				if failErr := s.erasureStore.FailErasureFanout(ctx, child.ChildEventID, child.Attempts, nextAttempt, err.Error(), terminal); failErr != nil {
					return operation, fmt.Errorf("publish GDPR fan-out: %v; persist child retry: %w", err, failErr)
				}
				return operation, fmt.Errorf("publish GDPR fan-out: %w", err)
			}
			if err := s.erasureStore.CompleteErasureFanoutChild(ctx, child.ChildEventID, child.Attempts); err != nil {
				return operation, err
			}
		}
		if err := s.erasureStore.CompleteErasureFanout(ctx, operation.OperationID, operation.Attempts); err != nil {
			return operation, err
		}
		completed, getErr := s.erasureStore.GetErasureOperation(ctx, operation.OperationID)
		if getErr != nil {
			return operation, getErr
		}
		operation = completed
	}
	return operation, nil
}

func erasureAuditIntent(operation ErasureOperation) (AuditOutboxRow, error) {
	payload, err := json.Marshal(map[string]any{
		"event_id": operation.OperationID + ":audit", "occurred_at": operation.CreatedAt.UTC().Format(time.RFC3339Nano),
		"org_id": operation.OrgID, "user_id": operation.ActorID, "actor_role": operation.ActorRole,
		"plane": "control", "producer": "user-core", "event": "erasure",
		"subject": "user:" + operation.UserID, "resource_id": operation.UserID, "outcome": "ok",
		"details": map[string]any{"operation_id": operation.OperationID, "mode": operation.Mode, "local_user_deleted": operation.LocalUserDeleted},
	})
	if err != nil {
		return AuditOutboxRow{}, fmt.Errorf("encode erasure audit intent: %w", err)
	}
	return AuditOutboxRow{EventID: operation.OperationID + ":audit", Subject: ErasureAuditSubject, Payload: payload}, nil
}

func erasureFanoutPayload(operation ErasureOperation, child ErasureFanoutChild) ([]byte, error) {
	subjectType := "user"
	if operation.Mode == ErasureModeAnonymize {
		subjectType = "user_anonymize"
	}
	payload, err := json.Marshal(map[string]any{
		"event_id": child.ChildEventID, "operation_id": operation.OperationID, "subject_type": subjectType,
		"subject_id": operation.UserID, "org_id": child.OrgID,
		"requested_by": operation.ActorID, "mode": operation.Mode,
		"ts": operation.CreatedAt.UTC().Format(time.RFC3339Nano),
	})
	if err != nil {
		return nil, fmt.Errorf("encode GDPR fan-out: %w", err)
	}
	return payload, nil
}

func erasureReceipt(operation ErasureOperation) ErasureReceipt {
	return ErasureReceipt{
		OperationID: operation.OperationID, Success: operation.CompletedAt != nil,
		UserID: operation.UserID, Mode: operation.Mode,
		AuthCompleted: operation.AuthCompletedAt != nil, LocalDeleted: operation.LocalUserDeleted,
		AuditEnqueued: operation.AuditEnqueuedAt != nil, FanoutPublished: operation.FanoutPublishedAt != nil,
		ErasedAt: operation.CompletedAt,
	}
}

func erasureRetryDelay(attempt int) time.Duration {
	if attempt < 1 {
		attempt = 1
	}
	seconds := min(attempt*attempt, 300)
	return time.Duration(seconds) * time.Second
}

type erasureWorker struct {
	service *Service
	cancel  context.CancelFunc
	wg      sync.WaitGroup
}

func (s *Service) StartErasureSaga() {
	if !s.ErasureAvailable() || s.erasureWorker != nil {
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	worker := &erasureWorker{service: s, cancel: cancel}
	worker.wg.Add(1)
	s.erasureWorker = worker
	go worker.run(ctx)
}

func (s *Service) CloseErasureSaga() {
	if s == nil || s.erasureWorker == nil {
		return
	}
	s.erasureWorker.cancel()
	s.erasureWorker.wg.Wait()
}

func (worker *erasureWorker) run(ctx context.Context) {
	defer worker.wg.Done()
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		for range 20 {
			operation, found, err := worker.service.erasureStore.ClaimNextErasureOperation(ctx)
			if err != nil || !found {
				if err != nil && !errors.Is(err, context.Canceled) {
					log.Printf("user-core erasure saga claim: %v", err)
				}
				break
			}
			if _, err := worker.service.processClaimedErasure(ctx, operation); err != nil {
				nextAttempt := time.Now().UTC().Add(erasureRetryDelay(operation.Attempts))
				if recordErr := worker.service.erasureStore.FailErasureOperation(ctx, operation.OperationID, operation.Attempts, nextAttempt, err.Error()); recordErr != nil {
					log.Printf("user-core erasure saga %s failed: %v; retry state: %v", operation.OperationID, err, recordErr)
				} else {
					log.Printf("user-core erasure saga %s retained for retry: %v", operation.OperationID, err)
				}
				break
			}
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
