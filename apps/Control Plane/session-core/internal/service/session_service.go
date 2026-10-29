package service

import (
	"context"
	"encoding/json"
	"fmt"
	"hash/fnv"
	"time"

	"github.com/rs/zerolog/log"

	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/clients"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/convex"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/domain"
	internalnats "github.com/I-Dacosta/CoreSystem/apps/session-core/internal/nats"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/redis"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/repository"
)

type SessionService struct {
	// G36-cutover Step D (2026-05-12): planRepo / approvalRepo / todoRepo /
	// lineageRepo removed. Rust session-core owns those concerns now; see
	// `Model Plane/rust/services/session-core/src/orchestration_http.rs`
	// (port 28083:8083). `repo` (SessionRepository) is kept because it backs
	// the Control Session aggregator (Wave 3 §8.17), which remains CP-side.
	repo       *repository.SessionRepository
	natsLocal  *internalnats.Client
	natsShared *internalnats.SharedPublisher
	cache      *redis.Client
	rolloutPct int // MODEL_PLANE_V2_ROLLOUT_PCT: 0-100, routes tenants to v2 via FNV-1a hash
	// convex is optional; nil when CONVEX_URL / CONVEX_SERVICE_KEY are unset.
	convex *convex.Client
	// orgClient is optional; nil when ORG_CORE_URL is unset.
	orgClient *clients.OrgClient
}

func NewSessionService(
	repo *repository.SessionRepository,
	natsLocal *internalnats.Client,
	natsShared *internalnats.SharedPublisher,
	cache *redis.Client,
	rolloutPct int,
	convexClient *convex.Client,
	orgClient *clients.OrgClient,
) *SessionService {
	return &SessionService{
		repo:       repo,
		natsLocal:  natsLocal,
		natsShared: natsShared,
		cache:      cache,
		rolloutPct: rolloutPct,
		convex:     convexClient,
		orgClient:  orgClient,
	}
}

// routeToV2 returns true when the tenant should be routed to Model Plane v2
// based on a stable FNV-1a hash of the tenant ID modulo 100.
func routeToV2(tenantID string, rolloutPct int) bool {
	if rolloutPct <= 0 {
		return false
	}
	if rolloutPct >= 100 {
		return true
	}
	h := fnv.New32a()
	h.Write([]byte(tenantID))
	return int(h.Sum32()%100) < rolloutPct
}

func (s *SessionService) CreateSession(ctx context.Context, userID string, req *domain.CreateSessionRequest) (*domain.Session, error) {
	version := domain.ModelPlaneV1
	explicitVersion := false
	if req.Metadata != nil {
		if v, ok := req.Metadata["model_plane_version"]; ok {
			if vs, ok := v.(string); ok && vs == "v2" {
				version = domain.ModelPlaneV2
				explicitVersion = true
			}
		}
	}
	if !explicitVersion && routeToV2(req.TenantID, s.rolloutPct) {
		version = domain.ModelPlaneV2
	}

	var role string
	if s.orgClient != nil {
		var memberErr error
		role, memberErr = s.orgClient.ValidateMembership(ctx, req.OrgID, userID)
		if memberErr != nil {
			return nil, fmt.Errorf("%w: %v", domain.ErrOrgMembershipDenied, memberErr)
		}
	}

	session := &domain.Session{
		TenantID:          req.TenantID,
		WorkspaceID:       req.WorkspaceID,
		UserID:            userID,
		OrgID:             req.OrgID,
		UserRole:          role,
		ModelPlaneVersion: version,
		Status:            domain.SessionStatusActive,
		PlanMode:          req.PlanMode,
		Metadata:          req.Metadata,
	}

	if err := s.repo.Create(ctx, session); err != nil {
		return nil, fmt.Errorf("create session: %w", err)
	}

	initPayload, _ := json.Marshal(map[string]any{
		"session_id":   session.ID,
		"tenant_id":    session.TenantID,
		"workspace_id": session.WorkspaceID,
		"user_id":      session.UserID,
		"plan_mode":    session.PlanMode,
	})

	if s.natsShared != nil {
		if err := s.natsShared.PublishSessionCommand(ctx, session.ID, "init", initPayload, string(version)); err != nil {
			log.Error().Err(err).Str("session_id", session.ID).Msg("Failed to publish session init command")
		}
	}

	eventPayload, _ := json.Marshal(map[string]any{
		"type":                "session.created",
		"model_plane_version": string(version),
	})
	evt, err := s.repo.AppendEvent(ctx, session.ID, "session.created", eventPayload)
	if err != nil {
		log.Error().Err(err).Str("session_id", session.ID).Msg("Failed to record session.created event")
	} else {
		s.publishSessionEvent(ctx, evt)
	}

	if s.cache != nil {
		_ = s.cache.CacheSessionState(ctx, session.ID, session, 30*time.Minute)
	}

	// Mirror session into Convex so the frontend can subscribe reactively.
	if s.convex != nil {
		go func() {
			syncCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if _, err := s.convex.CreateConversation(
				syncCtx,
				session.ID,
				session.TenantID,
				session.UserID,
				"", // title derived server-side
				session.PlanMode,
			); err != nil {
				log.Warn().Err(err).Str("session_id", session.ID).Msg("Convex conversation sync failed")
			}
		}()
	}

	return session, nil
}

