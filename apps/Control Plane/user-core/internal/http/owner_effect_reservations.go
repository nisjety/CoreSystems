package http

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
)

// ownerEffectReservationRequest contains the signed decision only for the
// direct Conversation-Core → Control hop. The handler verifies it and passes
// only the content-free commitment into Control's durable reservation ledger.
type ownerEffectReservationRequest struct {
	ControlDecisionToken string                                  `json:"control_decision_token"`
	Commitment           spaces.OwnerEffectReservationCommitment `json:"commitment"`
}

func (s *Server) reserveOwnerEffect(c *gin.Context) {
	decision, request, ok := s.ownerEffectReservationRequest(c)
	if !ok {
		return
	}
	reservation, err := s.spaceRepo.ReserveOwnerEffect(c.Request.Context(), decision, request.Commitment, time.Now().UTC())
	if err != nil {
		writeOwnerEffectReservationError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": ownerEffectReservationResponse(reservation)})
}

func (s *Server) commitOwnerEffectReservation(c *gin.Context) {
	decision, _, ok := s.ownerEffectReservationRequest(c)
	if !ok {
		return
	}
	reservation, err := s.spaceRepo.CommitOwnerEffectReservation(c.Request.Context(), strings.TrimSpace(c.Param("reservation_id")), decision, time.Now().UTC())
	if err != nil {
		writeOwnerEffectReservationError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": ownerEffectReservationResponse(reservation)})
}

func (s *Server) getOwnerEffectReservation(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "owner effect reservation is unavailable"})
		return
	}
	reservation, err := s.spaceRepo.GetOwnerEffectReservation(c.Request.Context(), strings.TrimSpace(c.Param("reservation_id")))
	if err != nil {
		writeOwnerEffectReservationError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": ownerEffectReservationResponse(reservation)})
}

func (s *Server) ownerEffectReservationRequest(c *gin.Context) (spaces.RunActionDecision, ownerEffectReservationRequest, bool) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "owner effect reservation is unavailable"})
		return spaces.RunActionDecision{}, ownerEffectReservationRequest{}, false
	}
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, 24*1024))
	decoder.DisallowUnknownFields()
	var request ownerEffectReservationRequest
	if err := decoder.Decode(&request); err != nil || decoder.Decode(&struct{}{}) != io.EOF || request.Commitment.Validate() != nil || strings.TrimSpace(request.ControlDecisionToken) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid owner effect reservation is required"})
		return spaces.RunActionDecision{}, ownerEffectReservationRequest{}, false
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "owner effect reservation verifier unavailable"})
		return spaces.RunActionDecision{}, ownerEffectReservationRequest{}, false
	}
	decision, err := spaces.VerifyRunActionDecision(key, request.ControlDecisionToken, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "owner effect reservation denied"})
		return spaces.RunActionDecision{}, ownerEffectReservationRequest{}, false
	}
	return decision, request, true
}

func writeOwnerEffectReservationError(c *gin.Context, err error) {
	if errors.Is(err, spaces.ErrOwnerEffectReservationDenied) || errors.Is(err, spaces.ErrOwnerEffectReservationConflict) {
		c.JSON(http.StatusForbidden, gin.H{"error": "owner effect reservation denied"})
		return
	}
	c.JSON(http.StatusServiceUnavailable, gin.H{"error": "owner effect reservation is unavailable"})
}

func ownerEffectReservationResponse(reservation *spaces.OwnerEffectReservation) gin.H {
	if reservation == nil {
		return gin.H{}
	}
	return gin.H{
		"reservation_id": reservation.ReservationID,
		"operation_id":   reservation.Commitment.OperationID,
		"status":         reservation.Status,
		"expires_at":     reservation.ExpiresAt,
		"committed_at":   reservation.CommittedAt,
		"cancelled_at":   reservation.CancelledAt,
		"reason":         reservation.Reason,
	}
}
