package http

import (
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strconv"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/channels"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/feed"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/preferences"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/subscribers"
	"github.com/gin-gonic/gin"
)

// Handler is the unified Gin handler for notification-core. It owns the
// dispatch path (legacy) plus the new feed / preferences / channels /
// subscribers paths added in U5-2 (ui-ux-velion-gap.md §10).
type Handler struct {
	cfg           *config.Config
	notifications *notification.Service
	feed          *feed.Service
	preferences   *preferences.Service
	channels      *channels.Service
	subscribers   *subscribers.Service
}

// HandlerDeps groups the new services so NewHandler stays append-only.
type HandlerDeps struct {
	Notifications *notification.Service
	Feed          *feed.Service
	Preferences   *preferences.Service
	Channels      *channels.Service
	Subscribers   *subscribers.Service
}

func NewHandler(cfg *config.Config, deps HandlerDeps) *Handler {
	return &Handler{
		cfg:           cfg,
		notifications: deps.Notifications,
		feed:          deps.Feed,
		preferences:   deps.Preferences,
		channels:      deps.Channels,
		subscribers:   deps.Subscribers,
	}
}

// ── Health ────────────────────────────────────────────────────────────────

func (h *Handler) Health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"status":  "ok",
		"service": h.cfg.ServiceName,
	})
}

// ── Dispatch (legacy / V0) ────────────────────────────────────────────────

func (h *Handler) CreateNotificationRequest(c *gin.Context) {
	var request notification.Request

	decoder := json.NewDecoder(c.Request.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		var maxBytesErr *http.MaxBytesError
		if errors.As(err, &maxBytesErr) {
			c.JSON(http.StatusRequestEntityTooLarge, gin.H{"error": "request body too large"})
			return
		}

		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	acceptedRequest, err := h.notifications.Accept(c.Request.Context(), request)
	if err != nil {
		if notification.IsValidationError(err) {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}
		if notification.IsRuntimeDispatchError(err) && acceptedRequest != nil {
			c.JSON(http.StatusBadGateway, acceptedRequest)
			return
		}

		log.Printf("notification-core: failed to accept request: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "internal error"})
		return
	}

	c.JSON(http.StatusAccepted, acceptedRequest)
}

// ── Feed ──────────────────────────────────────────────────────────────────

func (h *Handler) ListFeed(c *gin.Context) {
	if h.feed == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "feed service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}

	page, _ := strconv.Atoi(c.Query("page"))
	limit, _ := strconv.Atoi(c.Query("limit"))
	params := feed.ListParams{
		RecipientID: userID,
		Page:        page,
		Limit:       limit,
	}
	if rs := c.Query("read"); rs != "" {
		switch strings.ToLower(rs) {
		case "true":
			t := true
			params.Read = &t
		case "false":
			f := false
			params.Read = &f
		}
	}
	if c.Query("showArchived") == "true" {
		t := true
		params.Archived = &t
	} else {
		f := false
		params.Archived = &f
	}

	result, err := h.feed.List(c.Request.Context(), params)
	if err != nil {
		log.Printf("notification-core: list feed: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to list notifications"})
		return
	}
	c.JSON(http.StatusOK, result)
}

func (h *Handler) UnreadCount(c *gin.Context) {
	if h.feed == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "feed service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	n, err := h.feed.UnreadCount(c.Request.Context(), userID)
	if err != nil {
		log.Printf("notification-core: unread count: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to count unread"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"count": n})
}

func (h *Handler) UnseenCount(c *gin.Context) {
	if h.feed == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "feed service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	n, err := h.feed.UnseenCount(c.Request.Context(), userID)
	if err != nil {
		log.Printf("notification-core: unseen count: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to count unseen"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"count": n})
}

func (h *Handler) MarkRead(c *gin.Context) {
	if h.feed == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "feed service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	id := c.Param("id")
	if strings.TrimSpace(id) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "id required"})
		return
	}
	n, err := h.feed.MarkRead(c.Request.Context(), userID, id)
	if err != nil {
		if errors.Is(err, feed.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "notification not found"})
			return
		}
		log.Printf("notification-core: mark read: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to mark read"})
		return
	}
	c.JSON(http.StatusOK, n)
}

func (h *Handler) MarkAllRead(c *gin.Context) {
	if h.feed == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "feed service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	n, err := h.feed.MarkAllRead(c.Request.Context(), userID)
	if err != nil {
		log.Printf("notification-core: mark all read: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to mark all read"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"updated": n})
}

// MarkSeen is the per-item "user glanced at this" toggle.
// Implementation reuses MarkRead behaviour (read implies seen) but only
// when called on its dedicated endpoint to keep the contract explicit.
func (h *Handler) MarkSeen(c *gin.Context) {
	if h.feed == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "feed service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	id := c.Param("id")
	if strings.TrimSpace(id) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "id required"})
		return
	}
	// MarkRead also flips seen; we don't have a dedicated single-row
	// MarkSeen because the UI always wants both flags moved together.
	n, err := h.feed.MarkRead(c.Request.Context(), userID, id)
	if err != nil {
		if errors.Is(err, feed.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "notification not found"})
			return
		}
		log.Printf("notification-core: mark seen: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to mark seen"})
		return
	}
	c.JSON(http.StatusOK, n)
}

func (h *Handler) MarkAllSeen(c *gin.Context) {
	if h.feed == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "feed service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	n, err := h.feed.MarkAllSeen(c.Request.Context(), userID)
	if err != nil {
		log.Printf("notification-core: mark all seen: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to mark all seen"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"updated": n})
}

