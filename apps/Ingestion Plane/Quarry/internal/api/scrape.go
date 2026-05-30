package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"
	zlog "github.com/rs/zerolog/log"
	temporalclient "go.temporal.io/sdk/client"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/asyncjobs"
	"github.com/triodelab/quarry/internal/batch"
	quarrybrowser "github.com/triodelab/quarry/internal/browser"
	"github.com/triodelab/quarry/internal/config"
	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/dataplane"
	"github.com/triodelab/quarry/internal/executor"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/modules"
	"github.com/triodelab/quarry/internal/nats"
	"github.com/triodelab/quarry/internal/notify"
	"github.com/triodelab/quarry/internal/objectstore"
	"github.com/triodelab/quarry/internal/otp"
	"github.com/triodelab/quarry/internal/pipeline"
	"github.com/triodelab/quarry/internal/platform"
	"github.com/triodelab/quarry/internal/scraper"
	quarrysearch "github.com/triodelab/quarry/internal/search"
	"github.com/triodelab/quarry/internal/searchindex"
	"github.com/triodelab/quarry/internal/security"
	"github.com/triodelab/quarry/internal/session"
	"github.com/triodelab/quarry/internal/sse"
	quarrytemporal "github.com/triodelab/quarry/internal/temporal"
	"github.com/triodelab/quarry/internal/tracker"
)

type asyncJobDispatcher interface {
	Dispatch(context.Context, asyncjobs.Message) error
	Cancel(context.Context, asyncjobs.CancelMessage) error
	Close() error
}

type Handler struct {
	scraper               *scraper.Scraper
	cfg                   *config.Config
	agentClient           *ai.AgentModeHTTPClient
	security              *security.Service
	metrics               *Telemetry
	changeTracker         *tracker.ChangeTracker
	moduleRegistry        *modules.Registry
	jobStore              *jobs.Store
	extractionJobStore    jobs.ExtractionStore // For Phase 3.1 async extraction
	schemaCache           *SchemaCache         // Phase 3.1 schema cache
	dataplaneClient       *dataplane.Client    // Integration with data plane document and retrieval services
	immediateExec         *executor.ImmediateExecutor
	scheduledExec         *executor.ScheduledExecutor
	execRouter            *executor.Router
	temporalClient        temporalclient.Client
	pipelineChain         *pipeline.Chain
	batchManager          *batch.Manager
	streamManager         *sse.StreamManager
	resultCache           *redis.Client // Redis client for caching module scrape results
	crawlStore            quarrycrawl.Store
	searchAsyncStore      quarrysearch.AsyncStore
	searchClient          *quarrysearch.BraveClient
	githubSearchClient    *quarrysearch.GitHubClient
	localSearchIndex      *searchindex.Index
	artifactStore         objectstore.Store
	sharedPublisher       *nats.SharedPublisher // NATS publisher for cross-plane events
	sessionManager        *session.Manager      // Phase 2: interactive browser sessions
	browserRuntime        quarrybrowser.Runtime
	asyncDispatcher       asyncJobDispatcher
	controlPlane          *platform.ControlPlaneService
	idempotencyStore      platform.IdempotencyStore
	authClient            *platform.AuthClient // auth-core client for signup
	otpStore              *otp.Store           // OTP store for email verification
	emailSender           notify.Sender        // Email backend for OTP delivery
	activeCrawlCancels    sync.Map
	activeSearchCancels   sync.Map
	activeExtractCancels  sync.Map
	activeResearchCancels sync.Map
	activeAgentCancels    sync.Map
	fetchFormatsFn        func(context.Context, string, *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error)
	extractStructuredFn   func(context.Context, string, *scraper.StructuredExtractOptions) (map[string]interface{}, error)
	extractAIDataFn       func(context.Context, *ai.ExtractRequest) (*ai.ExtractResponse, error)
	ingestExtractionFn    func(context.Context, string, *dataplane.DocumentCreateRequest) (*dataplane.DocumentResponse, error)
	crawlPreviewFn        func(context.Context, quarrycrawl.Spec, []string) (map[string]interface{}, error)
}

