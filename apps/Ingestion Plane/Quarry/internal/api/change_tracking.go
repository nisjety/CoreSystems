package api

import (
	"net/http"
	"strings"

	"github.com/gofiber/fiber/v2"
)

func (h *Handler) getLatestChange(c *fiber.Ctx) error {
	if h.changeTracker == nil {
		return writeError(c, http.StatusServiceUnavailable, "change tracker is not initialized", nil)
	}

	key := strings.TrimSpace(c.Query("url"))
	if key == "" {
		collection := strings.TrimSpace(c.Query("collection"))
		if collection != "" {
			key = "collection:" + collection
		}
	}
	if key == "" {
		return writeError(c, http.StatusBadRequest, "Missing required query parameter: 'url' (e.g. ?url=https://example.com) or 'collection' (e.g. ?collection=my-collection)", nil)
	}

	tag := strings.TrimSpace(c.Query("tag"))
	stored, ok := h.changeTracker.GetLatest(c.Context(), key, tag)
	if !ok || stored == nil {
		return writeError(c, http.StatusNotFound, "no tracked snapshot found", nil)
	}

	return c.JSON(fiber.Map{
		"success": true,
		"key":     key,
		"tag":     tag,
		"snapshot": fiber.Map{
			"url":           stored.URL,
			"timestamp":     stored.Timestamp,
			"hash":          stored.Hash,
			"contentLength": len(stored.Content),
		},
	})
}