func (s *SessionService) SendMessage(ctx context.Context, sessionID, actorUserID string, req *domain.SendMessageRequest) (*domain.SessionEvent, error) {
	session, err := s.getSession(ctx, sessionID)
	if err != nil {
		return nil, err
	}
	if err := requireSessionOwner(session, actorUserID); err != nil {
		return nil, err
	}

	if session.Status != domain.SessionStatusActive {
		return nil, fmt.Errorf("session is not active: %s", session.Status)
	}

	payload, _ := json.Marshal(map[string]any{
		"role":    req.Role,
		"content": req.Content,
	})

	evt, err := s.repo.AppendEvent(ctx, sessionID, "message.sent", payload)
	if err != nil {
		return nil, fmt.Errorf("append message event: %w", err)
	}
	s.publishSessionEvent(ctx, evt)

	if s.natsShared != nil {
		if err := s.natsShared.PublishSessionCommand(ctx, sessionID, "message", payload, string(session.ModelPlaneVersion)); err != nil {
			log.Error().Err(err).Str("session_id", sessionID).Msg("Failed to publish message command")
		}
	}

	if s.cache != nil {
		_ = s.cache.InvalidateSessionCache(ctx, sessionID)
	}

	// Mirror message into Convex for reactive frontend subscriptions.
	if s.convex != nil {
		go func() {
			syncCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if _, err := s.convex.PostMessage(syncCtx, sessionID, req.Role, req.Content); err != nil {
				log.Warn().Err(err).Str("session_id", sessionID).Msg("Convex message sync failed")
			}
		}()
	}

	return evt, nil
}

func (s *SessionService) GetSessionState(ctx context.Context, sessionID, actorUserID string) (*domain.SessionState, error) {
	session, err := s.getSession(ctx, sessionID)
	if err != nil {
		return nil, err
	}
	if err := requireSessionOwner(session, actorUserID); err != nil {
		return nil, err
	}

	// G36-cutover Step D: only the legacy session-scoped approvals path
	// remains. The model-plane-scoped approval repository was decommissioned
	// alongside plan/todo/lineage and now lives in Rust session-core.
	approvals, err := s.repo.GetPendingApprovals(ctx, sessionID)
	if err != nil {
		return nil, fmt.Errorf("get pending approvals: %w", err)
	}

	var cursor int64
	if s.cache != nil {
		cursor, _ = s.cache.GetEventCursor(ctx, sessionID)
	}

	return &domain.SessionState{
		Session:          *session,
		PendingApprovals: approvals,
		EventCursor:      cursor,
	}, nil
}

func (s *SessionService) GetEventsSince(ctx context.Context, sessionID string, afterSequence int64, limit int) ([]domain.SessionEvent, error) {
	if limit <= 0 || limit > 1000 {
		limit = 100
	}
	return s.repo.GetEventsSince(ctx, sessionID, afterSequence, limit)
}