func NewHandler(scraperEngine *scraper.Scraper, cfg *config.Config, sharedPublisher *nats.SharedPublisher) *Handler {
	registry := modules.NewDefaultRegistry()
	jobTTL := 24 * time.Hour
	if cfg != nil && cfg.BatchJobTTLHours > 0 {
		jobTTL = time.Duration(cfg.BatchJobTTLHours) * time.Hour
	}
	store := jobs.NewStore(jobTTL)
	batchWorkers := 4
	if cfg != nil && cfg.BatchMaxWorkers > 0 {
		batchWorkers = cfg.BatchMaxWorkers
	}
	webhookSecret := ""
	if cfg != nil {
		webhookSecret = cfg.WebhookSecret
	}
	batchManager := batch.NewManager(scraperEngine, batchWorkers, jobTTL, webhookSecret)
	streamManager := sse.NewStreamManager(store)

	// Wire per-URL batch results to SSE streaming so subscribers receive
	// individual crawl:data events as each URL completes.
	batchManager.SetResultCallback(func(jobID string, result models.BatchScrapeResult) {
		streamManager.Broadcast(jobID, sse.EventCrawlData, result)
	})
	agentBaseURL := ""
	agentAPIKey := ""
	agentAPIKeyHeader := ""
	if cfg != nil {
		agentBaseURL = cfg.AICoreHTTPBaseURL
		agentAPIKey = cfg.APIKey
		agentAPIKeyHeader = cfg.APIKeyHeader
	}
	agentClient := ai.NewAgentModeHTTPClient(agentBaseURL, agentAPIKey, agentAPIKeyHeader)
	searchTimeout := 10 * time.Second
	searchAPIKey := ""
	searchBaseURL := ""
	githubToken := ""
	githubBaseURL := ""
	githubTimeout := 10 * time.Second
	if cfg != nil && cfg.BraveSearchTimeoutSec > 0 {
		searchTimeout = time.Duration(cfg.BraveSearchTimeoutSec) * time.Second
	}
	if cfg != nil {
		searchAPIKey = cfg.BraveSearchAPIKey
		searchBaseURL = cfg.BraveSearchBaseURL
		githubToken = cfg.GitHubToken
		githubBaseURL = cfg.GitHubAPIBaseURL
		if cfg.GitHubTimeoutSec > 0 {
			githubTimeout = time.Duration(cfg.GitHubTimeoutSec) * time.Second
		}
	}
	searchClient := quarrysearch.NewBraveClient(searchAPIKey, searchBaseURL, searchTimeout)
	githubSearchClient := quarrysearch.NewGitHubClient(githubToken, githubBaseURL, githubTimeout)

	var temporalCli temporalclient.Client
	if cfg != nil && cfg.TemporalEnabled {
		if cli, err := quarrytemporal.NewClient(cfg); err == nil {
			temporalCli = cli
		}
	}

	// Build Redis result cache client (reuses REDIS_URL when CACHE_BACKEND=redis)
	var resultCacheClient *redis.Client
	if cfg != nil && strings.EqualFold(strings.TrimSpace(cfg.CacheBackend), "redis") && cfg.RedisURL != "" {
		if opt, err := redis.ParseURL(cfg.RedisURL); err == nil {
			resultCacheClient = redis.NewClient(opt)
		}
	}

	// Initialize extraction job store for Phase 3.1 async extraction
	var extractionJobStore jobs.ExtractionStore
	backendType := strings.TrimSpace(os.Getenv("JOB_STORE_BACKEND"))
	extractionJobTTL := time.Hour

	if strings.EqualFold(backendType, "postgres") {
		// Try to use PostgreSQL backend
		dsn := strings.TrimSpace(os.Getenv("QUARRY_POSTGRES_DSN"))
		if dsn == "" {
			dsn = strings.TrimSpace(os.Getenv("DATABASE_URL"))
		}

		if dsn != "" {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()

			db, err := pgxpool.New(ctx, dsn)
			if err == nil {
				if pingErr := db.Ping(ctx); pingErr == nil {
					extractionJobStore = jobs.NewPostgresJobStore(db, extractionJobTTL)
					zlog.Info().Msg("extraction jobs will be persisted to PostgreSQL")
				} else {
					db.Close()
				}
			}
		}
	}

	// Fallback to in-memory store if PostgreSQL not available
	if extractionJobStore == nil {
		extractionJobStore = jobs.NewInMemoryJobStore(extractionJobTTL)
		zlog.Info().Msg("extraction jobs will use in-memory store (transient)")
	}

	// Initialize schema cache (Phase 3.1 Feature 2)
	schemaCacheTTL := 1 * time.Hour
	schemaCache := NewSchemaCache(schemaCacheTTL)

	// Initialize dataplane client for ingesting extracted content (Phase 3.1 Feature 3)
	dataplaneBaseURL := strings.TrimSpace(os.Getenv("DATAPLANE_BASE_URL"))
	if dataplaneBaseURL == "" {
		dataplaneBaseURL = "http://data-documents-service:8001" // Default to docker-compose host
	}
	dataplaneClient := dataplane.NewClient(dataplaneBaseURL)
	retrievalBaseURL := ""
	internalAPIKey := ""
	if cfg != nil {
		retrievalBaseURL = cfg.RetrievalBaseURL
		internalAPIKey = cfg.DataplaneInternalAPIKey
	}
	dataplaneClient.SetRetrievalBaseURL(retrievalBaseURL)
	dataplaneClient.SetInternalAPIKey(internalAPIKey)
	zlog.Info().Str("baseURL", dataplaneBaseURL).Msg("dataplane client initialized for ingestion")

	var artifactStore objectstore.Store
	if store, err := objectstore.NewFromConfig(cfg); err != nil {
		zlog.Warn().Err(err).Str("backend", cfg.ArtifactStoreBackend).Msg("artifact store disabled")
	} else {
		artifactStore = store
	}

	var controlPlaneService *platform.ControlPlaneService
	if cfg != nil && (strings.TrimSpace(cfg.BillingCoreURL) != "" || strings.TrimSpace(cfg.OrgCoreURL) != "" || strings.TrimSpace(cfg.UserCoreURL) != "") {
		controlPlaneService = platform.NewControlPlaneService(platform.ControlPlaneConfig{
			BillingBaseURL: cfg.BillingCoreURL,
			OrgBaseURL:     cfg.OrgCoreURL,
			UserBaseURL:    cfg.UserCoreURL,
			InternalAPIKey: cfg.AuthCoreInternalAPIKey,
			CacheTTL:       time.Duration(cfg.ControlPlaneCacheSec) * time.Second,
		})
	}

	var idempotencyStore platform.IdempotencyStore
	if cfg != nil && cfg.IdempotencyTTLHours > 0 {
		idempotencyStore = platform.NewIdempotencyStore(cfg.RedisURL)
	}

	// OTP store — requires Redis (reuses result-cache client when available).
	var otpStore *otp.Store
	var emailSender notify.Sender = notify.NoopSender{}
	if resultCacheClient != nil {
		otpStore = otp.NewStore(resultCacheClient)
	} else if cfg != nil && cfg.RedisURL != "" {
		if opt, err := redis.ParseURL(cfg.RedisURL); err == nil {
			otpStore = otp.NewStore(redis.NewClient(opt))
		}
	}
	if cfg != nil {
		if cfg.ResendAPIKey != "" {
			emailSender = notify.NewResendSender(cfg.ResendAPIKey, cfg.SMTPFrom)
		} else if cfg.SMTPHost != "" {
			emailSender = notify.NewSMTPSender(cfg.SMTPHost, cfg.SMTPPort, cfg.SMTPFrom, cfg.SMTPUser, cfg.SMTPPass)
		}
	}

	// Initialize local search index (Bleve-backed).
	var localIndex *searchindex.Index
	if cfg != nil && cfg.SearchIndexEnabled {
		idx, idxErr := searchindex.Open(searchindex.WithPath(cfg.SearchIndexPath))
		if idxErr != nil {
			zlog.Warn().Err(idxErr).Msg("local search index disabled")
		} else {
			localIndex = idx
			cnt, _ := idx.Count()
			zlog.Info().Str("path", cfg.SearchIndexPath).Uint64("docs", cnt).Msg("local search index opened")
		}
	}

	return &Handler{
		scraper:            scraperEngine,
		cfg:                cfg,
		agentClient:        agentClient,
		security:           security.NewService(cfg),
		metrics:            NewTelemetry(),
		changeTracker:      tracker.NewChangeTracker(),
		moduleRegistry:     registry,
		jobStore:           store,
		extractionJobStore: extractionJobStore,
		schemaCache:        schemaCache,
		dataplaneClient:    dataplaneClient,
		immediateExec:      executor.NewImmediateExecutor(scraperEngine, registry),
		scheduledExec:      executor.NewScheduledExecutor(cfg, temporalCli),
		execRouter:         executor.NewRouter(cfg != nil && cfg.TemporalEnabled),
		temporalClient:     temporalCli,
		pipelineChain:      pipeline.NewDefaultChain(),
		batchManager:       batchManager,
		streamManager:      streamManager,
		resultCache:        resultCacheClient,
		crawlStore:         quarrycrawl.NewStore(cfg, jobTTL),
		searchAsyncStore:   quarrysearch.NewAsyncStore(cfg, jobTTL),
		searchClient:       searchClient,
		githubSearchClient: githubSearchClient,
		localSearchIndex:   localIndex,
		artifactStore:      artifactStore,
		sharedPublisher:    sharedPublisher,
		controlPlane:       controlPlaneService,
		idempotencyStore:   idempotencyStore,
		otpStore:           otpStore,
		emailSender:        emailSender,
	}
}

