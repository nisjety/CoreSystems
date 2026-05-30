package api

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/gofiber/contrib/websocket"
	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/asyncjobs"
	"github.com/triodelab/quarry/internal/batch"
	quarrybrowser "github.com/triodelab/quarry/internal/browser"
	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/platform"
	quarrysearch "github.com/triodelab/quarry/internal/search"
	"github.com/triodelab/quarry/internal/session"
	"github.com/triodelab/quarry/internal/sse"
)

type asyncCreateEnvelope struct {
	Success         bool        `json:"success"`
	ID              string      `json:"id"`
	Resource        string      `json:"resource"`
	Status          string      `json:"status"`
	CreatedAt       time.Time   `json:"createdAt"`
	ExpiresAt       time.Time   `json:"expiresAt"`
	StatusURL       string      `json:"statusUrl"`
	EventsURL       string      `json:"eventsUrl"`
	WebsocketURL    string      `json:"websocketUrl,omitempty"`
	ResolvedOptions interface{} `json:"resolvedOptions,omitempty"`
}

type resourceStatusEnvelope struct {
	Success   bool      `json:"success"`
	ID        string    `json:"id"`
	Resource  string    `json:"resource"`
	Status    string    `json:"status"`
	CreatedAt time.Time `json:"createdAt,omitempty"`
	ExpiresAt time.Time `json:"expiresAt,omitempty"`
	Error     string    `json:"error,omitempty"`
}

type crawlStatusEnvelope struct {
	resourceStatusEnvelope
	Completed int                     `json:"completed"`
	Total     int                     `json:"total"`
	Next      string                  `json:"next,omitempty"`
	Data      []*quarrycrawl.Document `json:"data"`
	Warning   string                  `json:"warning,omitempty"`
}

type searchStatusEnvelope struct {
	resourceStatusEnvelope
	Query     string           `json:"query"`
	Completed int              `json:"completed"`
	Total     int              `json:"total"`
	Count     int              `json:"count"`
	Next      string           `json:"next,omitempty"`
	Data      []V2SearchResult `json:"data"`
}

type extractStatusEnvelope struct {
	resourceStatusEnvelope
	Data map[string]interface{} `json:"data,omitempty"`
}

type batchStatusEnvelope struct {
	resourceStatusEnvelope
	Completed int                        `json:"completed"`
	Failed    int                        `json:"failed"`
	Total     int                        `json:"total"`
	Count     int                        `json:"count"`
	Next      string                     `json:"next,omitempty"`
	Data      []models.BatchScrapeResult `json:"data,omitempty"`
}

type batchErrorsEnvelope struct {
	Success  bool                       `json:"success"`
	ID       string                     `json:"id"`
	Resource string                     `json:"resource"`
	Errors   []models.BatchScrapeResult `json:"errors"`
}

type activeCrawlsEnvelope struct {
	Success bool                  `json:"success"`
	Count   int                   `json:"count"`
	Data    []activeCrawlResource `json:"data"`
}

type crawlJobsEnvelope struct {
	Success bool                  `json:"success"`
	Count   int                   `json:"count"`
	Total   int                   `json:"total"`
	Data    []activeCrawlResource `json:"data"`
}

type activeCrawlResource struct {
	ID        string    `json:"id"`
	URL       string    `json:"url"`
	Status    string    `json:"status"`
	CreatedAt time.Time `json:"createdAt"`
	UpdatedAt time.Time `json:"updatedAt"`
	Completed int       `json:"completed"`
	Total     int       `json:"total"`
	StatusURL string    `json:"statusUrl"`
}

type browserCreateRequest struct {
	URL      string                  `json:"url"`
	Viewport *session.ViewportConfig `json:"viewport,omitempty"`
	Mobile   bool                    `json:"mobile,omitempty"`
	Profile  string                  `json:"profile,omitempty"`
}

type browserSessionEnvelope struct {
	Success   bool                `json:"success"`
	ID        string              `json:"id"`
	Resource  string              `json:"resource"`
	Session   session.SessionInfo `json:"session"`
	StatusURL string              `json:"statusUrl,omitempty"`
}

type browserListEnvelope struct {
	Success bool                  `json:"success"`
	Count   int                   `json:"count"`
	Data    []session.SessionInfo `json:"data"`
}

type teamCreditUsageEnvelope struct {
	Success      bool                   `json:"success"`
	Organization string                 `json:"organizationId"`
	Plan         string                 `json:"plan,omitempty"`
	Credits      int64                  `json:"credits,omitempty"`
	Quota        *platform.QuotaStatus  `json:"quota,omitempty"`
	Entitlements map[string]bool        `json:"entitlements,omitempty"`
	Metadata     map[string]interface{} `json:"metadata,omitempty"`
	Source       string                 `json:"source"`
}

type teamTokenUsageEnvelope struct {
	Success      bool   `json:"success"`
	Organization string `json:"organizationId"`
	TotalTokens  int64  `json:"totalTokens"`
	JobCount     int    `json:"jobCount"`
	Metric       string `json:"metric,omitempty"`
	Source       string `json:"source"`
}

type teamConcurrencyEnvelope struct {
	Success      bool   `json:"success"`
	Organization string `json:"organizationId"`
	Tier         string `json:"tier"`
	Active       int    `json:"active"`
	Limit        int    `json:"limit"`
	Remaining    int    `json:"remaining"`
	Source       string `json:"source"`
}

type teamQueueStatusEnvelope struct {
	Success      bool           `json:"success"`
	Organization string         `json:"organizationId"`
	Counts       map[string]int `json:"counts"`
	Resources    map[string]int `json:"resources"`
}

type teamActivityEnvelope struct {
	Success      bool                `json:"success"`
	Organization string              `json:"organizationId"`
	Count        int                 `json:"count"`
	Data         []teamActivityEvent `json:"data"`
}

type teamActivityEvent struct {
	JobID      string    `json:"jobId"`
	Resource   string    `json:"resource"`
	Status     string    `json:"status"`
	Summary    string    `json:"summary,omitempty"`
	CreatedAt  time.Time `json:"createdAt"`
	UpdatedAt  time.Time `json:"updatedAt"`
	RequestURL string    `json:"requestUrl,omitempty"`
}

func (h *Handler) registerPlatformV1(app *fiber.App) {
	v1 := app.Group("/v1")
	// /v1/crawl → removed; use /v2/crawl (superset of all v1 crawl options).
	// /v1/extract → removed; use /v2/extract (multi-URL, system-prompt, schema validation).
	// /v1/map → removed; use /v2/map.

	v1.Post("/llmstxt", h.v1FeatureGuard("feature.llmstxt"), h.v1AsyncCreateGuard("llmstxt"), h.v1LlmsTxtCreate)
	v1.Get("/llmstxt", h.v1LLMsTxt)
	v1.Get("/llmstxt/full", h.v1LLMsTxtFull)
	v1.Get("/llmstxt/ctx", h.v1LLMsTxtCtx)
	v1.Get("/llmstxt/ctx/full", h.v1LLMsTxtCtxFull)
	v1.Get("/llmstxt/:id", h.v1LlmsTxtStatus)

	v1.Post("/search", h.v1FeatureGuard("feature.search"), h.v1AsyncCreateGuard("search"), h.v1Search)
	v1.Get("/search/:id", h.v1SearchStatus)
	v1.Delete("/search/:id", h.v1CancelSearch)

	v1.Post("/research", h.v1FeatureGuard("feature.research"), h.v1AsyncCreateGuard("research"), h.v1Research)
	v1.Get("/research/:id", h.v1ResearchStatus)
	v1.Delete("/research/:id", h.v1CancelResearch)

	v1.Post("/agent", h.v1FeatureGuard("feature.agent"), h.v1AsyncCreateGuard("agent"), h.v1Agent)
	v1.Get("/agent/:id", h.v1AgentStatus)
	v1.Delete("/agent/:id", h.v1CancelAgent)

	v1.Post("/batch/scrape", h.v1FeatureGuard("feature.batch_scrape"), h.v1AsyncCreateGuard("batch/scrape"), h.v1BatchScrape)
	v1.Get("/batch/scrape/:id/errors", h.v1BatchScrapeErrors)
	v1.Get("/batch/scrape/:id", h.v1BatchScrapeStatus)
	v1.Delete("/batch/scrape/:id", h.v1CancelBatchScrape)

	v1.Post("/browser", h.v1FeatureGuard("feature.browser"), h.v1AsyncCreateGuard("browser"), h.v1BrowserCreate)
	v1.Get("/browser", h.v1FeatureGuard("feature.browser"), h.v1BrowserList)
	v1.Get("/browser/:id", h.v1FeatureGuard("feature.browser"), h.v1BrowserSession)
	v1.Get("/browser/:id/live", h.v1FeatureGuard("feature.browser"), h.v1BrowserLive)
	v1.Post("/browser/:id/execute", h.v1FeatureGuard("feature.browser"), h.v1BrowserExecute)
	v1.Delete("/browser/:id", h.v1FeatureGuard("feature.browser"), h.v1BrowserDelete)

	v1.Get("/team/credit-usage", h.v1TeamCreditUsage)
	v1.Get("/team/credit-usage/historical", h.v1TeamCreditUsageHistorical)
	v1.Get("/team/token-usage", h.v1TeamTokenUsage)
	v1.Get("/team/token-usage/historical", h.v1TeamTokenUsageHistorical)
	v1.Get("/team/concurrency", h.v1TeamConcurrency)
	v1.Get("/team/queue-status", h.v1TeamQueueStatus)
	v1.Get("/team/activity", h.v1TeamActivity)

	v1.Get("/scrape/:id", h.v1BrowserSession)
	v1.Post("/scrape/:id/interact", h.v1BrowserExecute)
	v1.Delete("/scrape/:id/interact", h.v1BrowserDelete)

	v1.Get("/jobs/:id/events", h.streamJob)
	v1.Get("/jobs/:id/ws", h.ensureWebsocketUpgrade, websocket.New(h.streamJobWS))
	v1.Get("/agent/:id/ws", h.ensureWebsocketUpgrade, websocket.New(h.streamJobWS))
}

