package http

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/domain"
)

// --- Health ---

func (s *Server) healthCheck(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"status":  "healthy",
		"service": "session-core",
		"version": "0.1.0",
	})
}

// --- Sessions ---

func (s *Server) createSession(c *gin.Context) {
	userID := getUserID(c)
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	var req domain.CreateSessionRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	// Idempotency check
	if idemKey := c.GetHeader("X-Idempotency-Key"); idemKey != "" && s.cache != nil {
		duplicate, err := s.cache.CheckAndSetIdempotency(c.Request.Context(), idemKey, 24*time.Hour)
		if err == nil && duplicate {
			c.JSON(http.StatusConflict, gin.H{"error": "duplicate request"})
			return
		}
	}

	session, err := s.sessionService.CreateSession(c.Request.Context(), userID, &req)
	if err != nil {
		if errors.Is(err, domain.ErrOrgMembershipDenied) {
			c.JSON(http.StatusForbidden, gin.H{"error": "org membership required"})
			return
		}
		log.Error().Err(err).Str("user_id", userID).Msg("Failed to create session")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to create session"})
		return
	}

	c.JSON(http.StatusCreated, gin.H{"session": session})
}

func (s *Server) getSessionState(c *gin.Context) {
	sessionID := c.Param("id")
	if sessionID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "session id required"})
		return
	}
	userID := getUserID(c)
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	state, err := s.sessionService.GetSessionState(c.Request.Context(), sessionID, userID)
	if err != nil {
		log.Error().Err(err).Str("session_id", sessionID).Msg("Failed to get session state")
		c.JSON(http.StatusNotFound, gin.H{"error": "session not found"})
		return
	}

	c.JSON(http.StatusOK, state)
}

func (s *Server) sendMessage(c *gin.Context) {
	sessionID := c.Param("id")
	userID := getUserID(c)
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	var req domain.SendMessageRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	evt, err := s.sessionService.SendMessage(c.Request.Context(), sessionID, userID, &req)
	if err != nil {
		if errors.Is(err, domain.ErrSessionAccessDenied) {
			c.JSON(http.StatusNotFound, gin.H{"error": "session not found"})
			return
		}
		log.Error().Err(err).Str("session_id", sessionID).Msg("Failed to send message")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to send message"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"event": evt})
}

func (s *Server) resolveApproval(c *gin.Context) {
	sessionID := c.Param("id")
	approvalID := c.Param("approval_id")
	userID := getUserID(c)
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	var req domain.ApprovalDecisionRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	if err := s.sessionService.ResolveApproval(c.Request.Context(), sessionID, approvalID, userID, &req); err != nil {
		if errors.Is(err, domain.ErrSessionAccessDenied) {
			c.JSON(http.StatusNotFound, gin.H{"error": "session not found"})
			return
		}
		log.Error().Err(err).Str("approval_id", approvalID).Msg("Failed to resolve approval")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve approval"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"status": "resolved"})
}

func (s *Server) resumeSession(c *gin.Context) {
	sessionID := c.Param("id")
	userID := getUserID(c)
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	if err := s.sessionService.ResumeSession(c.Request.Context(), sessionID, userID); err != nil {
		if errors.Is(err, domain.ErrSessionAccessDenied) {
			c.JSON(http.StatusNotFound, gin.H{"error": "session not found"})
			return
		}
		log.Error().Err(err).Str("session_id", sessionID).Msg("Failed to resume session")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resume session"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"status": "resumed"})
}

// G36-cutover Step D (2026-05-12): the Plans / Todos / Lineage handler
// functions were removed. Rust session-core's `orchestration_http.rs`
// (port 28083:8083) owns those concerns now — see §8.24 + §8.26 of
// `apps/Frontend Plane/velion/velion-gap.md`.

// --- SSE (Server-Sent Events) ---

func (s *Server) streamEvents(c *gin.Context) {
	sessionID := c.Param("id")
	if sessionID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "session id required"})
		return
	}
	userID := getUserID(c)
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	// Parse last-event-id for cursor recovery
	var afterSequence int64
	if lastID := c.GetHeader("Last-Event-ID"); lastID != "" {
		if seq, err := strconv.ParseInt(lastID, 10, 64); err == nil {
			afterSequence = seq
		}
	}
	if qs := c.Query("after_sequence"); qs != "" {
		if seq, err := strconv.ParseInt(qs, 10, 64); err == nil {
			afterSequence = seq
		}
	}

	// Verify session exists
	_, err := s.sessionService.GetSessionState(c.Request.Context(), sessionID, userID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "session not found"})
		return
	}

	// Set SSE headers
	c.Writer.Header().Set("Content-Type", "text/event-stream")
	c.Writer.Header().Set("Cache-Control", "no-cache")
	c.Writer.Header().Set("Connection", "keep-alive")
	c.Writer.Header().Set("X-Accel-Buffering", "no")
	c.Writer.WriteHeader(http.StatusOK)
	c.Writer.Flush()

	ctx, cancel := context.WithCancel(c.Request.Context())
	defer cancel()

	// Subscribe before replay to avoid missing a race window between replay and live.
	eventsCh := make(chan domain.SessionEvent, 128)
	var subscribed bool
	if s.natsShared != nil {
		sub, err := s.natsShared.SubscribeSessionEvents(sessionID, func(msg *nats.Msg) {
			evt, parseErr := decodeSessionEvent(msg.Data)
			if parseErr != nil {
				return
			}
			select {
			case eventsCh <- evt:
			default:
			}
		})
		if err == nil {
			subscribed = true
			defer sub.Unsubscribe()
		} else {
			log.Warn().Err(err).Str("session_id", sessionID).Msg("NATS subscription failed, falling back to Postgres polling")
		}
	}

	// Replay any events we missed from Postgres.
	existingEvents, _ := s.sessionService.GetEventsSince(ctx, sessionID, afterSequence, 1000)
	for _, evt := range existingEvents {
		if err := writeSSEEvent(c.Writer, evt); err != nil {
			return
		}
		afterSequence = evt.Sequence
	}
	c.Writer.Flush()

	if subscribed {
		s.streamFromNATS(ctx, c.Writer, eventsCh, sessionID, afterSequence)
		return
	}

	s.streamFromPostgres(ctx, c.Writer, sessionID, afterSequence)
}