func (h *Handler) Close() error {
	if h.jobStore != nil {
		_ = h.jobStore.Close()
	}
	if h.extractionJobStore != nil {
		_ = h.extractionJobStore.Close()
	}
	if h.schemaCache != nil {
		_ = h.schemaCache.Close()
	}
	if h.changeTracker != nil {
		_ = h.changeTracker.Close()
	}
	if h.batchManager != nil {
		_ = h.batchManager.Close()
	}
	if h.resultCache != nil {
		_ = h.resultCache.Close()
		h.resultCache = nil
	}
	if h.crawlStore != nil {
		_ = h.crawlStore.Close()
		h.crawlStore = nil
	}
	if h.searchAsyncStore != nil {
		_ = h.searchAsyncStore.Close()
		h.searchAsyncStore = nil
	}
	if h.localSearchIndex != nil {
		_ = h.localSearchIndex.Close()
		h.localSearchIndex = nil
	}
	if h.browserRuntime != nil {
		_ = h.browserRuntime.Close()
		h.browserRuntime = nil
	}
	if h.asyncDispatcher != nil {
		_ = h.asyncDispatcher.Close()
		h.asyncDispatcher = nil
	}
	if h.sessionManager != nil {
		_ = h.sessionManager.Close()
		h.sessionManager = nil
	}
	if h.artifactStore != nil {
		_ = h.artifactStore.Close()
		h.artifactStore = nil
	}
	if h.temporalClient != nil {
		h.temporalClient.Close()
	}
	if h.controlPlane != nil {
		_ = h.controlPlane.Close()
	}
	return nil
}

func (h *Handler) fetchFormats(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
	if h.fetchFormatsFn != nil {
		return h.fetchFormatsFn(ctx, targetURL, opts)
	}
	if h.scraper == nil {
		return nil, nil, fmt.Errorf("scraper is not initialized")
	}
	outputs, actionResults, err := h.scraper.FetchFormats(ctx, targetURL, opts)
	if err == nil {
		h.maybeIndexDocument(targetURL, outputs)
	}
	return outputs, actionResults, err
}