// registerAgentSignup registers the public (no-auth) onboarding routes used
// by agent self-service email verification.
func (h *Handler) registerAgentSignup(app *fiber.App) {
	app.Post("/v1/agent-signup", h.v1AgentSignup)
	app.Post("/v1/agent-signup/confirm", h.v1AgentSignupConfirm)
}

func (h *Handler) v1AsyncCreateGuard(resource string) fiber.Handler {
	return func(c *fiber.Ctx) error {
		principal := platform.GetPrincipal(c)
		scope := h.asyncCreateScope(resource, principal)
		key := strings.TrimSpace(c.Get("Idempotency-Key"))
		requestHash := h.asyncCreateRequestHash(c)

		if h.idempotencyStore != nil {
			if key == "" {
				return writeError(c, http.StatusBadRequest, "Idempotency-Key header is required", nil)
			}
			record, acquired, err := h.idempotencyStore.Reserve(c.UserContext(), scope, key, requestHash, h.idempotencyTTL())
			if err != nil {
				return writeError(c, http.StatusInternalServerError, "failed to reserve idempotency key", err.Error())
			}
			if record != nil && record.RequestHash != "" && record.RequestHash != requestHash {
				return writeError(c, http.StatusConflict, "idempotency key was already used with a different request payload", nil)
			}
			if !acquired {
				if record != nil && record.State == platform.IdempotencyCompleted {
					if record.ContentType != "" {
						c.Set("Content-Type", record.ContentType)
					}
					c.Set("X-Idempotent-Replayed", "true")
					return c.Status(record.StatusCode).Send(record.Body)
				}
				return writeError(c, http.StatusConflict, "request with the same idempotency key is already in progress", nil)
			}
			defer func() {
				if c.Response().StatusCode() < 200 || c.Response().StatusCode() >= 300 {
					_ = h.idempotencyStore.Release(context.Background(), scope, key)
				}
			}()
		}

		if err := h.enforceAsyncConcurrency(c, principal); err != nil {
			if h.idempotencyStore != nil && key != "" {
				_ = h.idempotencyStore.Release(context.Background(), scope, key)
			}
			return err
		}

		if err := c.Next(); err != nil {
			if h.idempotencyStore != nil && key != "" {
				_ = h.idempotencyStore.Release(context.Background(), scope, key)
			}
			return err
		}

		if h.idempotencyStore != nil && key != "" {
			_, err := h.idempotencyStore.Commit(
				context.Background(),
				scope,
				key,
				c.Response().StatusCode(),
				string(c.Response().Header.ContentType()),
				append([]byte(nil), c.Response().Body()...),
				h.idempotencyTTL(),
			)
			if err != nil {
				return writeError(c, http.StatusInternalServerError, "failed to persist idempotent response", err.Error())
			}
		}
		return nil
	}
}

func (h *Handler) v1FeatureGuard(feature string) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if err := platform.RequireEntitlement(feature, platform.GetPrincipal(c), h != nil && h.controlPlane != nil); err != nil {
			return writeError(c, http.StatusForbidden, err.Error(), map[string]interface{}{
				"feature": feature,
			})
		}
		return c.Next()
	}
}

func (h *Handler) asyncCreateRequestHash(c *fiber.Ctx) string {
	sum := sha256.Sum256(append([]byte(c.Path()+"\n"), c.Body()...))
	return fmt.Sprintf("%x", sum[:])
}

func (h *Handler) asyncCreateScope(resource string, principal *platform.Principal) string {
	orgID := "internal"
	if principal != nil && strings.TrimSpace(principal.OrganizationID) != "" {
		orgID = principal.OrganizationID
	}
	return orgID + ":" + resource
}

func (h *Handler) idempotencyTTL() time.Duration {
	if h == nil || h.cfg == nil || h.cfg.IdempotencyTTLHours <= 0 {
		return 24 * time.Hour
	}
	return time.Duration(h.cfg.IdempotencyTTLHours) * time.Hour
}

func (h *Handler) enforceAsyncConcurrency(c *fiber.Ctx, principal *platform.Principal) error {
	if principal == nil || principal.OrganizationID == "" || principal.Tier == "internal" {
		return nil
	}
	limit := h.resolveConcurrencyLimit(c.UserContext(), principal)
	if limit <= 0 {
		return nil
	}
	active := h.countActiveJobsForOrg(principal.OrganizationID)
	if active < limit {
		return nil
	}
	return writeError(c, http.StatusTooManyRequests, "concurrency limit exceeded", map[string]interface{}{
		"limit":     limit,
		"active":    active,
		"remaining": 0,
	})
}

func (h *Handler) currentOrgAndUser(c *fiber.Ctx) (string, string, string) {
	principal := platform.GetPrincipal(c)
	if principal == nil {
		return "internal", "internal-service", "free"
	}
	orgID := strings.TrimSpace(principal.OrganizationID)
	if orgID == "" {
		orgID = "internal"
	}
	userID := strings.TrimSpace(principal.UserID)
	if userID == "" {
		userID = "internal-service"
	}
	tier := strings.TrimSpace(principal.Tier)
	if tier == "" {
		tier = "free"
	}
	return orgID, userID, tier
}

func (h *Handler) zdrMode(c *fiber.Ctx) bool {
	principal := platform.GetPrincipal(c)
	return principal != nil && principal.ZDRMode
}

func (h *Handler) redactStoredString(c *fiber.Ctx, value string) string {
	if h.zdrMode(c) {
		return ""
	}
	return value
}

func (h *Handler) countActiveJobsForOrg(orgID string) int {
	if h == nil || h.jobStore == nil || strings.TrimSpace(orgID) == "" {
		return 0
	}
	count := 0
	for _, job := range h.jobStore.List() {
		if job == nil {
			continue
		}
		if strings.TrimSpace(job.Meta["org_id"]) != orgID {
			continue
		}
		if job.Status == jobs.StatusPending || job.Status == jobs.StatusRunning {
			count++
		}
	}
	return count
}

func (h *Handler) resolveConcurrencyLimit(ctx context.Context, principal *platform.Principal) int {
	if principal == nil {
		return 0
	}
	if value := principal.MetadataInt("quarry_concurrency_limit", "concurrency_limit"); value > 0 {
		return value
	}
	if value := principal.QuotaInt("concurrency", "async_jobs"); value > 0 {
		return value
	}
	limit := defaultConcurrencyLimit(principal.Tier)
	if h == nil || h.controlPlane == nil || strings.TrimSpace(principal.OrganizationID) == "" {
		return limit
	}
	account, err := h.controlPlane.GetAccount(ctx, principal.OrganizationID)
	if err != nil || account == nil {
		return limit
	}
	if value := intFromAny(account.Metadata["quarry_concurrency_limit"]); value > 0 {
		return value
	}
	if value := intFromAny(account.Metadata["concurrency_limit"]); value > 0 {
		return value
	}
	if value := int(account.QuotaLimits["concurrency"]); value > 0 {
		return value
	}
	if value := int(account.QuotaLimits["async_jobs"]); value > 0 {
		return value
	}
	return limit
}

func defaultConcurrencyLimit(tier string) int {
	switch strings.ToLower(strings.TrimSpace(tier)) {
	case "starter":
		return 5
	case "pro":
		return 10
	case "enterprise":
		return 25
	case "internal":
		return 100
	default:
		return 2
	}
}