func (s *Server) streamFromNATS(ctx context.Context, w http.ResponseWriter, events <-chan domain.SessionEvent, sessionID string, lastSeq int64) {
	flusher, ok := w.(http.Flusher)
	if !ok {
		return
	}

	heartbeat := time.NewTicker(15 * time.Second)
	defer heartbeat.Stop()

	cursor := lastSeq
	for {
		select {
		case <-ctx.Done():
			return
		case <-heartbeat.C:
			_, _ = fmt.Fprintf(w, ": heartbeat\n\n")
			flusher.Flush()
		case evt := <-events:
			if evt.Sequence <= cursor {
				continue
			}
			if err := writeSSEEvent(w, evt); err != nil {
				return
			}
			cursor = evt.Sequence
			flusher.Flush()
			if s.cache != nil {
				_ = s.cache.SetEventCursor(ctx, sessionID, cursor)
			}
		}
	}
}

func (s *Server) streamFromPostgres(ctx context.Context, w http.ResponseWriter, sessionID string, lastSeq int64) {
	flusher, ok := w.(http.Flusher)
	if !ok {
		return
	}

	ticker := time.NewTicker(500 * time.Millisecond)
	defer ticker.Stop()

	heartbeat := time.NewTicker(15 * time.Second)
	defer heartbeat.Stop()

	cursor := lastSeq

	for {
		select {
		case <-ctx.Done():
			return
		case <-heartbeat.C:
			_, _ = fmt.Fprintf(w, ": heartbeat\n\n")
			flusher.Flush()
		case <-ticker.C:
			events, err := s.sessionService.GetEventsSince(ctx, sessionID, cursor, 100)
			if err != nil {
				continue
			}
			for _, evt := range events {
				if err := writeSSEEvent(w, evt); err != nil {
					return
				}
				cursor = evt.Sequence
			}
			if len(events) > 0 {
				flusher.Flush()
				if s.cache != nil {
					_ = s.cache.SetEventCursor(ctx, sessionID, cursor)
				}
			}
		}
	}
}

func writeSSEEvent(w io.Writer, evt domain.SessionEvent) error {
	data := map[string]any{
		"id":         evt.ID,
		"session_id": evt.SessionID,
		"sequence":   evt.Sequence,
		"event_type": evt.EventType,
		"payload":    json.RawMessage(evt.Payload),
		"created_at": evt.CreatedAt,
	}
	b, _ := json.Marshal(data)
	_, err := fmt.Fprintf(w, "id: %d\nevent: %s\ndata: %s\n\n", evt.Sequence, evt.EventType, b)
	return err
}

func decodeSessionEvent(raw []byte) (domain.SessionEvent, error) {
	var envelope struct {
		SessionID string    `json:"session_id"`
		Sequence  int64     `json:"sequence"`
		EventType string    `json:"event_type"`
		Payload   string    `json:"payload"`
		CreatedAt time.Time `json:"created_at"`
	}

	if err := json.Unmarshal(raw, &envelope); err != nil {
		return domain.SessionEvent{}, err
	}

	return domain.SessionEvent{
		SessionID: envelope.SessionID,
		Sequence:  envelope.Sequence,
		EventType: envelope.EventType,
		Payload:   []byte(envelope.Payload),
		CreatedAt: envelope.CreatedAt,
	}, nil
}

func parsePagination(c *gin.Context) (int, int) {
	const (
		defaultLimit = 50
		maxLimit     = 200
	)

	limit := defaultLimit
	if raw := c.Query("limit"); raw != "" {
		if parsed, err := strconv.Atoi(raw); err == nil {
			limit = parsed
		}
	}
	if limit <= 0 {
		limit = defaultLimit
	}
	if limit > maxLimit {
		limit = maxLimit
	}

	offset := 0
	if raw := c.Query("offset"); raw != "" {
		if parsed, err := strconv.Atoi(raw); err == nil {
			offset = parsed
		}
	}
	if offset < 0 {
		offset = 0
	}

	return limit, offset
}

// --- Helpers ---

func getUserID(c *gin.Context) string {
	if id, exists := c.Get("user_id"); exists {
		if s, ok := id.(string); ok && s != "" {
			return s
		}
	}
	return ""
}