// maybeIndexDocument adds a scraped page to the local Bleve index when enabled.
// Runs best-effort — errors are logged but never propagated.
func (h *Handler) maybeIndexDocument(targetURL string, outputs map[string]interface{}) {
	if h.localSearchIndex == nil || !h.localSearchIndex.Enabled() {
		return
	}
	body, _ := outputs["markdown"].(string)
	if body == "" {
		body, _ = outputs["text"].(string)
	}
	if body == "" {
		return
	}
	title, _ := outputs["title"].(string)
	if title == "" {
		title = targetURL
	}
	if err := h.localSearchIndex.Put(searchindex.Document{
		URL:    targetURL,
		Title:  title,
		Body:   body,
		Source: "scrape",
	}); err != nil {
		zlog.Warn().Err(err).Str("url", targetURL).Msg("failed to index scraped document")
	}
}

func (h *Handler) extractStructured(ctx context.Context, targetURL string, opts *scraper.StructuredExtractOptions) (map[string]interface{}, error) {
	if h.extractStructuredFn != nil {
		return h.extractStructuredFn(ctx, targetURL, opts)
	}
	if h.scraper == nil {
		return nil, fmt.Errorf("scraper is not initialized")
	}
	return h.scraper.ExtractStructured(ctx, targetURL, opts)
}

func (h *Handler) extractAIData(ctx context.Context, req *ai.ExtractRequest) (*ai.ExtractResponse, error) {
	if h.extractAIDataFn != nil {
		return h.extractAIDataFn(ctx, req)
	}
	if h.scraper == nil || h.scraper.AIClient() == nil {
		return nil, fmt.Errorf("ai extraction is not initialized")
	}
	return h.scraper.AIClient().ExtractData(ctx, req)
}

func (h *Handler) ingestExtraction(ctx context.Context, orgID string, req *dataplane.DocumentCreateRequest) (*dataplane.DocumentResponse, error) {
	if h.ingestExtractionFn != nil {
		return h.ingestExtractionFn(ctx, orgID, req)
	}
	if h.dataplaneClient == nil {
		return nil, fmt.Errorf("dataplane client is not initialized")
	}
	return h.dataplaneClient.IngestExtraction(ctx, orgID, req)
}

// SetSessionManager injects the session manager (called from main after pool init).
func (h *Handler) SetSessionManager(sm *session.Manager) {
	h.sessionManager = sm
	if sm != nil {
		h.browserRuntime = quarrybrowser.NewLocalRuntime(sm)
	}
}

func (h *Handler) SetBrowserRuntime(runtime quarrybrowser.Runtime) {
	h.browserRuntime = runtime
}

func (h *Handler) SetAsyncDispatcher(dispatcher asyncJobDispatcher) {
	h.asyncDispatcher = dispatcher
}

func (h *Handler) SetAuthClient(client *platform.AuthClient) {
	// Stored on controlPlane's auth bridge — expose directly for signup handler.
	h.authClient = client
}

func (h *Handler) Register(app *fiber.App) {
	app.Use(func(c *fiber.Ctx) error {
		start := time.Now()
		err := c.Next()
		if h.metrics != nil {
			status := c.Response().StatusCode()
			isError := err != nil || status >= http.StatusBadRequest
			h.metrics.Record(time.Since(start), isError)
		}
		return err
	})

	app.Get("/health", func(c *fiber.Ctx) error {
		return c.Status(http.StatusOK).JSON(fiber.Map{"success": true, "service": "quarry", "status": "ok"})
	})
	app.Get("/ready", h.ready)
	app.Get("/metrics", h.metricsEndpoint)
	app.Get("/v1/modules", h.listModules)
	app.Post("/v1/scrape", h.scrape)
	app.Post("/v1/agent/sync", h.agentMode)
	app.Get("/v1/jobs/:id", h.getJobStatus)
	app.Get("/v1/jobs/:id/stream", h.streamJob)
	app.Get("/v1/change/latest", h.getLatestChange)
	app.Post("/v1/change/check", h.checkChangeNow)
	app.Post("/v1/extract/:id/ingest", h.handleExtractIngest)
	h.registerPlatformV1(app)

	// Agent self-service onboarding (no auth — public).
	h.registerAgentSignup(app)

	// Phase 3: workflow signal endpoints (pause/resume/cancel)
	app.Post("/v1/jobs/:id/signal/:action", h.signalJob)

	// U2-16 (velion ui-ux-velion-gap.md §10): agent-tools surface.
	// `POST /v1/agent-tools/fetch` + `/search` give agent-core's
	// `web_fetch` and `web_search` builtins a real backend.
	h.registerAgentTools(app)

	// Phase 2: interactive browser sessions
	h.RegisterV2(app)
}