func (h *Handler) v1Crawl(c *fiber.Ctx) error {
	if h.jobStore == nil || h.crawlStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl executor is not initialized", nil)
	}
	orgID, userID, tier := h.currentOrgAndUser(c)

	normalized, err := mergeTopLevelFetchAliases(c.Body())
	if err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	c.Request().SetBodyRaw(normalized)
	present := fieldPresence(normalized)

	req, spec, err := h.parseV2CrawlRequest(c, false)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	spec, resolvedOptions, err := resolveV1CrawlSpec(req, spec, present)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	spec, err = h.applyPromptGeneratedCrawlOptions(c.UserContext(), normalized, req, spec)
	if err != nil {
		return writeError(c, http.StatusBadGateway, "failed to generate crawl plan", err.Error())
	}
	spec.ZDRMode = h.zdrMode(c)
	resolvedOptions = crawlResolvedOptions(spec)

	meta := map[string]string{
		"kind":         "crawl",
		"url":          h.redactStoredString(c, spec.URL),
		"preset":       spec.Preset,
		"module":       spec.Module,
		"mode":         "async",
		"api_version":  "v1",
		"crawl_style":  "recursive",
		"result_store": "crawl",
		"org_id":       orgID,
		"user_id":      userID,
		"tier":         tier,
		"zdr_mode":     fmt.Sprintf("%t", h.zdrMode(c)),
	}
	job := h.jobStore.New(meta)
	_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
		current.Status = jobs.StatusPending
	})

	storedSpec := spec
	storedURL := spec.URL
	if spec.ZDRMode {
		storedSpec.URL = ""
		storedSpec.Prompt = ""
		storedSpec.PageOptions.Headers = nil
		storedSpec.PageOptions.Actions = nil
		storedSpec.PageOptions.ProxyURL = ""
		if storedSpec.ChangeTracking != nil {
			cloned := *storedSpec.ChangeTracking
			cloned.Schema = nil
			cloned.Prompt = ""
			storedSpec.ChangeTracking = &cloned
		}
		storedURL = ""
	}
	run := &quarrycrawl.Run{
		ID:        job.ID,
		URL:       storedURL,
		Status:    quarrycrawl.StatusQueued,
		CreatedAt: job.CreatedAt,
		UpdatedAt: job.UpdatedAt,
		ExpiresAt: job.ExpiresAt,
		Spec:      storedSpec,
	}
	if err := h.crawlStore.CreateRun(context.Background(), run); err != nil {
		_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusFailed
			current.Error = err.Error()
		})
		return writeError(c, http.StatusInternalServerError, "failed to create crawl run", err.Error())
	}

	if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindCrawl, job.ID, asyncjobs.CrawlPayload{
		Spec:    spec,
		Webhook: req.Webhook,
		OrgID:   orgID,
		UserID:  userID,
	}, "v1"); err != nil {
		_ = h.crawlStore.SetRun(context.Background(), &quarrycrawl.Run{
			ID:        run.ID,
			URL:       run.URL,
			Status:    quarrycrawl.StatusFailed,
			CreatedAt: run.CreatedAt,
			UpdatedAt: time.Now().UTC(),
			ExpiresAt: run.ExpiresAt,
			Spec:      run.Spec,
		})
		markJobDispatchFailure(h.jobStore, job.ID, err)
		return writeError(c, http.StatusBadGateway, "failed to queue crawl job", err.Error())
	}

	h.recordUserActivity(c, "crawl.created", "crawl", map[string]interface{}{
		"jobId":   job.ID,
		"summary": "crawl queued",
		"url":     h.redactStoredString(c, spec.URL),
		"preset":  spec.Preset,
	})

	return c.JSON(h.newAsyncCreateEnvelope(c, "crawl", job.ID, job.CreatedAt, job.ExpiresAt, "queued", resolvedOptions))
}

func (h *Handler) v1CrawlStatus(c *fiber.Ctx) error {
	runID := strings.TrimSpace(c.Params("id"))
	if runID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	run, err := h.crawlStore.GetRun(c.UserContext(), runID)
	if err != nil {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	skip := max(c.QueryInt("skip", 0), 0)
	limit := c.QueryInt("limit", 100)
	if limit <= 0 {
		limit = 100
	}
	data, totalDocs, err := h.crawlStore.ListDocuments(c.UserContext(), runID, skip, limit)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to list crawl documents", err.Error())
	}

	next := ""
	if skip+len(data) < totalDocs || run.Status == quarrycrawl.StatusQueued || run.Status == quarrycrawl.StatusRunning {
		next = buildResourceNextURL(c, "crawl", runID, skip, limit, totalDocs, len(data))
	}

	return c.JSON(crawlStatusEnvelope{
		resourceStatusEnvelope: resourceStatusEnvelope{
			Success:   run.Status != quarrycrawl.StatusFailed,
			ID:        run.ID,
			Resource:  "crawl",
			Status:    mapRunStatus(run.Status),
			CreatedAt: run.CreatedAt,
			ExpiresAt: run.ExpiresAt,
			Error:     mapCrawlError(run.Status),
		},
		Completed: run.Completed,
		Total:     run.Total,
		Next:      next,
		Data:      data,
		Warning:   run.Warning,
	})
}

func (h *Handler) v1CrawlErrors(c *fiber.Ctx) error {
	return h.v2CrawlErrors(c)
}

func (h *Handler) v1CancelCrawl(c *fiber.Ctx) error {
	return h.v2CancelCrawl(c)
}

func (h *Handler) v1CrawlParamsPreview(c *fiber.Ctx) error {
	normalized, err := mergeTopLevelFetchAliases(c.Body())
	if err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	c.Request().SetBodyRaw(normalized)
	return h.v2CrawlParamsPreview(c)
}

func (h *Handler) v1Map(c *fiber.Ctx) error {
	normalized, err := mergeTopLevelFetchAliases(c.Body())
	if err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	c.Request().SetBodyRaw(normalized)
	return h.v2Map(c)
}

func (h *Handler) v1CrawlActive(c *fiber.Ctx) error {
	if h.jobStore == nil || h.crawlStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl store is not initialized", nil)
	}
	jobsList := h.jobStore.List()
	items := make([]activeCrawlResource, 0, len(jobsList))
	for _, job := range jobsList {
		if job == nil || job.Meta["kind"] != "crawl" {
			continue
		}
		if job.Status != jobs.StatusPending && job.Status != jobs.StatusRunning {
			continue
		}
		item, ok := h.crawlResourceFromJob(c, job)
		if !ok {
			continue
		}
		items = append(items, item)
	}
	return c.JSON(activeCrawlsEnvelope{Success: true, Count: len(items), Data: items})
}

func (h *Handler) v1CrawlJobs(c *fiber.Ctx) error {
	if h.jobStore == nil || h.crawlStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl store is not initialized", nil)
	}

	limit := c.QueryInt("limit", 20)
	if limit <= 0 {
		limit = 20
	}
	if limit > 100 {
		limit = 100
	}
	statusFilter := strings.TrimSpace(strings.ToLower(c.Query("status")))

	jobsList, err := jobs.ListSortedJobs(c.UserContext(), h.jobStore, 1000)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to list crawl jobs", err.Error())
	}

	items := make([]activeCrawlResource, 0, min(limit, len(jobsList)))
	total := 0
	for _, job := range jobsList {
		if job == nil || job.Meta["kind"] != "crawl" {
			continue
		}
		item, ok := h.crawlResourceFromJob(c, job)
		if !ok {
			continue
		}
		if statusFilter != "" && strings.ToLower(item.Status) != statusFilter {
			continue
		}
		total++
		if len(items) == limit {
			continue
		}
		items = append(items, item)
	}

	return c.JSON(crawlJobsEnvelope{Success: true, Count: len(items), Total: total, Data: items})
}

func (h *Handler) crawlResourceFromJob(c *fiber.Ctx, job *jobs.Job) (activeCrawlResource, bool) {
	if job == nil {
		return activeCrawlResource{}, false
	}

	run, err := h.crawlStore.GetRun(c.UserContext(), job.ID)
	if err == nil {
		return activeCrawlResource{
			ID:        run.ID,
			URL:       h.redactStoredString(c, run.URL),
			Status:    mapRunStatus(run.Status),
			CreatedAt: run.CreatedAt,
			UpdatedAt: run.UpdatedAt,
			Completed: run.Completed,
			Total:     run.Total,
			StatusURL: buildAbsoluteResourceURL(c, "/v1/crawl/"+run.ID),
		}, true
	}

	return activeCrawlResource{
		ID:        job.ID,
		URL:       h.redactStoredString(c, job.Meta["url"]),
		Status:    mapJobStatus(job.Status),
		CreatedAt: job.CreatedAt,
		UpdatedAt: job.UpdatedAt,
		Completed: 0,
		Total:     0,
		StatusURL: buildAbsoluteResourceURL(c, "/v1/crawl/"+job.ID),
	}, true
}

