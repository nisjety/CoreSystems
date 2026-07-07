package http

import (
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/social"
	"github.com/gin-gonic/gin"
)

type Handler struct {
	cfg     *config.Config
	service *social.Service
}

func NewHandler(cfg *config.Config, service *social.Service) *Handler {
	return &Handler{cfg: cfg, service: service}
}

type createPostBody struct {
	Title            string            `json:"title"`
	Body             string            `json:"body"`
	Platforms        []string          `json:"platforms"`
	Media            []social.MediaRef `json:"media"`
	Source           social.SourceRef  `json:"source"`
	AIContext        map[string]any    `json:"ai_context"`
	ApprovalRequired *bool             `json:"approval_required"`
	ScheduledAt      *time.Time        `json:"scheduled_at"`
}

type createCampaignBody struct {
	Name      string           `json:"name"`
	Brief     string           `json:"brief"`
	Goal      string           `json:"goal"`
	Status    string           `json:"status"`
	Platforms []string         `json:"platforms"`
	StartsAt  *time.Time       `json:"starts_at"`
	EndsAt    *time.Time       `json:"ends_at"`
	Source    social.SourceRef `json:"source"`
	Metadata  map[string]any   `json:"metadata"`
}

type decideApprovalBody struct {
	Decision string `json:"decision"`
	Reason   string `json:"reason"`
}

type schedulePostBody struct {
	ScheduledAt time.Time `json:"scheduled_at"`
}

type enqueuePublishBody struct {
	IdempotencyKey string     `json:"idempotency_key"`
	ScheduledFor   *time.Time `json:"scheduled_for"`
}

type drainPublishBody struct {
	Limit int `json:"limit"`
}

type snapshotMetricsBody struct {
	OrgID string `json:"org_id"`
}

func (h *Handler) Health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"status": "ok", "service": h.cfg.ServiceName})
}

func (h *Handler) ListAccounts(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	accounts, err := h.service.ListAccounts(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": accounts})
}

func (h *Handler) SyncAccounts(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	accounts, err := h.service.ListAccounts(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"accounts": accounts, "synced": len(accounts)}})
}

