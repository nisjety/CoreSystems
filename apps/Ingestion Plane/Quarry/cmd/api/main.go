package main

import (
	"context"
	"errors"
	stdlog "log"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/gofiber/adaptor/v2"
	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/cors"
	"github.com/gofiber/fiber/v2/middleware/logger"
	"github.com/gofiber/fiber/v2/middleware/recover"
	"github.com/gofiber/fiber/v2/middleware/requestid"
	"github.com/joho/godotenv"
	"github.com/prometheus/client_golang/prometheus/promhttp"
	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/api"
	quarrybrowser "github.com/triodelab/quarry/internal/browser"
	"github.com/triodelab/quarry/internal/config"
	quarrymiddleware "github.com/triodelab/quarry/internal/middleware"
	"github.com/triodelab/quarry/internal/nats"
	"github.com/triodelab/quarry/internal/platform"
	"github.com/triodelab/quarry/internal/scraper"
	"github.com/triodelab/quarry/internal/workerqueue"
)

func main() {
	// Load .env file if it exists
	if err := godotenv.Load(); err != nil {
		zlog.Warn().Err(err).Msg("No .env file found (using defaults/environment)")
	}

	cfg, err := config.Load()
	if err != nil {
		stdlog.Fatalf("failed to load config: %v", err)
	}

	// ── Observability (OTel + Prometheus) ────────────────────────────────────
	otelProvider, otelErr := platform.InitOTel(context.Background(), "quarry-api", "1.0.0")
	if otelErr != nil {
		zlog.Warn().Err(otelErr).Msg("otel init failed, metrics disabled")
	}

	app := fiber.New(fiber.Config{
		BodyLimit: cfg.MaxRequestBodyBytes,
		ErrorHandler: func(c *fiber.Ctx, err error) error {
			var fiberErr *fiber.Error
			status := fiber.StatusInternalServerError
			message := "internal server error"
			if errors.As(err, &fiberErr) {
				status = fiberErr.Code
				if strings.TrimSpace(fiberErr.Message) != "" {
					message = fiberErr.Message
				}
			}
			return api.WriteError(c, status, message, nil)
		},
	})
	app.Use(recover.New())
	app.Use(cors.New())
	app.Use(requestid.New())
	app.Use(logger.New(logger.Config{
		Format: "${time} ${status} - ${latency} ${method} ${path} reqid=${locals:requestid}\n",
	}))
	app.Use(quarrymiddleware.RequestContextTimeout(time.Duration(cfg.RequestTimeoutSec) * time.Second))
	app.Use(quarrymiddleware.Timing(quarrymiddleware.TimingConfig{
		Enabled:  cfg.HumanDelayEnabled,
		MinDelay: time.Duration(cfg.HumanDelayMinMs) * time.Millisecond,
		MaxDelay: time.Duration(cfg.HumanDelayMaxMs) * time.Millisecond,
	}))
	app.Use(quarrymiddleware.ProxyRotation(quarrymiddleware.ProxyConfig{
		Enabled: cfg.ProxyEnabled,
		Pool:    cfg.ProxyPool,
	}))
	app.Use(quarrymiddleware.CustomRetry(quarrymiddleware.RetryConfig{
		Enabled:     cfg.RetryEnabled,
		MaxRetries:  cfg.RetryMaxAttempts,
		BaseBackoff: time.Duration(cfg.RetryBackoffMs) * time.Millisecond,
	}))
	app.Use(quarrymiddleware.JobMonitoring(quarrymiddleware.MonitoringConfig{
		Enabled: cfg.JobMonitoring,
	}))

	// ── Control-plane auth bridge ────────────────────────────────────────────
	// Resolves caller identity via auth-core Bearer token or static API key.
	var authClient *platform.AuthClient
	var controlPlaneService *platform.ControlPlaneService
	if cfg.AuthCoreURL != "" && cfg.AuthCoreInternalAPIKey != "" {
		authClient = platform.NewAuthClientWithCache(
			cfg.AuthCoreURL,
			cfg.AuthCoreInternalAPIKey,
			time.Duration(cfg.ControlPlaneCacheSec)*time.Second,
		)
		if cfg.AuthCoreGRPCAddr != "" {
			grpcCtx, grpcCancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer grpcCancel()
			if err := authClient.ConnectGRPC(grpcCtx, cfg.AuthCoreGRPCAddr); err != nil {
				zlog.Warn().Err(err).Str("addr", cfg.AuthCoreGRPCAddr).Msg("auth-core gRPC dial failed — falling back to HTTP verify")
			} else {
				zlog.Info().Str("addr", cfg.AuthCoreGRPCAddr).Msg("auth-core gRPC connected — VerifyToken will use gRPC")
			}
		}
		zlog.Info().Str("auth_core_url", cfg.AuthCoreURL).Msg("Auth-core bridge enabled (Bearer + API key)")
	} else {
		zlog.Warn().Msg("AUTH_CORE_URL not set — falling back to static API key only")
	}

	app.Use("/v1", platform.AuthBridgeMiddleware(platform.AuthBridgeConfig{
		AuthClient:         authClient,
		StaticAPIKey:       cfg.APIKey,
		StaticAPIKeyHeader: cfg.APIKeyHeader,
	}))

	if cfg.BillingCoreURL != "" || cfg.OrgCoreURL != "" || cfg.UserCoreURL != "" {
		controlPlaneService = platform.NewControlPlaneService(platform.ControlPlaneConfig{
			BillingBaseURL: cfg.BillingCoreURL,
			OrgBaseURL:     cfg.OrgCoreURL,
			UserBaseURL:    cfg.UserCoreURL,
			InternalAPIKey: cfg.AuthCoreInternalAPIKey,
			CacheTTL:       time.Duration(cfg.ControlPlaneCacheSec) * time.Second,
		})
		app.Use("/v1", platform.ControlPlanePolicyMiddleware(platform.ControlPlanePolicyConfig{
			Service: controlPlaneService,
		}))
	}

	// ── Tier-aware rate limiting ─────────────────────────────────────────────
	// Replaces the old single-bucket global limiter with per-org, per-tier limits.
	app.Use("/v1", platform.TierRateLimiter(platform.TierRateLimiterConfig{
		FallbackMax:    cfg.RateLimitMax,
		FallbackWindow: time.Duration(cfg.RateLimitWindowSec) * time.Second,
	}))

	// Initialize AI client (Model Plane v2 REST — sole backend)
	var aiClient ai.AIClient
	if cfg.EnableAIExtraction {
		restClient, initErr := ai.NewRESTClient(cfg.AICoreHTTPBaseURL, cfg.PublisherNATSURL(), cfg.PublisherNATSToken(), cfg.AICoreInternalAPIKey)
		if initErr != nil {
			stdlog.Fatalf("failed to initialize AI REST client: %v", initErr)
		}
		aiClient = restClient
		zlog.Info().Str("base_url", cfg.AICoreHTTPBaseURL).Msg("AI extraction enabled via Model Plane v2 REST")
	}

	scraperEngine, err := scraper.New(cfg, aiClient)
	if err != nil {
		stdlog.Fatalf("failed to initialize scraper: %v", err)
	}
	// cleanup handled in graceful shutdown block

	// Initialize shared NATS publisher for cross-plane events
	sharedPub, err := nats.NewSharedPublisher(
		cfg.PublisherNATSURL(),
		cfg.PublisherNATSToken(),
		"quarry-api",
	)
	if err != nil {
		zlog.Warn().Err(err).Msg("failed to initialize shared NATS publisher, continuing without event publishing")
	}

	// ── Billing client + quota gate + usage metering ─────────────────────────
	var billingClient *platform.BillingClient
	if cfg.BillingCoreURL != "" && cfg.AuthCoreInternalAPIKey != "" {
		billingClient = platform.NewBillingClient(cfg.BillingCoreURL, cfg.AuthCoreInternalAPIKey)
		zlog.Info().Str("billing_core_url", cfg.BillingCoreURL).Msg("Billing-core client enabled")

		// Quota gate — blocks POST requests when org quota is exhausted.
		app.Use("/v1", platform.QuotaGateMiddleware(platform.QuotaGateConfig{
			BillingClient: billingClient,
			Publisher:     sharedPub,
			Metric:        "crawl_credits",
		}))
	} else {
		zlog.Warn().Msg("BILLING_CORE_URL not set — quota enforcement disabled (open-core mode)")
	}

	// Usage metering — records credits after successful requests.
	app.Use("/v1", platform.UsageMeteringMiddleware(platform.UsageMeteringConfig{
		Publisher:     sharedPub,
		BillingClient: billingClient,
	}))

	h := api.NewHandler(scraperEngine, cfg, sharedPub)
	if authClient != nil {
		h.SetAuthClient(authClient)
	}
	if cfg.NATSSharedURL != "" && !strings.EqualFold(strings.TrimSpace(cfg.JobStoreBackend), "memory") {
		asyncQueue, queueErr := workerqueue.New(cfg.NATSSharedURL, cfg.NATSSharedToken, "quarry-api-dispatcher")
		if queueErr != nil {
			zlog.Warn().Err(queueErr).Msg("failed to initialize async worker dispatcher, falling back to in-process execution")
		} else if asyncQueue != nil {
			h.SetAsyncDispatcher(asyncQueue)
			zlog.Info().Str("nats_url", cfg.NATSSharedURL).Msg("async jobs configured for remote worker dispatch")
		}
	} else if cfg.NATSSharedURL != "" {
		zlog.Warn().Str("job_store_backend", cfg.JobStoreBackend).Msg("async worker dispatch disabled because JOB_STORE_BACKEND=memory would hide remote worker updates")
	}

	if cfg.BrowserServiceURL != "" {
		h.SetBrowserRuntime(quarrybrowser.NewClient(
			cfg.BrowserServiceURL,
			cfg.BrowserServiceInternalAPIKey,
			time.Duration(cfg.BrowserServiceTimeoutSec)*time.Second,
		))
		zlog.Info().Str("browser_service_url", cfg.BrowserServiceURL).Msg("interactive browser runtime configured for remote browser service")
	} else {
		zlog.Warn().Msg("BROWSER_SERVICE_URL not set; interactive browser endpoints are disabled in quarry-api")
	}

	h.Register(app)

	// ── Prometheus /metrics endpoint ─────────────────────────────────────────
	app.Get("/metrics", adaptor.HTTPHandler(promhttp.Handler()))

	port := os.Getenv("PORT")
	if port == "" {
		port = "8090"
	}
	serverErrCh := make(chan error, 1)
	go func() {
		serverErrCh <- app.Listen(":" + port)
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM)
	select {
	case <-stop:
	case err := <-serverErrCh:
		if err != nil {
			zlog.Error().Err(err).Msg("server stopped unexpectedly")
		}
		return
	}

	zlog.Info().Msg("shutting down gracefully...")

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if otelProvider != nil {
		otelProvider.Shutdown(shutdownCtx)
	}

	if err := app.ShutdownWithContext(shutdownCtx); err != nil {
		zlog.Error().Err(err).Msg("http server shutdown failed")
	}
	if err := <-serverErrCh; err != nil {
		zlog.Debug().Err(err).Msg("server loop exited after shutdown")
	}

	if err := scraperEngine.Close(); err != nil {
		zlog.Error().Err(err).Msg("scraper engine shutdown failed")
	}

	if aiClient != nil {
		if err := aiClient.Close(); err != nil {
			zlog.Error().Err(err).Msg("ai client shutdown failed")
		}
	}

	if authClient != nil {
		if err := authClient.Close(); err != nil {
			zlog.Error().Err(err).Msg("auth-core grpc shutdown failed")
		}
	}

	if err := h.Close(); err != nil {
		zlog.Error().Err(err).Msg("api handler shutdown failed")
	}

	if sharedPub != nil {
		if err := sharedPub.Close(); err != nil {
			zlog.Error().Err(err).Msg("shared NATS publisher shutdown failed")
		}
	}

	zlog.Info().Msg("server shutdown complete")
}