func mapJobStatus(status jobs.Status) string {
	switch status {
	case jobs.StatusReady:
		return "completed"
	case jobs.StatusFailed:
		return "failed"
	case jobs.StatusCancelled:
		return "cancelled"
	case jobs.StatusPending, jobs.StatusRunning:
		return "scraping"
	default:
		return "scraping"
	}
}

func (h *Handler) v1Search(c *fiber.Ctx) error {
	if h.jobStore == nil || h.searchAsyncStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "search async store is not initialized", nil)
	}
	orgID, userID, tier := h.currentOrgAndUser(c)
	normalized, err := mergeTopLevelFetchAliases(c.Body())
	if err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	c.Request().SetBodyRaw(normalized)

	var req V2SearchRequest
	if err := json.Unmarshal(normalized, &req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}
	req.Preset = normalizeSearchPresetName(req.Preset)
	if strings.TrimSpace(req.Query) == "" {
		return writeError(c, http.StatusBadRequest, "query is required", nil)
	}
	req.BlendMode = normalizeBlendMode(req.BlendMode)
	if req.BlendMode == "" {
		return writeError(c, http.StatusBadRequest, "blendMode contains unsupported value", nil)
	}
	if req.Limit <= 0 {
		req.Limit = 10
	}
	if req.Limit > 50 {
		req.Limit = 50
	}
	if req.TimeoutSec <= 0 {
		req.TimeoutSec = 15
	}
	if req.TimeoutSec > 300 {
		req.TimeoutSec = 300
	}
	if req.Webhook != nil && strings.TrimSpace(req.Webhook.URL) != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
	}
	present := fieldPresence(normalized)

	sources, err := parseV2SearchSources(req.Sources)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	scrapeOpts, shouldScrape, err := buildV2SearchScrapeOptions(req)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if scrapeOpts != nil && req.ScrapeOptions != nil {
		scrapeOpts.ProxyURL = h.resolveProxyURL(c, req.ScrapeOptions.Proxy)
	}
	cfg, resolvedOptions, err := resolveV1SearchConfig(req, present, sources, scrapeOpts, shouldScrape)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if err := h.ensureSearchSourcesAvailable(cfg.Sources); err != nil {
		return writeError(c, http.StatusServiceUnavailable, err.Error(), nil)
	}

	job := h.jobStore.New(map[string]string{
		"kind":        "search",
		"query":       h.redactStoredString(c, req.Query),
		"preset":      cfg.Preset,
		"api_version": "v1",
		"org_id":      orgID,
		"user_id":     userID,
		"tier":        tier,
		"zdr_mode":    fmt.Sprintf("%t", h.zdrMode(c)),
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(job.ID, sse.EventJobCreated, map[string]any{
			"jobId":    job.ID,
			"resource": "search",
			"query":    req.Query,
		})
	}
	if err := h.searchAsyncStore.CreateRun(c.UserContext(), &quarrysearch.AsyncRun{
		ID:        job.ID,
		Query:     h.redactStoredString(c, req.Query),
		CreatedAt: job.CreatedAt,
		UpdatedAt: job.UpdatedAt,
		ExpiresAt: job.ExpiresAt,
	}); err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to initialize search job", err.Error())
	}
	_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
		current.Status = jobs.StatusPending
		current.Result = map[string]any{
			"preset": cfg.Preset,
			"query":  h.redactStoredString(c, req.Query),
		}
	})
	if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindSearch, job.ID, asyncjobs.SearchPayload{
		Preset:       cfg.Preset,
		OrgID:        orgID,
		BlendMode:    cfg.BlendMode,
		Query:        cfg.Query,
		Limit:        cfg.Limit,
		Sources:      toAsyncSearchSources(cfg.Sources),
		ScrapeOpts:   cfg.ScrapeOpts,
		ShouldScrape: cfg.ShouldScrape,
		TimeoutSec:   cfg.TimeoutSec,
		Webhook:      cfg.Webhook,
	}, "v1"); err != nil {
		markJobDispatchFailure(h.jobStore, job.ID, err)
		return writeError(c, http.StatusBadGateway, "failed to queue search job", err.Error())
	}

	h.recordUserActivity(c, "search.created", "search", map[string]interface{}{
		"jobId":   job.ID,
		"summary": "search queued",
		"query":   h.redactStoredString(c, req.Query),
	})

	return c.JSON(h.newAsyncCreateEnvelope(c, "search", job.ID, job.CreatedAt, job.ExpiresAt, "queued", resolvedOptions))
}

func (h *Handler) v1SearchStatus(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "search" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}
	run, err := h.searchAsyncStore.GetRun(c.UserContext(), jobID)
	if err != nil {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}
	skip := max(c.QueryInt("skip", 0), 0)
	limit := c.QueryInt("limit", 0)
	results, totalResults, err := h.searchAsyncStore.ListResults(c.UserContext(), jobID, skip, limit)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to retrieve search results", err.Error())
	}
	data := fromStoredSearchResults(results)
	return c.JSON(searchStatusEnvelope{
		resourceStatusEnvelope: resourceStatusEnvelope{
			Success:   true,
			ID:        job.ID,
			Resource:  "search",
			Status:    mapV2SearchJobStatus(job.Status),
			CreatedAt: job.CreatedAt,
			ExpiresAt: run.ExpiresAt,
			Error:     job.Error,
		},
		Query:     stringFromAny(job.Result["query"], job.Meta["query"]),
		Completed: max(intFromAny(job.Result["completed"]), run.Completed),
		Total:     max(intFromAny(job.Result["total"]), max(run.Total, totalResults)),
		Count:     len(data),
		Next:      buildAbsoluteSearchNextURL(c, jobID, skip, limit, totalResults, len(data)),
		Data:      data,
	})
}

func (h *Handler) v1CancelSearch(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "search" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}
	if cancelValue, ok := h.activeSearchCancels.Load(jobID); ok {
		if cancelFn, ok := cancelValue.(context.CancelFunc); ok {
			cancelFn()
		}
	}
	h.publishAsyncCancel(c.UserContext(), asyncjobs.KindSearch, jobID)
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusCancelled
		current.Error = "cancelled by user"
	})
	h.recordUserActivity(c, "search.cancelled", "search", map[string]interface{}{
		"jobId":   jobID,
		"summary": "search cancelled",
	})
	return c.JSON(fiber.Map{
		"success": true,
		"id":      jobID,
		"status":  "cancelled",
	})
}