func (h *Handler) scrape(c *fiber.Ctx) error {
	var req models.ScrapeAPIRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}

	if req.Collection == "" && req.URL != "" {
		if err := validateAbsoluteHTTPURL(req.URL); err != nil {
			return writeError(c, http.StatusBadRequest, err.Error(), nil)
		}
		req.Collection = inferCollection(req.URL)
	}
	if req.Collection == "" {
		return writeError(c, http.StatusBadRequest, "collection or inferable url is required", nil)
	}
	if !modulePattern.MatchString(req.Collection) {
		return writeError(c, http.StatusBadRequest, "collection contains invalid characters", nil)
	}
	if req.MaxPages <= 0 {
		req.MaxPages = 1
	}
	if h.cfg != nil {
		if req.MaxPages > h.cfg.ScrapeMaxPages {
			return writeError(c, http.StatusBadRequest, "maxPages exceeds configured limit", nil)
		}
		if req.EnrichLimit < 0 || req.EnrichLimit > h.cfg.ScrapeMaxEnrichLimit {
			return writeError(c, http.StatusBadRequest, "enrichLimit is out of allowed range", nil)
		}
		if req.MaxAge < 0 || req.MaxAge > h.cfg.ScrapeMaxAgeMs {
			return writeError(c, http.StatusBadRequest, "maxAge is out of allowed range", nil)
		}
	}

	var securityAssessment *security.Assessment
	if req.URL != "" && h.security != nil {
		secCtx, cancel := context.WithTimeout(c.UserContext(), 8*time.Second)
		defer cancel()
		assessment, err := h.security.AssessURL(secCtx, req.URL)
		if err != nil {
			return writeError(c, http.StatusBadRequest, "security assessment failed", err.Error())
		}
		securityAssessment = assessment
		if assessment.Blocked {
			return c.Status(http.StatusForbidden).JSON(models.ScrapeAPIResponse{Success: false, Security: assessment, Error: assessment.BlockReason})
		}
	}

	scrapeReq := &models.ScrapeRequest{
		BaseURL:        req.URL,
		Collection:     req.Collection,
		MaxPages:       req.MaxPages,
		Enrich:         req.Enrich,
		EnrichLimit:    req.EnrichLimit,
		MaxAge:         req.MaxAge,
		ChangeTracking: req.ChangeTrack,
	}

	var result *models.ScrapeResult
	outputs := map[string]interface{}{}
	actionResults := []models.ActionExecutionResult{}
	module, _, ok := h.resolveModule(req.Module)
	if !ok {
		return writeError(c, http.StatusBadRequest, "module is not available", nil)
	}

	var runErr error

	// singleURLFetch=true means we skip the module collection scraper (browser pool)
	// and go straight to fetchFormats (waterfall/colly). Declared here so the goto
	// below does not jump over it.
	singleURLFetch := req.URL != "" && req.MaxPages <= 1 && len(req.Formats) > 0 && !req.Enrich

	// Wrapper for caching full scrape including formats
	type CachedFullScrape struct {
		Result  *models.ScrapeResult           `json:"result"`
		Outputs map[string]interface{}         `json:"outputs"`
		Actions []models.ActionExecutionResult `json:"actions"`
	}

	cacheKey := ""
	if h.resultCache != nil && req.MaxAge > 0 {
		formatsStr := strings.Join(req.Formats, ",")
		cacheKey = fmt.Sprintf("scrape:full:%s:%s:%s:%d:%v:%d:fmt(%s)",
			module.Name(), req.URL, req.Collection, req.MaxPages, req.Enrich, req.EnrichLimit, formatsStr)

		if raw, getErr := h.resultCache.Get(c.UserContext(), cacheKey).Bytes(); getErr == nil && len(raw) > 0 {
			var cached CachedFullScrape
			if jsonErr := json.Unmarshal(raw, &cached); jsonErr == nil {
				zlog.Debug().Str("key", cacheKey).Msg("full scrape cache hit")
				result = cached.Result
				outputs = cached.Outputs
				actionResults = cached.Actions
				goto SKIP_LIVE_SCRAPE
			}
		}
	}

	if !singleURLFetch {
		result, runErr = h.runModuleCached(c.UserContext(), module, scrapeReq)
		if runErr != nil {
			return c.Status(http.StatusBadGateway).JSON(models.ScrapeAPIResponse{Success: false, Security: securityAssessment, Error: runErr.Error()})
		}
	} else {
		result = &models.ScrapeResult{Count: 0, Products: []*models.Product{}}
	}

	if req.URL != "" {
		proxyURL := h.resolveProxyURL(c, nil)
		if req.Proxy != nil {
			proxyURL = h.resolveProxyURL(c, &proxyRequest{
				URL:    req.Proxy.URL,
				Type:   req.Proxy.Type,
				Region: req.Proxy.Region,
			})
		}

		formatOpts := &scraper.FormatOptions{
			Formats:         req.Formats,
			IncludeTags:     req.IncludeTags,
			ExcludeTags:     req.ExcludeTags,
			OnlyMainContent: req.OnlyMain,
			WaitFor:         req.WaitFor,
			Headers:         req.Headers,
			Actions:         toActionSteps(req.Actions),
			Mobile:          req.Mobile,
			Viewport:        req.Viewport,
			Location:        req.Location,
			BlockAds:        req.BlockAds,
			ProxyURL:        proxyURL,
			ParserMode:      req.ParserMode,
		}

		fetchedOutputs, actionsRun, formatErr := h.fetchFormats(c.UserContext(), req.URL, formatOpts)
		if formatErr != nil {
			return c.Status(http.StatusBadGateway).JSON(models.ScrapeAPIResponse{Success: false, Security: securityAssessment, Error: formatErr.Error()})
		}
		outputs = fetchedOutputs
		actionResults = toActionExecutionResults(actionsRun)
		outputs, actionResults = h.persistResponseArtifacts(c.UserContext(), req.URL, outputs, actionResults)
	}

	if cacheKey != "" {
		cachedData := CachedFullScrape{
			Result:  result,
			Outputs: outputs,
			Actions: actionResults,
		}
		if encoded, jsonErr := json.Marshal(cachedData); jsonErr == nil {
			ttl := time.Duration(req.MaxAge) * time.Millisecond
			if setErr := h.resultCache.Set(c.UserContext(), cacheKey, encoded, ttl).Err(); setErr != nil {
				zlog.Warn().Err(setErr).Str("key", cacheKey).Msg("failed to cache full scrape result")
			} else {
				zlog.Info().Str("key", cacheKey).Dur("ttl", ttl).Msg("full scrape result cached")
			}
		}
	}