func (h *Handler) ListPosts(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	limit, _ := strconv.Atoi(c.DefaultQuery("limit", "50"))
	posts, err := h.service.ListPosts(c.Request.Context(), social.ListPostsFilter{
		OrgID:    orgID,
		Status:   c.Query("status"),
		Platform: c.Query("platform"),
		Limit:    limit,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": posts, "meta": gin.H{"limit": limit}})
}

func (h *Handler) ListCampaigns(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	limit, _ := strconv.Atoi(c.DefaultQuery("limit", "50"))
	campaigns, err := h.service.ListCampaigns(c.Request.Context(), social.ListCampaignsFilter{
		OrgID:  orgID,
		Status: c.Query("status"),
		Limit:  limit,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": campaigns, "meta": gin.H{"limit": limit}})
}

func (h *Handler) CreateCampaign(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body createCampaignBody
	if err := decodeJSONBody(c, &body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	campaign, err := h.service.CreateCampaign(c.Request.Context(), social.CreateCampaignInput{
		OrgID:       orgID,
		Name:        body.Name,
		Brief:       body.Brief,
		Goal:        body.Goal,
		Status:      body.Status,
		Platforms:   body.Platforms,
		StartsAt:    body.StartsAt,
		EndsAt:      body.EndsAt,
		Source:      body.Source,
		Metadata:    body.Metadata,
		ActorUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": campaign})
}

func (h *Handler) CreatePost(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body createPostBody
	if err := decodeJSONBody(c, &body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	post, err := h.service.CreatePost(c.Request.Context(), social.CreatePostInput{
		OrgID:            orgID,
		Title:            body.Title,
		Body:             body.Body,
		Platforms:        body.Platforms,
		Media:            body.Media,
		Source:           body.Source,
		AIContext:        body.AIContext,
		ApprovalRequired: body.ApprovalRequired,
		ScheduledAt:      body.ScheduledAt,
		ActorUserID:      actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": post})
}

func (h *Handler) ListApprovals(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	limit, _ := strconv.Atoi(c.DefaultQuery("limit", "50"))
	state := c.Query("state")
	if state == "" {
		state = c.Query("status")
	}
	approvals, err := h.service.ListApprovals(c.Request.Context(), social.ListApprovalsFilter{
		OrgID:      orgID,
		State:      state,
		PostID:     c.Query("post_id"),
		CampaignID: c.Query("campaign_id"),
		Limit:      limit,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": approvals, "meta": gin.H{"limit": limit}})
}

func (h *Handler) DecideApproval(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body decideApprovalBody
	if err := decodeJSONBody(c, &body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	approval, err := h.service.DecideApproval(c.Request.Context(), social.DecideApprovalInput{
		OrgID:          orgID,
		ApprovalID:     c.Param("id"),
		Decision:       body.Decision,
		DecisionReason: body.Reason,
		ActorUserID:    actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": approval})
}

func (h *Handler) SchedulePost(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body schedulePostBody
	if err := decodeJSONBody(c, &body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	result, err := h.service.SchedulePost(c.Request.Context(), social.SchedulePostInput{
		OrgID:       orgID,
		PostID:      c.Param("id"),
		ScheduledAt: body.ScheduledAt,
		ActorUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": result})
}

func (h *Handler) EnqueuePublishJob(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body enqueuePublishBody
	if err := decodeJSONBody(c, &body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	scheduledFor := time.Now().UTC()
	if body.ScheduledFor != nil {
		scheduledFor = body.ScheduledFor.UTC()
	}
	job, err := h.service.EnqueuePublish(c.Request.Context(), social.EnqueuePublishInput{
		OrgID:             orgID,
		PostID:            c.Param("id"),
		IdempotencyKey:    body.IdempotencyKey,
		RequestedByUserID: actorUserID(c),
		ScheduledFor:      scheduledFor,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusAccepted, gin.H{"data": job})
}

func (h *Handler) GetPublishJob(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	job, err := h.service.GetPublishJob(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": job})
}

func (h *Handler) DrainPublishJobs(c *gin.Context) {
	var body drainPublishBody
	_ = decodeJSONBody(c, &body)
	processed, err := h.service.ProcessDuePublishJobs(c.Request.Context(), h.cfg.ServiceName+"-manual-drain", body.Limit)
	if err != nil {
		log.Printf("social-core: manual drain failed: %v", err)
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"processed": processed}})
}

// SnapshotMetrics is the manual trigger for the metrics worker, mirroring
// DrainPublishJobs. Org scope is optional: x-org-id (or org_id in the body)
// snapshots one org; otherwise every org with synced accounts is covered.
func (h *Handler) SnapshotMetrics(c *gin.Context) {
	var body snapshotMetricsBody
	_ = decodeJSONBody(c, &body)
	orgID := strings.TrimSpace(c.GetHeader("x-org-id"))
	if orgID == "" {
		orgID = strings.TrimSpace(body.OrgID)
	}
	summary, err := h.service.SnapshotProviderMetrics(c.Request.Context(), orgID)
	if err != nil {
		log.Printf("social-core: manual metrics snapshot failed: %v", err)
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": summary})
}

// ListMetrics is the read side of SnapshotMetrics: internal-only (same
// requireInternalKey gate as the rest of /api/v1/social), used by
// insight-core to fetch real metric values after a metrics.snapshotted event
// (which intentionally carries only a summary count, not the values).
func (h *Handler) ListMetrics(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	filter := social.ProviderMetricsFilter{
		OrgID:     orgID,
		AccountID: c.Query("accountId"),
	}
	if raw := strings.TrimSpace(c.Query("snapshotDate")); raw != "" {
		parsed, err := time.Parse("2006-01-02", raw)
		if err != nil {
			c.JSON(http.StatusBadRequest, errorPayload("invalid_snapshot_date", "snapshotDate must be YYYY-MM-DD"))
			return
		}
		filter.SnapshotDate = parsed
	}
	metrics, err := h.service.ListProviderMetrics(c.Request.Context(), filter)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": metrics})
}

// ListCatalogs and ListCatalogProducts are the read-only commerce/catalog
// surface (task #29 — see internal/social/catalog.go for the owner
// decision). Meta Commerce Catalog only; Shopify commerce data is a
// conversation-core concern, not implemented here.
func (h *Handler) ListCatalogs(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	catalogs, err := h.service.ListCatalogs(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": catalogs})
}

func (h *Handler) ListCatalogProducts(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	products, err := h.service.ListCatalogProducts(c.Request.Context(), orgID, c.Query("accountId"), c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": products})
}

func decodeJSONBody(c *gin.Context, target any) error {
	decoder := json.NewDecoder(c.Request.Body)
	decoder.DisallowUnknownFields()
	return decoder.Decode(target)
}

func requireOrgID(c *gin.Context) string {
	orgID := strings.TrimSpace(c.GetHeader("x-org-id"))
	if orgID == "" {
		orgID = strings.TrimSpace(c.Query("org_id"))
	}
	if orgID == "" {
		orgID = strings.TrimSpace(c.Query("orgId"))
	}
	if orgID == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_org_id", "x-org-id is required."))
		return ""
	}
	return orgID
}

func actorUserID(c *gin.Context) string {
	userID := strings.TrimSpace(c.GetHeader("x-user-id"))
	if userID == "" {
		return "internal-service"
	}
	return userID
}

func writeServiceError(c *gin.Context, err error) {
	switch {
	case errors.Is(err, social.ErrNotFound):
		c.JSON(http.StatusNotFound, errorPayload("not_found", "Social resource was not found."))
	case social.IsInvalidInput(err):
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", err.Error()))
	default:
		log.Printf("social-core: service error: %v", err)
		c.JSON(http.StatusInternalServerError, errorPayload("internal_error", "Internal error."))
	}
}

func errorPayload(code, message string) gin.H {
	return gin.H{"error": gin.H{"code": code, "message": message}}
}