func (h *Handler) v1Extract(c *fiber.Ctx) error {
	orgID, userID, tier := h.currentOrgAndUser(c)
	normalized, err := mergeTopLevelFetchAliases(c.Body())
	if err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}
	c.Request().SetBodyRaw(normalized)

	var req V2ExtractRequest
	if err := json.Unmarshal(normalized, &req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}

	req.Preset = normalizeExtractPresetName(req.Preset)
	req.Prompt = strings.TrimSpace(req.Prompt)
	req.SystemPrompt = strings.TrimSpace(req.SystemPrompt)
	if req.Webhook != nil && strings.TrimSpace(req.Webhook.URL) != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
	}

	schema, err := normalizeOptionalSchema(req.Schema)
	if err != nil {
		return writeError(c, http.StatusBadRequest, "schema must be valid JSON or a JSON-encoded string", nil)
	}
	if len(req.URLs) == 0 && req.Prompt == "" {
		return writeError(c, http.StatusBadRequest, "urls or prompt is required", nil)
	}
	present := fieldPresence(normalized)

	if req.Limit <= 0 {
		req.Limit = 5
	}
	if req.Limit > 50 {
		req.Limit = 50
	}
	if req.TimeoutSec <= 0 {
		req.TimeoutSec = 60
	}
	if req.TimeoutSec > 300 {
		req.TimeoutSec = 300
	}

	scrapeOpts, err := buildV2ExtractScrapeOptions(req.ScrapeOptions)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if scrapeOpts != nil && req.ScrapeOptions != nil {
		scrapeOpts.ProxyURL = h.resolveProxyURL(c, req.ScrapeOptions.Proxy)
	}
	req, scrapeOpts, resolvedOptions, err := resolveV1ExtractRequest(req, present, scrapeOpts)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}

	resolvedURLs, invalidURLs, err := h.resolveV2ExtractURLs(c.UserContext(), req)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if req.EnableWebSearch && len(resolvedURLs) == 0 && req.Prompt != "" && h.searchClient != nil {
		searchCtx, searchCancel := context.WithTimeout(c.UserContext(), 10*time.Second)
		defer searchCancel()
		results, searchErr := h.searchClient.Search(searchCtx, quarrysearch.SearchTypeWeb, quarrysearch.SearchOptions{
			Query: req.Prompt,
			Limit: req.Limit,
		})
		if searchErr == nil {
			searchURLs := make([]string, 0, len(results))
			for _, result := range results {
				searchURLs = append(searchURLs, result.URL)
			}
			resolvedURLs, _, err = normalizeExtractURLs(searchURLs, true)
			if err != nil {
				return writeError(c, http.StatusBadRequest, err.Error(), nil)
			}
		}
	}

	if len(resolvedURLs) == 0 {
		if len(invalidURLs) > 0 {
			return writeError(c, http.StatusBadRequest, "no valid URLs resolved", map[string]interface{}{"invalidURLs": invalidURLs})
		}
		return writeError(c, http.StatusBadRequest, "no URLs resolved (provide urls or enable web search with a prompt)", nil)
	}
	if len(resolvedURLs) > req.Limit {
		resolvedURLs = resolvedURLs[:req.Limit]
	}
	if h.extractionJobStore == nil || h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "extraction store is not initialized", nil)
	}

	job := h.jobStore.New(map[string]string{
		"kind":        "extract",
		"api_version": "v1",
		"url_count":   fmt.Sprintf("%d", len(resolvedURLs)),
		"preset":      req.Preset,
		"org_id":      orgID,
		"user_id":     userID,
		"tier":        tier,
		"zdr_mode":    fmt.Sprintf("%t", h.zdrMode(c)),
	})
	extractionJob := &jobs.ExtractionJob{
		ID:     job.ID,
		URL:    strings.Join(resolvedURLs, ","),
		Schema: schema,
		Prompt: req.Prompt,
		Status: jobs.ExtractionQueued,
		Result: map[string]any{
			"preset": req.Preset,
		},
	}
	if h.zdrMode(c) {
		extractionJob = sanitizeExtractionJobForZDR(extractionJob)
	}
	if err := h.extractionJobStore.Create(c.UserContext(), extractionJob); err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to queue extraction job", err.Error())
	}
	_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
		urlTrace := append([]string(nil), resolvedURLs...)
		invalid := append([]string(nil), invalidURLs...)
		if h.zdrMode(c) {
			urlTrace = nil
			invalid = nil
		}
		current.Status = jobs.StatusPending
		current.Result = map[string]any{
			"preset":      req.Preset,
			"urlTrace":    urlTrace,
			"invalidURLs": invalid,
			"completed":   0,
			"total":       len(resolvedURLs),
		}
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(job.ID, sse.EventJobCreated, map[string]any{
			"jobId":       job.ID,
			"resource":    "extract",
			"urlTrace":    resolvedURLs,
			"invalidURLs": invalidURLs,
		})
	}

	if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindExtract, job.ID, asyncjobs.ExtractPayload{
		Preset:       req.Preset,
		OrgID:        orgID,
		UserID:       userID,
		Schema:       schema,
		Prompt:       req.Prompt,
		SystemPrompt: req.SystemPrompt,
		TimeoutSec:   req.TimeoutSec,
		URLTrace:     append([]string(nil), resolvedURLs...),
		Webhook:      req.Webhook,
		ScrapeFormat: scrapeOpts,
	}, "v1"); err != nil {
		markJobDispatchFailure(h.jobStore, job.ID, err)
		return writeError(c, http.StatusBadGateway, "failed to queue extraction job", err.Error())
	}

	h.recordUserActivity(c, "extract.created", "extract", map[string]interface{}{
		"jobId":   job.ID,
		"summary": "extract queued",
	})

	resolvedOptions["urlTrace"] = append([]string(nil), resolvedURLs...)
	resolvedOptions["invalidURLs"] = append([]string(nil), invalidURLs...)
	return c.JSON(h.newAsyncCreateEnvelope(c, "extract", job.ID, job.CreatedAt, job.ExpiresAt, "queued", resolvedOptions))
}

func (h *Handler) v1ExtractStatus(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.extractionJobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "extraction store not initialized", nil)
	}
	job, err := h.extractionJobStore.Get(c.UserContext(), jobID)
	if err != nil {
		return writeError(c, http.StatusNotFound, "extraction job not found", nil)
	}
	return c.JSON(extractStatusEnvelope{
		resourceStatusEnvelope: resourceStatusEnvelope{
			Success:   job.Status != jobs.ExtractionFailed,
			ID:        jobID,
			Resource:  "extract",
			Status:    string(job.Status),
			CreatedAt: job.CreatedAt,
			ExpiresAt: job.ExpiresAt,
			Error:     job.Error,
		},
		Data: job.Result,
	})
}

func (h *Handler) v1BatchScrape(c *fiber.Ctx) error {
	if h.batchManager == nil || h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "batch manager is not initialized", nil)
	}
	orgID, userID, tier := h.currentOrgAndUser(c)
	var req models.BatchScrapeRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	if len(req.URLs) == 0 {
		return writeError(c, http.StatusBadRequest, "urls is required", nil)
	}
	for _, targetURL := range req.URLs {
		if err := validateAbsoluteHTTPURL(targetURL); err != nil {
			return writeError(c, http.StatusBadRequest, "urls contains invalid value", err.Error())
		}
	}
	if req.Webhook != nil && req.Webhook.URL != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
	}

	job := h.batchManager.CreateJob(&req)
	h.jobStore.Upsert(&jobs.Job{
		ID:        job.ID,
		Status:    jobs.StatusPending,
		CreatedAt: job.CreatedAt,
		ExpiresAt: job.ExpiresAt,
		Meta: map[string]string{
			"kind":        "batch_scrape",
			"api_version": "v1",
			"org_id":      orgID,
			"user_id":     userID,
			"tier":        tier,
			"url_count":   fmt.Sprintf("%d", len(req.URLs)),
			"zdr_mode":    fmt.Sprintf("%t", h.zdrMode(c)),
		},
		Result: map[string]any{},
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(job.ID, sse.EventJobCreated, map[string]any{
			"jobId":    job.ID,
			"resource": "batch/scrape",
			"total":    len(req.URLs),
		})
	}
	if err := h.batchManager.StartJob(context.Background(), job.ID); err != nil {
		return writeError(c, http.StatusBadGateway, "failed to start batch job", err.Error())
	}
	if h.streamManager != nil {
		h.streamManager.Broadcast(job.ID, sse.EventJobStarted, map[string]any{"progress": 0})
	}
	go h.mirrorBatchJob(job.ID)

	h.recordUserActivity(c, "batch_scrape.created", "batch/scrape", map[string]interface{}{
		"jobId":   job.ID,
		"summary": "batch scrape queued",
	})

	return c.JSON(h.newAsyncCreateEnvelope(c, "batch/scrape", job.ID, job.CreatedAt, job.ExpiresAt, "queued", map[string]interface{}{
		"urls": len(req.URLs),
	}))
}

func (h *Handler) v1BatchScrapeStatus(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "batch id is required", nil)
	}
	batchStatus := h.batchManager.GetJobStatus(jobID)
	if batchStatus == nil {
		return writeError(c, http.StatusNotFound, "batch job not found", nil)
	}
	skip := max(c.QueryInt("skip", 0), 0)
	limit := c.QueryInt("limit", 100)
	if limit <= 0 {
		limit = 100
	}
	data, total := paginateBatchResults(batchStatus.Data, skip, limit)
	statusJob, _ := h.jobStore.Get(jobID)
	createdAt := batchStatus.CreatedAt
	errorMessage := batchStatus.Error
	if statusJob != nil {
		createdAt = statusJob.CreatedAt
		if errorMessage == "" {
			errorMessage = statusJob.Error
		}
	}
	return c.JSON(batchStatusEnvelope{
		resourceStatusEnvelope: resourceStatusEnvelope{
			Success:   batchStatus.Status != "failed",
			ID:        jobID,
			Resource:  "batch/scrape",
			Status:    batchStatus.Status,
			CreatedAt: createdAt,
			ExpiresAt: batchStatus.ExpiresAt,
			Error:     errorMessage,
		},
		Completed: batchStatus.Completed,
		Failed:    batchStatus.Failed,
		Total:     batchStatus.Total,
		Count:     len(data),
		Next:      buildResourceNextURL(c, "batch/scrape", jobID, skip, limit, total, len(data)),
		Data:      data,
	})
}

func (h *Handler) v1BatchScrapeErrors(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "batch id is required", nil)
	}
	batchStatus := h.batchManager.GetJobStatus(jobID)
	if batchStatus == nil {
		return writeError(c, http.StatusNotFound, "batch job not found", nil)
	}
	errorsOnly := make([]models.BatchScrapeResult, 0)
	for _, item := range batchStatus.Data {
		if !item.Success {
			errorsOnly = append(errorsOnly, item)
		}
	}
	return c.JSON(batchErrorsEnvelope{
		Success:  true,
		ID:       jobID,
		Resource: "batch/scrape",
		Errors:   errorsOnly,
	})
}