SKIP_LIVE_SCRAPE:

	var changeResult *models.ChangeTrackingResult
	if h.changeTracker != nil && req.ChangeTrack != nil && req.ChangeTrack.Enabled {
		payload, payloadErr := buildChangeTrackingPayload(result, outputs)
		if payloadErr == nil {
			trackingKey := req.URL
			if trackingKey == "" {
				trackingKey = "collection:" + req.Collection
			}
			var (
				tracked  *models.ChangeTrackingResult
				trackErr error
			)
			if req.ChangeTrack.DryRun {
				tracked, trackErr = h.changeTracker.Compare(c.UserContext(), trackingKey, payload, req.ChangeTrack)
			} else {
				tracked, trackErr = h.changeTracker.Track(c.UserContext(), trackingKey, payload, req.ChangeTrack)
			}
			if trackErr == nil {
				changeResult = tracked
				// Enhance changed diffs with AI analysis (non-blocking).
				h.enrichChangeResultWithAI(c.UserContext(), changeResult, req.URL, req.ChangeTrack)
			}
		}
	}

	return c.JSON(models.ScrapeAPIResponse{Success: true, Data: result, Outputs: outputs, Actions: actionResults, Security: securityAssessment, ChangeTracking: changeResult})
}

// runModuleCached executes the module, honouring req.MaxAge for Redis-backed result caching.
// When MaxAge > 0 and a Redis client is available, a successful result is cached for MaxAge ms.
// Subsequent requests within that window are served from cache without a live scrape.
func (h *Handler) runModuleCached(ctx context.Context, mod modules.Module, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	if h.resultCache == nil || req.MaxAge <= 0 {
		// No cache configured or caller does not want caching — run live.
		return mod.Run(ctx, h.scraper, req)
	}

	cacheKey := fmt.Sprintf("scrape:module:%s:%s:%s:%d:%v:%d",
		mod.Name(), req.BaseURL, req.Collection, req.MaxPages, req.Enrich, req.EnrichLimit)

	// Check cache first.
	if raw, err := h.resultCache.Get(ctx, cacheKey).Bytes(); err == nil && len(raw) > 0 {
		var cached models.ScrapeResult
		if jsonErr := json.Unmarshal(raw, &cached); jsonErr == nil {
			zlog.Debug().Str("key", cacheKey).Msg("scrape module result cache hit")
			return &cached, nil
		}
	}

	// Cache miss — run live.
	result, err := mod.Run(ctx, h.scraper, req)
	if err != nil {
		return nil, err
	}

	// Store in cache with MaxAge TTL.
	if encoded, jsonErr := json.Marshal(result); jsonErr == nil {
		ttl := time.Duration(req.MaxAge) * time.Millisecond
		if setErr := h.resultCache.Set(ctx, cacheKey, encoded, ttl).Err(); setErr != nil {
			zlog.Warn().Err(setErr).Str("key", cacheKey).Msg("failed to cache scrape module result")
		} else {
			zlog.Info().Str("key", cacheKey).Dur("ttl", ttl).Msg("scrape module result cached")
		}
	}
	return result, nil
}

func (h *Handler) resolveModule(moduleName string) (modules.Module, string, bool) {
	selected := strings.ToLower(strings.TrimSpace(moduleName))
	if selected == "" {
		selected = "multi"
	}

	if h.moduleRegistry == nil {
		return nil, selected, false
	}

	module, ok := h.moduleRegistry.MustGetOrDefault(selected, "multi")
	if !ok {
		return nil, selected, false
	}
	return module, selected, true
}

func (h *Handler) listModules(c *fiber.Ctx) error {
	if h.moduleRegistry == nil {
		return c.JSON(fiber.Map{"success": true, "modules": []string{}})
	}
	return c.JSON(fiber.Map{"success": true, "modules": h.moduleRegistry.Names(), "default": "multi"})
}

