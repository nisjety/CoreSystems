package http

import (
	"errors"
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"

	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/service"
)

// getControlSessionCurrent handles GET /api/v1/sessions/current.
// G10 Step 1: serves the aggregated Control Session snapshot for the
// authenticated user (X-User-Id from the internal proxy).
func (s *Server) getControlSessionCurrent(c *gin.Context) {
	if s.controlSessionService == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "control-session service not configured"})
		return
	}

	userID, ok := userIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	snap, err := s.controlSessionService.Get(c.Request.Context(), userID)
	if err != nil {
		if errors.Is(err, service.ErrUserCoreUnavailable) {
			log.Warn().Err(err).Str("user_id", userID).Msg("control-session: upstream unavailable")
			c.JSON(http.StatusBadGateway, gin.H{"error": "upstream session source unavailable"})
			return
		}
		log.Error().Err(err).Str("user_id", userID).Msg("control-session: get failed")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to build control session"})
		return
	}

	c.JSON(http.StatusOK, snap)
}

// postControlSessionRefresh handles POST /api/v1/sessions/refresh.
// G10 Step 3: forces a re-aggregation and publishes
// `app.session.entitlements_changed` so notification-core (and any future
// subscribers) can react. Use after plan upgrades, org switches, or billing
// webhook acks where the cached snapshot is known to be stale.
func (s *Server) postControlSessionRefresh(c *gin.Context) {
	if s.controlSessionService == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "control-session service not configured"})
		return
	}

	userID, ok := userIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	snap, err := s.controlSessionService.Refresh(c.Request.Context(), userID)
	if err != nil {
		if errors.Is(err, service.ErrUserCoreUnavailable) {
			c.JSON(http.StatusBadGateway, gin.H{"error": "upstream session source unavailable"})
			return
		}
		log.Error().Err(err).Str("user_id", userID).Msg("control-session: refresh failed")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to refresh control session"})
		return
	}

	c.JSON(http.StatusOK, snap)
}

// userIDFromContext reads the user id placed in the gin context by the
// authContextMiddleware. Returns ("", false) when missing/blank.
func userIDFromContext(c *gin.Context) (string, bool) {
	v, ok := c.Get("user_id")
	if !ok {
		return "", false
	}
	s, ok := v.(string)
	if !ok || s == "" {
		return "", false
	}
	return s, true
}
