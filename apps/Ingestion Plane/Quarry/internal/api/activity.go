package api

import (
	"context"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/platform"
)

func (h *Handler) recordUserActivity(c *fiber.Ctx, action, resource string, details map[string]interface{}) {
	if h == nil || h.controlPlane == nil || c == nil {
		return
	}
	principal := platform.GetPrincipal(c)
	if principal == nil || principal.UserID == "" || principal.UserID == "internal-service" {
		return
	}
	payload := cloneAnyMap(details)
	if h.zdrMode(c) {
		delete(payload, "url")
		delete(payload, "query")
		delete(payload, "requestUrl")
	}
	userID := principal.UserID
	ipAddress := c.IP()
	userAgent := c.Get("User-Agent")

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		if err := h.controlPlane.LogUserActivity(ctx, platform.UserActivityLogParams{
			UserID:    userID,
			Action:    action,
			Resource:  resource,
			Details:   payload,
			IPAddress: ipAddress,
			UserAgent: userAgent,
		}); err != nil {
			log.Warn().Err(err).Str("user_id", userID).Str("action", action).Msg("failed to record user activity")
		}
	}()
}