func (h *Handler) v1CancelBatchScrape(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "batch id is required", nil)
	}
	if err := h.batchManager.CancelJob(jobID); err != nil {
		if err == batch.ErrJobNotFound {
			return writeError(c, http.StatusNotFound, "batch job not found", nil)
		}
		return writeError(c, http.StatusBadGateway, "failed to cancel batch job", err.Error())
	}
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusCancelled
		current.Error = "cancelled by user"
	})
	h.recordUserActivity(c, "batch_scrape.cancelled", "batch/scrape", map[string]interface{}{
		"jobId":   jobID,
		"summary": "batch scrape cancelled",
	})
	return c.JSON(fiber.Map{"success": true, "id": jobID, "status": "cancelled"})
}

func (h *Handler) v1BrowserCreate(c *fiber.Ctx) error {
	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}
	var req browserCreateRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}
	req.URL = strings.TrimSpace(req.URL)
	if err := validateAbsoluteHTTPURL(req.URL); err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	ctx, cancel := context.WithTimeout(c.UserContext(), 30*time.Second)
	defer cancel()
	resp, err := h.browserRuntime.Create(ctx, quarrybrowser.CreateRequest{
		URL:      req.URL,
		Viewport: req.Viewport,
		Mobile:   req.Mobile,
		Profile:  strings.TrimSpace(req.Profile),
	})
	if err != nil {
		return writeError(c, http.StatusInternalServerError, fmt.Sprintf("session creation failed: %v", err), nil)
	}
	state := resp.State
	return c.Status(http.StatusCreated).JSON(browserSessionEnvelope{
		Success:   true,
		ID:        state.Session.ID,
		Resource:  "browser",
		Session:   sessionInfoFromState(&state),
		StatusURL: buildAbsoluteResourceURL(c, "/v1/browser/"+state.Session.ID),
	})
}

func (h *Handler) v1BrowserList(c *fiber.Ctx) error {
	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}
	sessions, err := h.browserRuntime.List(c.UserContext())
	if err != nil {
		return writeError(c, http.StatusServiceUnavailable, err.Error(), nil)
	}
	return c.JSON(browserListEnvelope{Success: true, Count: len(sessions), Data: sessions})
}

func (h *Handler) v1BrowserExecute(c *fiber.Ctx) error {
	sessionID := strings.TrimSpace(c.Params("id"))
	if sessionID == "" {
		return writeError(c, http.StatusBadRequest, "session id is required", nil)
	}
	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}
	var req InteractRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}
	ctx, cancel := context.WithTimeout(c.UserContext(), 90*time.Second)
	defer cancel()
	results, state, err := h.runBrowserInteraction(ctx, sessionID, &req)
	if err != nil {
		return writeError(c, mapBrowserInteractionError(err), err.Error(), nil)
	}
	html := ""
	if req.ReturnHTML {
		htmlResp, htmlErr := h.browserRuntime.HTML(ctx, sessionID)
		if htmlErr == nil {
			html = truncateHTML(htmlResp.HTML, 50000)
		}
	}
	return c.JSON(InteractResponse{
		Success:  true,
		ScrapeID: sessionID,
		Results:  results,
		HTML:     html,
		Session:  sessionInfoFromState(state),
	})
}

func (h *Handler) v1BrowserLive(c *fiber.Ctx) error {
	sessionID := strings.TrimSpace(c.Params("id"))
	if sessionID == "" {
		return writeError(c, http.StatusBadRequest, "session id is required", nil)
	}
	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}
	response, err := h.browserRuntime.Live(c.UserContext(), sessionID)
	if err != nil {
		return writeError(c, http.StatusNotFound, err.Error(), nil)
	}
	return c.JSON(response)
}

func (h *Handler) v1BrowserDelete(c *fiber.Ctx) error {
	sessionID := strings.TrimSpace(c.Params("id"))
	if sessionID == "" {
		return writeError(c, http.StatusBadRequest, "session id is required", nil)
	}
	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}
	if err := h.browserRuntime.Delete(c.UserContext(), sessionID); err != nil {
		return writeError(c, http.StatusNotFound, err.Error(), nil)
	}
	return c.JSON(fiber.Map{"success": true, "id": sessionID, "status": "deleted"})
}

func (h *Handler) v1BrowserSession(c *fiber.Ctx) error {
	sessionID := strings.TrimSpace(c.Params("id"))
	if sessionID == "" {
		return writeError(c, http.StatusBadRequest, "session id is required", nil)
	}
	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}
	state, err := h.browserRuntime.Get(c.UserContext(), sessionID)
	if err != nil {
		return writeError(c, http.StatusNotFound, err.Error(), nil)
	}
	return c.JSON(browserSessionEnvelope{
		Success:  true,
		ID:       sessionID,
		Resource: "browser",
		Session:  sessionInfoFromState(state),
	})
}

func (h *Handler) v1TeamCreditUsage(c *fiber.Ctx) error {
	orgID, _, _ := h.currentOrgAndUser(c)
	payload := teamCreditUsageEnvelope{
		Success:      true,
		Organization: orgID,
		Source:       "open-core",
	}
	if h.controlPlane == nil || orgID == "internal" {
		return c.JSON(payload)
	}
	account, accountErr := h.controlPlane.GetAccount(c.UserContext(), orgID)
	if account != nil {
		payload.Plan = account.Plan
		payload.Credits = account.Credits
		payload.Metadata = cloneAnyMap(account.Metadata)
		payload.Source = "control-plane"
	}
	if entitlements, err := h.controlPlane.GetEntitlements(c.UserContext(), orgID); err == nil && entitlements != nil {
		payload.Entitlements = make(map[string]bool, len(entitlements.Entitlements))
		for _, entitlement := range entitlements.Entitlements {
			payload.Entitlements[entitlement.Key] = entitlement.Enabled
		}
		payload.Source = "control-plane"
	}
	if quota, err := h.controlPlane.GetQuotaStatus(c.UserContext(), orgID, "crawl_credits"); err == nil && quota != nil {
		payload.Quota = quota
		payload.Source = "control-plane"
	}
	if accountErr != nil && payload.Source == "open-core" {
		return writeError(c, http.StatusBadGateway, "failed to fetch billing account", accountErr.Error())
	}
	return c.JSON(payload)
}

func (h *Handler) v1TeamTokenUsage(c *fiber.Ctx) error {
	orgID, _, _ := h.currentOrgAndUser(c)
	if h.controlPlane != nil && orgID != "internal" {
		if quota, metric, err := h.controlPlane.GetFirstQuotaStatus(c.UserContext(), orgID, "llm_tokens", "tokens", "token_usage", "tokens_used"); err == nil && quota != nil {
			return c.JSON(teamTokenUsageEnvelope{
				Success:      true,
				Organization: orgID,
				TotalTokens:  int64(quota.Used),
				JobCount:     0,
				Metric:       metric,
				Source:       "control-plane",
			})
		}
	}
	totalTokens := int64(0)
	jobCount := 0
	if h.jobStore != nil {
		for _, job := range h.jobStore.List() {
			if job == nil || strings.TrimSpace(job.Meta["org_id"]) != orgID {
				continue
			}
			tokens := collectTokenUsage(job.Result)
			if tokens <= 0 {
				continue
			}
			totalTokens += tokens
			jobCount++
		}
	}
	return c.JSON(teamTokenUsageEnvelope{
		Success:      true,
		Organization: orgID,
		TotalTokens:  totalTokens,
		JobCount:     jobCount,
		Source:       "job-store",
	})
}

// usageBucket is one time-series bucket in the historical usage response.
type usageBucket struct {
	Period      string `json:"period"`
	CreditsUsed int64  `json:"creditsUsed,omitempty"`
	TokensUsed  int64  `json:"tokensUsed,omitempty"`
	JobCount    int    `json:"jobCount,omitempty"`
}

// v1TeamCreditUsageHistorical handles GET /v1/team/credit-usage/historical.
//
// Query params:
//   - granularity: "day" (default) | "hour" | "week"
//   - from: RFC3339 start time (default: 30 days ago)
//   - to:   RFC3339 end time (default: now)
func (h *Handler) v1TeamCreditUsageHistorical(c *fiber.Ctx) error {
	orgID, _, _ := h.currentOrgAndUser(c)
	from, to, gran := parseHistoricalQueryParams(c)

	buckets := map[string]*usageBucket{}

	if h.jobStore != nil {
		for _, job := range h.jobStore.List() {
			if job == nil || strings.TrimSpace(job.Meta["org_id"]) != orgID {
				continue
			}
			if job.CreatedAt.IsZero() || job.CreatedAt.Before(from) || job.CreatedAt.After(to) {
				continue
			}
			key := bucketKey(job.CreatedAt, gran)
			b, ok := buckets[key]
			if !ok {
				b = &usageBucket{Period: key}
				buckets[key] = b
			}
			b.CreditsUsed += collectCreditUsage(job.Result)
		}
	}

	return c.JSON(fiber.Map{
		"success":      true,
		"organization": orgID,
		"granularity":  gran,
		"from":         from.Format(time.RFC3339),
		"to":           to.Format(time.RFC3339),
		"data":         sortedUsageBuckets(buckets),
	})
}