func (h *Handler) ready(c *fiber.Ctx) error {
	status := map[string]string{"api": "ready"}
	healthy := true

	if h.cfg != nil {
		switch strings.ToLower(strings.TrimSpace(h.cfg.CacheBackend)) {
		case "memory", "":
			status["cache"] = "ready"
		case "disk":
			if _, err := os.Stat(h.cfg.CachePath); err == nil {
				status["cache"] = "ready"
			} else {
				healthy = false
				status["cache"] = "unavailable"
			}
		case "redis":
			status["cache"] = "configured"
		default:
			status["cache"] = "unknown"
		}

		if h.cfg.RedisURL != "" && h.cfg.JobStoreBackend == "redis" {
			opt, err := redis.ParseURL(h.cfg.RedisURL)
			if err != nil {
				healthy = false
				status["redis"] = "invalid config"
			} else {
				client := redis.NewClient(opt)
				ctx, cancel := context.WithTimeout(c.UserContext(), 2*time.Second)
				defer cancel()
				if pingErr := client.Ping(ctx).Err(); pingErr != nil {
					healthy = false
					status["redis"] = "unreachable"
				} else {
					status["redis"] = "ready"
				}
				_ = client.Close()
			}
		}

		if strings.EqualFold(strings.TrimSpace(h.cfg.JobStoreBackend), "postgres") {
			dsn := strings.TrimSpace(h.cfg.PostgresDSN)
			if dsn == "" {
				// Postgres not configured — degraded but not fatal.
				status["postgres"] = "not configured"
			} else {
				pgCtx, pgCancel := context.WithTimeout(c.UserContext(), 2*time.Second)
				defer pgCancel()
				pool, pgErr := pgxpool.New(pgCtx, dsn)
				if pgErr != nil {
					status["postgres"] = "invalid config"
				} else {
					if pingErr := pool.Ping(pgCtx); pingErr != nil {
						status["postgres"] = "unreachable"
					} else {
						status["postgres"] = "ready"
					}
					pool.Close()
				}
			}
		}

		if strings.EqualFold(strings.TrimSpace(h.cfg.ArtifactStoreBackend), "minio") {
			if h.artifactStore != nil && h.artifactStore.Enabled() {
				status["artifacts"] = "ready"
			} else {
				// Artifact store is optional — degraded but not fatal.
				status["artifacts"] = "degraded"
			}
		}

		healthCtx, healthCancel := context.WithTimeout(c.UserContext(), 2*time.Second)
		defer healthCancel()
		aiHealthURL := strings.TrimRight(h.cfg.AICoreHTTPBaseURL, "/") + "/health"
		aiHealthReq, aiHealthErr := http.NewRequestWithContext(healthCtx, http.MethodGet, aiHealthURL, nil)
		if aiHealthErr == nil {
			if aiHealthResp, aiDoErr := http.DefaultClient.Do(aiHealthReq); aiDoErr == nil {
				_ = aiHealthResp.Body.Close()
				if aiHealthResp.StatusCode == http.StatusOK {
					status["ai_core"] = "ready"
				} else {
					status["ai_core"] = "degraded"
				}
			} else {
				status["ai_core"] = "degraded"
			}
		} else {
			status["ai_core"] = "degraded"
		}
	}

	if !healthy {
		// Return 503 only when a configured dependency is actively unreachable,
		// not when optional services are simply unconfigured.
		return c.Status(http.StatusServiceUnavailable).JSON(fiber.Map{"success": false, "status": status})
	}
	// All configured dependencies are up; unconfigured ones noted as degraded.
	return c.Status(http.StatusOK).JSON(fiber.Map{"success": true, "status": status})
}

func (h *Handler) metricsEndpoint(c *fiber.Ctx) error {
	aiReliability := map[string]interface{}{"enabled": false}
	aiEfficiency := map[string]interface{}{"enabled": false}
	if h.scraper != nil {
		aiReliability = h.scraper.AIReliabilitySnapshot()
		aiEfficiency = h.scraper.AIEfficiencySnapshot()
	}

	cacheHitRate := 0.0
	if raw, ok := aiEfficiency["hit_ratio"]; ok {
		switch value := raw.(type) {
		case float64:
			cacheHitRate = value
		case float32:
			cacheHitRate = float64(value)
		}
	}

	if h.metrics == nil {
		return c.JSON(fiber.Map{
			"success": true,
			"metrics": map[string]interface{}{
				"requests_per_sec":     0,
				"avg_response_time_ms": 0,
				"cache_hit_rate":       cacheHitRate,
				"error_rate":           0,
			},
			"ai":            aiReliability,
			"ai_efficiency": aiEfficiency,
		})
	}

	base := h.metrics.Snapshot()
	return c.JSON(fiber.Map{
		"success": true,
		"metrics": map[string]interface{}{
			"requests_per_sec":     base["requests_per_sec"],
			"avg_response_time_ms": base["avg_latency_ms"],
			"cache_hit_rate":       cacheHitRate,
			"error_rate":           base["error_rate"],
			"requests_total":       base["requests_total"],
			"errors_total":         base["errors_total"],
		},
		"ai":            aiReliability,
		"ai_efficiency": aiEfficiency,
	})
}

func BuildScraperFromEnv() (*scraper.Scraper, error) {
	cfg, err := config.Load()
	if err != nil {
		return nil, err
	}
	// Note: This helper doesn't initialize AI client - use main.go pattern for production
	return scraper.New(cfg, nil)
}

func inferCollection(inputURL string) string {
	if inputURL == "" {
		return ""
	}
	parts := strings.Split(strings.Trim(inputURL, "/"), "/")
	for i, p := range parts {
		if p == "produktkategori" || p == "collections" || p == "category" || p == "categories" {
			if i+1 < len(parts) {
				return parts[i+1]
			}
		}
	}

	// Fallback: extract domain name as collection identifier.
	// e.g. https://www.example.com/about → "example-com"
	for _, part := range parts {
		if strings.Contains(part, ".") && !strings.HasPrefix(part, "http") {
			domain := strings.TrimPrefix(part, "www.")
			// Replace dots with dashes for a clean collection name
			domain = strings.ReplaceAll(domain, ".", "-")
			return domain
		}
	}
	return ""
}

// enrichChangeResultWithAI calls AnalyzeDiff in a short goroutine to attach an
// AI-generated interpretation to the change result. It mutates changeResult in
// place. Because the response has already been computed, this call runs on a
// detached context with a short deadline rather than the request context so the
// API response is never blocked.
func (h *Handler) enrichChangeResultWithAI(reqCtx context.Context, cr *models.ChangeTrackingResult, targetURL string, req *models.ChangeTrackingRequest) {
	if cr == nil || cr.ChangeStatus == "same" || cr.ChangeStatus == "new" {
		return // No meaningful diff to analyse.
	}
	if cr.Diff == nil || strings.TrimSpace(cr.Diff.Text) == "" {
		return
	}
	if h.scraper == nil || h.scraper.AIClient() == nil {
		return
	}

	diffText := cr.Diff.Text
	schema := ""
	orgID := ""

	aiCli := h.scraper.AIClient()
	go func() {
		analyCtx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
		defer cancel()
		analysis, err := aiCli.AnalyzeDiff(analyCtx, diffText, schema, targetURL, orgID)
		if err != nil {
			return // Graceful degradation — AI unavailable.
		}
		cr.AIAnalysis = &models.DiffAIAnalysis{
			Summary:      analysis.Summary,
			FieldChanges: toDiffFieldChanges(analysis.FieldChanges),
		}
	}()
}