func (h *Handler) DeleteNotification(c *gin.Context) {
	if h.feed == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "feed service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	id := c.Param("id")
	if strings.TrimSpace(id) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "id required"})
		return
	}
	if err := h.feed.Archive(c.Request.Context(), userID, id); err != nil {
		if errors.Is(err, feed.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "notification not found"})
			return
		}
		log.Printf("notification-core: archive: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to delete notification"})
		return
	}
	c.Status(http.StatusNoContent)
}

// ── Preferences ───────────────────────────────────────────────────────────

func (h *Handler) ListPreferences(c *gin.Context) {
	if h.preferences == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "preferences service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	prefs, err := h.preferences.ListForUser(c.Request.Context(), userID)
	if err != nil {
		log.Printf("notification-core: list preferences: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to list preferences"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"preferences": prefs})
}

type setPreferenceBody struct {
	Enabled bool `json:"enabled"`
}

func (h *Handler) SetPreference(c *gin.Context) {
	if h.preferences == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "preferences service not configured"})
		return
	}
	userID := requireUserID(c)
	if userID == "" {
		return
	}
	eventType := strings.TrimSpace(c.Param("eventType"))
	channel := strings.TrimSpace(c.Param("channel"))
	if eventType == "" || channel == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "eventType and channel required"})
		return
	}
	var body setPreferenceBody
	if err := json.NewDecoder(c.Request.Body).Decode(&body); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	pref, err := h.preferences.Put(c.Request.Context(), preferences.PutParams{
		UserID:    userID,
		EventType: eventType,
		Channel:   channel,
		Enabled:   body.Enabled,
	})
	if err != nil {
		log.Printf("notification-core: set preference: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to set preference"})
		return
	}
	c.JSON(http.StatusOK, pref)
}

// ── Channel configs (internal-only) ───────────────────────────────────────

func (h *Handler) ListChannelConfigs(c *gin.Context) {
	if h.channels == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "channels service not configured"})
		return
	}
	orgID := strings.TrimSpace(c.Query("org_id"))
	if orgID == "" {
		orgID = strings.TrimSpace(c.GetHeader("x-org-id"))
	}
	configs, err := h.channels.ListForOrg(c.Request.Context(), orgID)
	if err != nil {
		log.Printf("notification-core: list channel configs: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to list channel configs"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"configs": configs})
}

type patchChannelBody struct {
	Enabled               *bool   `json:"enabled,omitempty"`
	DefaultForSubscribers *bool   `json:"default_for_subscribers,omitempty"`
	Label                 *string `json:"label,omitempty"`
	Description           *string `json:"description,omitempty"`
}

func (h *Handler) PatchChannelConfig(c *gin.Context) {
	if h.channels == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "channels service not configured"})
		return
	}
	orgID := strings.TrimSpace(c.Query("org_id"))
	if orgID == "" {
		orgID = strings.TrimSpace(c.GetHeader("x-org-id"))
	}
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org_id required (query or x-org-id header)"})
		return
	}
	eventType := strings.TrimSpace(c.Param("eventType"))
	channel := strings.TrimSpace(c.Param("channel"))
	if eventType == "" || channel == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "eventType and channel required"})
		return
	}

	var body patchChannelBody
	if err := json.NewDecoder(c.Request.Body).Decode(&body); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	cfg, err := h.channels.Patch(c.Request.Context(), channels.PatchParams{
		OrgID:                 orgID,
		EventType:             eventType,
		Channel:               channel,
		Enabled:               body.Enabled,
		DefaultForSubscribers: body.DefaultForSubscribers,
		Label:                 body.Label,
		Description:           body.Description,
	})
	if err != nil {
		log.Printf("notification-core: patch channel config: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to patch channel config"})
		return
	}
	c.JSON(http.StatusOK, cfg)
}

// ── Subscribers (internal recipient upsert) ───────────────────────────────

type recipientUpsertBody struct {
	UserID    string `json:"user_id"`
	Email     string `json:"email"`
	Name      string `json:"name"`
	FirstName string `json:"first_name"`
	LastName  string `json:"last_name"`
	Phone     string `json:"phone"`
	Avatar    string `json:"avatar"`
	Locale    string `json:"locale"`
	Timezone  string `json:"timezone"`
	OrgID     string `json:"org_id"`
	Role      string `json:"role"`
}

func (h *Handler) UpsertRecipient(c *gin.Context) {
	if h.subscribers == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "subscribers service not configured"})
		return
	}
	var body recipientUpsertBody
	if err := json.NewDecoder(c.Request.Body).Decode(&body); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	if strings.TrimSpace(body.UserID) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "user_id required"})
		return
	}

	first, last := body.FirstName, body.LastName
	// Velion sends a combined `name`; split conservatively.
	if first == "" && last == "" && body.Name != "" {
		parts := strings.SplitN(strings.TrimSpace(body.Name), " ", 2)
		first = parts[0]
		if len(parts) == 2 {
			last = parts[1]
		}
	}

	sub, err := h.subscribers.Upsert(c.Request.Context(), subscribers.UpsertParams{
		UserID:    body.UserID,
		Email:     body.Email,
		Phone:     body.Phone,
		FirstName: first,
		LastName:  last,
		Avatar:    body.Avatar,
		Locale:    body.Locale,
		Timezone:  body.Timezone,
		OrgID:     body.OrgID,
		Role:      body.Role,
	})
	if err != nil {
		log.Printf("notification-core: upsert recipient: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to upsert recipient"})
		return
	}
	c.JSON(http.StatusOK, sub)
}

// ── Helpers ───────────────────────────────────────────────────────────────

// requireUserID extracts and validates the `x-user-id` header. Writes a
// 400 response and returns "" on missing/empty; callers must early-return
// when the result is empty.
func requireUserID(c *gin.Context) string {
	uid := strings.TrimSpace(c.GetHeader("x-user-id"))
	if uid == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "x-user-id header required"})
		return ""
	}
	return uid
}