// v1TeamTokenUsageHistorical handles GET /v1/team/token-usage/historical.
func (h *Handler) v1TeamTokenUsageHistorical(c *fiber.Ctx) error {
	orgID, _, _ := h.currentOrgAndUser(c)
	from, to, gran := parseHistoricalQueryParams(c)

	buckets := map[string]*usageBucket{}

	if h.jobStore != nil {
		for _, job := range h.jobStore.List() {
			if job == nil || strings.TrimSpace(job.Meta["org_id"]) != orgID {
				continue
			}
			if job.CreatedAt.IsZero() || job.CreatedAt.Before(from) || job.CreatedAt.After(to) {
				continue
			}
			tokens := collectTokenUsage(job.Result)
			if tokens <= 0 {
				continue
			}
			key := bucketKey(job.CreatedAt, gran)
			b, ok := buckets[key]
			if !ok {
				b = &usageBucket{Period: key}
				buckets[key] = b
			}
			b.TokensUsed += tokens
			b.JobCount++
		}
	}

	return c.JSON(fiber.Map{
		"success":      true,
		"organization": orgID,
		"granularity":  gran,
		"from":         from.Format(time.RFC3339),
		"to":           to.Format(time.RFC3339),
		"data":         sortedUsageBuckets(buckets),
	})
}

// parseHistoricalQueryParams extracts from/to window and granularity from the
// request query string, providing sensible defaults.
func parseHistoricalQueryParams(c *fiber.Ctx) (from, to time.Time, granularity string) {
	to = time.Now().UTC()
	from = to.AddDate(0, 0, -30)

	if raw := strings.TrimSpace(c.Query("from")); raw != "" {
		if t, err := time.Parse(time.RFC3339, raw); err == nil {
			from = t.UTC()
		}
	}
	if raw := strings.TrimSpace(c.Query("to")); raw != "" {
		if t, err := time.Parse(time.RFC3339, raw); err == nil {
			to = t.UTC()
		}
	}

	granularity = strings.ToLower(strings.TrimSpace(c.Query("granularity")))
	switch granularity {
	case "hour", "week":
	default:
		granularity = "day"
	}
	return
}

// bucketKey returns the time-bucket string for a given instant and granularity.
func bucketKey(t time.Time, granularity string) string {
	switch granularity {
	case "hour":
		return t.UTC().Format("2006-01-02T15:00Z")
	case "week":
		year, week := t.UTC().ISOWeek()
		return fmt.Sprintf("%04d-W%02d", year, week)
	default: // day
		return t.UTC().Format("2006-01-02")
	}
}

// collectCreditUsage extracts a credit count from a job result map.
func collectCreditUsage(value interface{}) int64 {
	if value == nil {
		return 0
	}
	m, ok := value.(map[string]interface{})
	if !ok {
		return 0
	}
	for _, key := range []string{"creditsUsed", "credits_used", "credits"} {
		if v, exists := m[key]; exists {
			switch x := v.(type) {
			case int64:
				return x
			case float64:
				return int64(x)
			case int:
				return int64(x)
			}
		}
	}
	return 0
}

// sortedUsageBuckets returns a slice of buckets sorted by period ascending.
func sortedUsageBuckets(m map[string]*usageBucket) []usageBucket {
	out := make([]usageBucket, 0, len(m))
	for _, b := range m {
		out = append(out, *b)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Period < out[j].Period })
	return out
}

func (h *Handler) v1TeamConcurrency(c *fiber.Ctx) error {
	orgID, _, tier := h.currentOrgAndUser(c)
	principal := platform.GetPrincipal(c)
	active := h.countActiveJobsForOrg(orgID)
	limit := h.resolveConcurrencyLimit(c.UserContext(), principal)
	remaining := limit - active
	if remaining < 0 {
		remaining = 0
	}
	return c.JSON(teamConcurrencyEnvelope{
		Success:      true,
		Organization: orgID,
		Tier:         tier,
		Active:       active,
		Limit:        limit,
		Remaining:    remaining,
		Source:       "control-plane+job-store",
	})
}

func (h *Handler) v1TeamQueueStatus(c *fiber.Ctx) error {
	orgID, _, _ := h.currentOrgAndUser(c)
	counts := map[string]int{
		"pending":   0,
		"running":   0,
		"ready":     0,
		"failed":    0,
		"cancelled": 0,
	}
	resources := map[string]int{}
	if h.jobStore != nil {
		for _, job := range h.jobStore.List() {
			if job == nil || strings.TrimSpace(job.Meta["org_id"]) != orgID {
				continue
			}
			counts[string(job.Status)]++
			resource := strings.TrimSpace(job.Meta["kind"])
			if resource == "" {
				resource = "unknown"
			}
			resources[resource]++
		}
	}
	return c.JSON(teamQueueStatusEnvelope{
		Success:      true,
		Organization: orgID,
		Counts:       counts,
		Resources:    resources,
	})
}

func (h *Handler) v1TeamActivity(c *fiber.Ctx) error {
	orgID, _, _ := h.currentOrgAndUser(c)
	limit := c.QueryInt("limit", 25)
	if limit <= 0 {
		limit = 25
	}
	if limit > 100 {
		limit = 100
	}
	events := make([]teamActivityEvent, 0, limit)
	if h.jobStore != nil {
		for _, job := range h.jobStore.List() {
			if job == nil || strings.TrimSpace(job.Meta["org_id"]) != orgID {
				continue
			}
			events = append(events, teamActivityEvent{
				JobID:      job.ID,
				Resource:   strings.TrimSpace(job.Meta["kind"]),
				Status:     string(job.Status),
				Summary:    teamActivitySummary(job),
				CreatedAt:  job.CreatedAt,
				UpdatedAt:  job.UpdatedAt,
				RequestURL: activityRequestURL(job),
			})
		}
	}
	if principal := platform.GetPrincipal(c); principal != nil && principal.UserID != "" && principal.UserID != "internal-service" && h.controlPlane != nil {
		if activities, _, err := h.controlPlane.ListUserActivities(c.UserContext(), principal.UserID, limit); err == nil {
			for _, activity := range activities {
				events = append(events, teamActivityEvent{
					JobID:      stringFromAny(activity.Details["jobId"], ""),
					Resource:   firstNonEmpty(activity.Resource, "activity"),
					Status:     firstNonEmpty(activity.Action, "logged"),
					Summary:    firstNonEmpty(stringFromAny(activity.Details["summary"], ""), activity.Resource),
					CreatedAt:  activity.CreatedAt,
					UpdatedAt:  activity.CreatedAt,
					RequestURL: activityRequestURLFromDetails(activity.Details, h.zdrMode(c)),
				})
			}
		}
	}
	sort.Slice(events, func(i, j int) bool {
		return events[i].CreatedAt.After(events[j].CreatedAt)
	})
	if len(events) > limit {
		events = events[:limit]
	}
	return c.JSON(teamActivityEnvelope{
		Success:      true,
		Organization: orgID,
		Count:        len(events),
		Data:         events,
	})
}

func (h *Handler) ensureWebsocketUpgrade(c *fiber.Ctx) error {
	if websocket.IsWebSocketUpgrade(c) {
		return c.Next()
	}
	return fiber.ErrUpgradeRequired
}

func (h *Handler) streamJobWS(conn *websocket.Conn) {
	jobID := strings.TrimSpace(conn.Params("id"))
	h.streamJobWSByID(conn, jobID)
}

// agentLivecastWS handles GET /v2/agent-livecast?jobId=<id> — compatible with
// the Firecrawl SDK's agent-livecast WS endpoint. It relays SSE StreamManager
// events over the WebSocket connection instead of proxying to an external service.
func (h *Handler) agentLivecastWS(conn *websocket.Conn) {
	jobID := strings.TrimSpace(conn.Query("jobId"))
	h.streamJobWSByID(conn, jobID)
}