func (s *SessionService) ResolveApproval(ctx context.Context, sessionID, approvalID, actorUserID string, req *domain.ApprovalDecisionRequest) error {
	session, err := s.getSession(ctx, sessionID)
	if err != nil {
		return err
	}
	if err := requireSessionOwner(session, actorUserID); err != nil {
		return err
	}

	// G36-cutover Step D: only the legacy session-scoped path remains
	// (see GetSessionState comment).
	if err := s.repo.ResolveApproval(ctx, approvalID, req.Approve, req.Feedback); err != nil {
		return fmt.Errorf("resolve approval: %w", err)
	}

	eventPayload, _ := json.Marshal(map[string]any{
		"approval_id": approvalID,
		"approved":    req.Approve,
		"feedback":    req.Feedback,
	})
	evt, err := s.repo.AppendEvent(ctx, sessionID, "approval.resolved", eventPayload)
	if err != nil {
		log.Error().Err(err).Msg("Failed to record approval event")
	} else {
		s.publishSessionEvent(ctx, evt)
	}

	if s.natsShared != nil {
		resumePayload, _ := json.Marshal(map[string]any{
			"approval_id": approvalID,
			"approved":    req.Approve,
			"feedback":    req.Feedback,
		})
		if err := s.natsShared.PublishSessionCommand(ctx, sessionID, "resume", resumePayload, string(session.ModelPlaneVersion)); err != nil {
			log.Error().Err(err).Msg("Failed to publish resume command")
		}
	}

	if s.cache != nil {
		_ = s.cache.InvalidateSessionCache(ctx, sessionID)
	}

	return nil
}

func (s *SessionService) ResumeSession(ctx context.Context, sessionID, actorUserID string) error {
	session, err := s.getSession(ctx, sessionID)
	if err != nil {
		return err
	}
	if err := requireSessionOwner(session, actorUserID); err != nil {
		return err
	}

	if s.natsShared != nil {
		resumePayload, _ := json.Marshal(map[string]any{
			"session_id": sessionID,
		})
		if err := s.natsShared.PublishSessionCommand(ctx, sessionID, "resume", resumePayload, string(session.ModelPlaneVersion)); err != nil {
			return fmt.Errorf("publish resume command: %w", err)
		}
	}

	return nil
}

func (s *SessionService) getSession(ctx context.Context, sessionID string) (*domain.Session, error) {
	if s.cache != nil {
		var cached domain.Session
		if err := s.cache.GetCachedSessionState(ctx, sessionID, &cached); err == nil {
			return &cached, nil
		}
	}

	session, err := s.repo.GetByID(ctx, sessionID)
	if err != nil {
		return nil, err
	}

	if s.cache != nil {
		_ = s.cache.CacheSessionState(ctx, sessionID, session, 30*time.Minute)
	}

	return session, nil
}

func requireSessionOwner(session *domain.Session, actorUserID string) error {
	if session == nil || actorUserID == "" || session.UserID == "" || session.UserID != actorUserID {
		return domain.ErrSessionAccessDenied
	}
	return nil
}

// G36-cutover Step D (2026-05-12): the Plans / Todos / Lineage service
// methods, their repositories, and their HTTP handlers were removed from
// CP session-core. Rust session-core's `orchestration_http.rs` (port
// 28083:8083) is the new authoritative owner — see §8.24 + §8.26 of
// `apps/Frontend Plane/verevon/verevon-gap.md`. The corresponding tables
// (`plans`, `plan_steps`, `todos`, `subagent_edges`, `approvals` model-plane
// rows) are dropped by migration `040_drop_agent_run_scaffold.up.sql`.

func (s *SessionService) publishSessionEvent(ctx context.Context, evt *domain.SessionEvent) {
	if s.natsShared == nil || evt == nil {
		return
	}
	if err := s.natsShared.PublishSessionEvent(ctx, evt.SessionID, evt.Sequence, evt.EventType, evt.Payload, evt.CreatedAt); err != nil {
		log.Warn().Err(err).Str("session_id", evt.SessionID).Str("event_type", evt.EventType).Msg("Failed to publish session event")
	}
}

// G36-cutover Step D (2026-05-12): `mapModelApprovalToLegacy` removed — it
// only existed to bridge the now-removed model-plane-scoped `approvalRepo`
// path into the legacy `domain.Approval` shape that the SSE event consumer
// expects. With the model-plane path moved to Rust, the bridge has no
// callers.
