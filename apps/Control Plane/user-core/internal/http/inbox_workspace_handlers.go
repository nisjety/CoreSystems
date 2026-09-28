package http

import (
	"encoding/json"
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// inboxWorkspaceState is deliberately limited to personal operator preferences.
// It does not own, derive, or change the shared conversation/ticket lifecycle.
type inboxWorkspaceState struct {
	PinnedConversationIDs []string `json:"pinnedConversationIds"`
	ReadConversationIDs   []string `json:"readConversationIds"`
}

type updateInboxWorkspacePreferenceRequest struct {
	ConversationID string `json:"conversationId"`
	Enabled        bool   `json:"enabled"`
}

const inboxWorkspacePreferenceLimit = 200

func decodeInboxWorkspaceState(settings map[string]any) inboxWorkspaceState {
	state := inboxWorkspaceState{PinnedConversationIDs: []string{}, ReadConversationIDs: []string{}}
	data, err := json.Marshal(settings)
	if err != nil {
		return state
	}
	_ = json.Unmarshal(data, &state)
	state.PinnedConversationIDs = normalizeConversationIDs(state.PinnedConversationIDs)
	state.ReadConversationIDs = normalizeConversationIDs(state.ReadConversationIDs)
	return state
}

func normalizeConversationIDs(ids []string) []string {
	next := make([]string, 0, min(len(ids), inboxWorkspacePreferenceLimit))
	seen := make(map[string]struct{}, len(ids))
	for _, id := range ids {
		id = trimBounded(id, 200)
		if id == "" {
			continue
		}
		if _, exists := seen[id]; exists {
			continue
		}
		seen[id] = struct{}{}
		next = append(next, id)
		if len(next) == inboxWorkspacePreferenceLimit {
			break
		}
	}
	return next
}

func updateConversationID(ids []string, conversationID string, enabled bool) []string {
	conversationID = trimBounded(conversationID, 200)
	if conversationID == "" {
		return normalizeConversationIDs(ids)
	}
	next := make([]string, 0, len(ids)+1)
	for _, id := range normalizeConversationIDs(ids) {
		if id != conversationID {
			next = append(next, id)
		}
	}
	if enabled {
		next = append([]string{conversationID}, next...)
	}
	return normalizeConversationIDs(next)
}

func (s *Server) inboxWorkspaceState(c *gin.Context) {
	userID, ok := getUserIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "inbox_workspace", map[string]any{
		"pinnedConversationIds": []any{},
		"readConversationIds":   []any{},
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get inbox workspace settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve inbox workspace"})
		return
	}

	c.JSON(http.StatusOK, decodeInboxWorkspaceState(settings))
}

func (s *Server) updateInboxWorkspacePreference(field string) gin.HandlerFunc {
	return func(c *gin.Context) {
		userID, ok := getUserIDFromContext(c)
		if !ok {
			c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
			return
		}

		var req updateInboxWorkspacePreferenceRequest
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
			return
		}
		if strings.TrimSpace(req.ConversationID) == "" {
			c.JSON(http.StatusBadRequest, gin.H{"error": "conversationId is required"})
			return
		}

		settings, err := s.userService.GetSettings(c.Request.Context(), userID, "inbox_workspace", map[string]any{
			"pinnedConversationIds": []any{},
			"readConversationIds":   []any{},
		})
		if err != nil {
			log.Error().Err(err).Str("user_id", userID).Msg("failed to read inbox workspace before update")
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save inbox workspace"})
			return
		}

		state := decodeInboxWorkspaceState(settings)
		switch field {
		case "pin":
			state.PinnedConversationIDs = updateConversationID(state.PinnedConversationIDs, req.ConversationID, req.Enabled)
		case "read":
			state.ReadConversationIDs = updateConversationID(state.ReadConversationIDs, req.ConversationID, req.Enabled)
		default:
			c.JSON(http.StatusInternalServerError, gin.H{"error": "unsupported inbox preference"})
			return
		}

		if _, err := s.userService.UpsertSettings(c.Request.Context(), userID, "inbox_workspace", map[string]any{
			"pinnedConversationIds": state.PinnedConversationIDs,
			"readConversationIds":   state.ReadConversationIDs,
		}); err != nil {
			log.Error().Err(err).Str("user_id", userID).Msg("failed to persist inbox workspace preference")
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save inbox workspace"})
			return
		}

		c.JSON(http.StatusOK, state)
	}
}