// streamJobWSByID is the shared WebSocket streaming implementation used by
// both streamJobWS (path param) and agentLivecastWS (query param).
func (h *Handler) streamJobWSByID(conn *websocket.Conn, jobID string) {
	if jobID == "" || h.jobStore == nil || h.streamManager == nil {
		_ = conn.Close()
		return
	}
	if _, ok := h.jobStore.Get(jobID); !ok {
		_ = conn.Close()
		return
	}

	eventCh, cleanup := h.streamManager.Subscribe(context.Background(), jobID)
	defer cleanup()

	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case event, ok := <-eventCh:
			if !ok {
				return
			}
			if err := conn.WriteJSON(event); err != nil {
				return
			}
		case <-ticker.C:
			job, ok := h.jobStore.Get(jobID)
			if !ok || job == nil {
				_ = conn.WriteJSON(sse.Event{Type: "job:deleted", Timestamp: time.Now(), JobID: jobID})
				return
			}
			if err := conn.WriteJSON(sse.Event{
				Type:      "heartbeat",
				Timestamp: time.Now(),
				JobID:     jobID,
				Data: map[string]interface{}{
					"jobId":    job.ID,
					"status":   job.Status,
					"progress": job.Progress,
				},
			}); err != nil {
				return
			}
			if job.Status == jobs.StatusReady || job.Status == jobs.StatusFailed || job.Status == jobs.StatusCancelled {
				return
			}
		}
	}
}

func (h *Handler) mirrorBatchJob(jobID string) {
	if h.batchManager == nil || h.jobStore == nil {
		return
	}
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()

	for range ticker.C {
		status := h.batchManager.GetJobStatus(jobID)
		if status == nil {
			return
		}
		jobStatus := mapBatchStatusToJobStatus(status.Status)
		progress := progressPercent(status.Completed+status.Failed, status.Total)
		_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
			current.Status = jobStatus
			current.Progress = progress
			current.Error = status.Error
			current.Result = map[string]any{
				"completed": status.Completed,
				"failed":    status.Failed,
				"total":     status.Total,
			}
		})
		if h.streamManager != nil {
			h.streamManager.Broadcast(jobID, sse.EventJobProgress, map[string]interface{}{
				"completed": status.Completed,
				"failed":    status.Failed,
				"total":     status.Total,
				"status":    status.Status,
				"progress":  progress,
			})
		}
		if h.streamManager != nil {
			switch status.Status {
			case "completed":
				h.streamManager.Broadcast(jobID, sse.EventJobCompleted, map[string]any{"progress": 100})
			case "failed", "cancelled":
				h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"status": status.Status, "error": status.Error})
			}
		}
		if status.Status == "completed" || status.Status == "failed" || status.Status == "cancelled" {
			return
		}
	}
}

func (h *Handler) runBrowserInteraction(ctx context.Context, sessionID string, req *InteractRequest) ([]session.ActionResult, *quarrybrowser.SessionState, error) {
	if req == nil {
		return nil, nil, fmt.Errorf("request is required")
	}
	if req.Prompt != "" || req.Goal != "" {
		results, state, err := h.aiInteract(ctx, sessionID, req)
		if err != nil {
			return nil, nil, fmt.Errorf("AI interaction failed: %v", err)
		}
		return results, state, nil
	}
	if len(req.Actions) == 0 {
		return nil, nil, fmt.Errorf("either 'actions', 'prompt', or 'goal' is required")
	}
	if h.browserRuntime == nil {
		return nil, nil, fmt.Errorf("browser runtime not initialized")
	}
	response, err := h.browserRuntime.Execute(ctx, sessionID, quarrybrowser.ExecuteRequest{
		Actions:         req.Actions,
		ContinueOnError: req.ContinueOnError,
	})
	if err != nil {
		return nil, nil, err
	}
	return response.Results, &response.State, nil
}

func (h *Handler) newAsyncCreateEnvelope(c *fiber.Ctx, resource, id string, createdAt, expiresAt time.Time, status string, resolvedOptions interface{}) asyncCreateEnvelope {
	return asyncCreateEnvelope{
		Success:         true,
		ID:              id,
		Resource:        resource,
		Status:          status,
		CreatedAt:       createdAt,
		ExpiresAt:       expiresAt,
		StatusURL:       buildAbsoluteResourceURL(c, "/v1/"+resourcePath(resource)+"/"+id),
		EventsURL:       buildAbsoluteResourceURL(c, "/v1/jobs/"+id+"/events"),
		WebsocketURL:    buildAbsoluteWebsocketURL(c, "/v1/jobs/"+id+"/ws"),
		ResolvedOptions: resolvedOptions,
	}
}

func buildAbsoluteResourceURL(c *fiber.Ctx, path string) string {
	host := strings.TrimSpace(c.Get("Host"))
	if host == "" {
		host = c.Hostname()
	}
	return fmt.Sprintf("%s://%s%s", c.Protocol(), host, path)
}

func buildAbsoluteWebsocketURL(c *fiber.Ctx, path string) string {
	scheme := "ws"
	if strings.EqualFold(c.Protocol(), "https") {
		scheme = "wss"
	}
	host := strings.TrimSpace(c.Get("Host"))
	if host == "" {
		host = c.Hostname()
	}
	return fmt.Sprintf("%s://%s%s", scheme, host, path)
}

func buildResourceNextURL(c *fiber.Ctx, resource, id string, skip, limit, total, returned int) string {
	if limit <= 0 || returned <= 0 {
		return ""
	}
	nextSkip := skip + returned
	if nextSkip >= total {
		return ""
	}
	return buildAbsoluteResourceURL(c, fmt.Sprintf("/v1/%s/%s?skip=%d&limit=%d", resourcePath(resource), id, nextSkip, limit))
}

func buildAbsoluteSearchNextURL(c *fiber.Ctx, jobID string, skip, limit, total, returned int) string {
	if limit <= 0 {
		limit = returned
	}
	return buildResourceNextURL(c, "search", jobID, skip, limit, total, returned)
}

func resourcePath(resource string) string {
	return strings.TrimPrefix(resource, "/")
}

func paginateBatchResults(results []models.BatchScrapeResult, skip, limit int) ([]models.BatchScrapeResult, int) {
	total := len(results)
	if skip < 0 {
		skip = 0
	}
	if limit <= 0 {
		limit = total
	}
	if skip > total {
		skip = total
	}
	end := skip + limit
	if end > total {
		end = total
	}
	return append([]models.BatchScrapeResult(nil), results[skip:end]...), total
}

func mapBatchStatusToJobStatus(status string) jobs.Status {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "queued":
		return jobs.StatusPending
	case "processing":
		return jobs.StatusRunning
	case "completed":
		return jobs.StatusReady
	case "failed":
		return jobs.StatusFailed
	case "cancelled":
		return jobs.StatusCancelled
	default:
		return jobs.StatusRunning
	}
}

func sessionInfoFromState(state *quarrybrowser.SessionState) session.SessionInfo {
	if state == nil {
		return session.SessionInfo{}
	}
	info := state.Session
	if strings.TrimSpace(state.CurrentURL) != "" {
		info.URL = state.CurrentURL
	}
	return info
}

func mapCrawlError(status quarrycrawl.RunStatus) string {
	if status == quarrycrawl.StatusFailed {
		return "crawl failed"
	}
	return ""
}

func collectTokenUsage(value interface{}) int64 {
	switch typed := value.(type) {
	case nil:
		return 0
	case map[string]interface{}:
		total := int64(0)
		for key, current := range typed {
			switch key {
			case "tokensUsed", "totalTokens", "tokenUsage":
				total += int64(intFromAny(current))
			default:
				total += collectTokenUsage(current)
			}
		}
		return total
	case []interface{}:
		total := int64(0)
		for _, current := range typed {
			total += collectTokenUsage(current)
		}
		return total
	default:
		return 0
	}
}

func teamActivitySummary(job *jobs.Job) string {
	if job == nil {
		return ""
	}
	if strings.EqualFold(strings.TrimSpace(job.Meta["zdr_mode"]), "true") {
		return strings.TrimSpace(job.Meta["kind"])
	}
	resource := strings.TrimSpace(job.Meta["kind"])
	switch resource {
	case "crawl":
		return firstNonEmpty(job.Meta["url"], "crawl")
	case "search", "research":
		return firstNonEmpty(job.Meta["query"], stringFromAny(job.Result["query"], resource))
	case "extract":
		return fmt.Sprintf("extract %s URLs", firstNonEmpty(job.Meta["url_count"], "0"))
	case "batch_scrape":
		return fmt.Sprintf("batch %s URLs", firstNonEmpty(job.Meta["url_count"], "0"))
	default:
		return resource
	}
}

func activityRequestURL(job *jobs.Job) string {
	if job == nil || strings.EqualFold(strings.TrimSpace(job.Meta["zdr_mode"]), "true") {
		return ""
	}
	return firstNonEmpty(job.Meta["url"], job.Meta["query"])
}

func activityRequestURLFromDetails(details map[string]interface{}, redacted bool) string {
	if redacted {
		return ""
	}
	return firstNonEmpty(
		stringFromAny(details["url"], ""),
		stringFromAny(details["query"], ""),
		stringFromAny(details["requestUrl"], ""),
	)
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return value
		}
	}
	return ""
}