// toDiffFieldChanges maps ai.DiffFieldChange to models.DiffFieldChange.
func toDiffFieldChanges(in []ai.DiffFieldChange) []models.DiffFieldChange {
	if len(in) == 0 {
		return nil
	}
	out := make([]models.DiffFieldChange, len(in))
	for i, fc := range in {
		out[i] = models.DiffFieldChange{
			FieldPath:  fc.FieldPath,
			ChangeType: fc.ChangeType,
			OldValue:   fc.OldValue,
			NewValue:   fc.NewValue,
		}
	}
	return out
}

func buildChangeTrackingPayload(result *models.ScrapeResult, outputs map[string]interface{}) (string, error) {
	if outputs != nil {
		if markdown, ok := outputs["markdown"].(string); ok {
			trimmed := strings.TrimSpace(markdown)
			if trimmed != "" {
				return trimmed, nil
			}
		}
		if html, ok := outputs["html"].(string); ok {
			trimmed := strings.TrimSpace(html)
			if trimmed != "" {
				return trimmed, nil
			}
		}
	}

	if result == nil {
		return "{}", nil
	}

	type trackedProduct struct {
		SKU           string `json:"sku,omitempty"`
		URL           string `json:"url,omitempty"`
		Name          string `json:"name,omitempty"`
		Brand         string `json:"brand,omitempty"`
		CurrentPrice  int    `json:"current_price,omitempty"`
		OriginalPrice int    `json:"original_price,omitempty"`
		OnSale        bool   `json:"on_sale,omitempty"`
		InStock       bool   `json:"in_stock,omitempty"`
		Description   string `json:"description,omitempty"`
		UseCase       string `json:"use_case,omitempty"`
		Ingredients   string `json:"ingredients,omitempty"`
	}

	tracked := make([]trackedProduct, 0, len(result.Products))
	for _, p := range result.Products {
		if p == nil {
			continue
		}
		tracked = append(tracked, trackedProduct{
			SKU:           p.SKU,
			URL:           p.URL,
			Name:          p.Name,
			Brand:         p.Brand,
			CurrentPrice:  p.CurrentPrice,
			OriginalPrice: p.OriginalPrice,
			OnSale:        p.OnSale,
			InStock:       p.InStock,
			Description:   p.Description,
			UseCase:       p.UseCase,
			Ingredients:   p.Ingredients,
		})
	}

	sort.Slice(tracked, func(i, j int) bool { return tracked[i].URL < tracked[j].URL })

	payload := map[string]interface{}{
		"count":    result.Count,
		"products": tracked,
	}

	b, err := json.Marshal(payload)
	if err != nil {
		return "", err
	}
	return string(b), nil
}

func toActionSteps(input []models.ActionRequest) []actions.ActionStep {
	if len(input) == 0 {
		return []actions.ActionStep{}
	}
	steps := make([]actions.ActionStep, 0, len(input))
	for _, item := range input {
		steps = append(steps, actions.ActionStep{
			Type:         item.Type,
			Selector:     item.Selector,
			Text:         item.Text,
			Key:          item.Key,
			Script:       item.Script,
			Milliseconds: item.Milliseconds,
			Direction:    item.Direction,
			FullPage:     item.FullPage,
			Retry:        item.Retry,
			AfterShot:    item.AfterShot,
		})
	}
	return steps
}

func toActionExecutionResults(input []actions.ActionResult) []models.ActionExecutionResult {
	if len(input) == 0 {
		return []models.ActionExecutionResult{}
	}
	results := make([]models.ActionExecutionResult, 0, len(input))
	for _, item := range input {
		results = append(results, models.ActionExecutionResult{
			Type:       item.Type,
			Success:    item.Success,
			DurationMs: item.DurationMs,
			Error:      item.Error,
			Output:     item.Output,
		})
	}
	return results
}

// fireWebhook delivers a webhook payload with exponential-backoff retry (3 attempts).
// Each attempt has a 10-second timeout. Delays: 1 s → 2 s between attempts.
// Called from a goroutine; never blocks the caller.
func (h *Handler) fireWebhook(webhookURL string, payload *models.WebhookPayload) {
	if h.batchManager == nil {
		return
	}
	const maxAttempts = 3
	backoff := time.Second
	for attempt := 1; attempt <= maxAttempts; attempt++ {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		err := h.batchManager.DeliverWebhook(ctx, webhookURL, payload)
		cancel()
		if err == nil {
			return
		}
		zlog.Warn().Err(err).
			Str("webhook_url", webhookURL).
			Int("attempt", attempt).
			Int("max_attempts", maxAttempts).
			Msg("webhook delivery failed")
		if attempt < maxAttempts {
			time.Sleep(backoff)
			backoff *= 2
		}
	}
	zlog.Error().
		Str("webhook_url", webhookURL).
		Msg("webhook delivery failed after all attempts")
}
