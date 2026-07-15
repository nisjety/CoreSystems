package main

import (
	"context"
	"net"
	"os"
	"os/signal"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/clients"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/config"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/convex"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/database"
	internalhttp "github.com/I-Dacosta/CoreSystem/apps/session-core/internal/http"
	metricsserver "github.com/I-Dacosta/CoreSystem/apps/session-core/internal/metrics"
	internalnats "github.com/I-Dacosta/CoreSystem/apps/session-core/internal/nats"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/redis"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/repository"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/service"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/subscribers"
)

func main() {
	cfg := config.Load()

	level, err := zerolog.ParseLevel(cfg.Logging.Level)
	if err != nil {
		level = zerolog.InfoLevel
	}
	zerolog.SetGlobalLevel(level)
	if cfg.Logging.Format == "console" {
		log.Logger = log.Output(zerolog.ConsoleWriter{Out: os.Stderr, TimeFormat: time.RFC3339})
	}
	log.Info().
		Str("service", "session-core").
		Str("http_port", cfg.Server.HTTPPort).
		Msg("Starting session-core")

	if err := cfg.ValidateScopedServiceTokens(); err != nil {
		log.Fatal().Err(err).Msg("[session-core startup] scoped upstream credential validation failed")
	}

	ctx := context.Background()

	db, err := database.Connect(ctx, cfg.Database.URL_DSN())
	if err != nil {
		log.Fatal().Err(err).Msg("Failed to connect to database")
	}
	defer db.Close()

	if err := db.RunMigrations(ctx, "migrations"); err != nil {
		log.Fatal().Err(err).Msg("Failed to run migrations")
	}
	log.Info().Msg("Database connected and migrations applied")

	natsLocal, err := internalnats.NewClient(cfg.NATS.LocalURL, internalnats.Credentials{
		User: cfg.NATS.LocalUser, Password: cfg.NATS.LocalPassword,
		Token: cfg.NATS.Token, AllowTokenFallback: cfg.NATS.AllowTokenFallback,
		InboxPrefix: "_INBOX.SESSION_CONTROL",
	})
	if err != nil {
		log.Warn().Err(err).Msg("Failed to connect local NATS; proceeding without local pub/sub")
	}

	var natsShared *internalnats.SharedPublisher
	var natsSharedClient *internalnats.Client
	if cfg.NATS.SharedURL != "" {
		sharedClient, err := internalnats.NewClient(cfg.NATS.SharedURL, internalnats.Credentials{
			User: cfg.NATS.SharedUser, Password: cfg.NATS.SharedPassword,
			Token: cfg.NATS.SharedToken, AllowTokenFallback: cfg.NATS.SharedAllowTokenFallback,
			InboxPrefix: "_INBOX.SESSION_SHARED",
		})
		if err != nil {
			log.Warn().Err(err).Msg("Failed to connect shared NATS; v2 routing disabled")
		} else {
			natsSharedClient = sharedClient
			natsShared = internalnats.NewSharedPublisher(sharedClient)
			log.Info().Msg("Shared NATS connected (streams provisioned externally)")
		}
	}

	var cache *redis.Client
	if cfg.Redis.Addr != "" {
		cache = redis.NewClient(cfg.Redis.Addr, cfg.Redis.Password, cfg.Redis.DB)
		log.Info().Str("addr", cfg.Redis.Addr).Msg("Redis cache connected")
	}

	repo := repository.NewSessionRepository(db.Pool())
	// G36-cutover Step D (2026-05-12): plan/todo/lineage/approval repos
	// removed. Rust session-core (`Model Plane/rust/services/session-core`)
	// owns the agent-run state via its `orchestration_store` module; the
	// HTTP surface lives on port 28083:8083 (`/v1/{plans,todos,lineage}`).
	// CP session-core now hosts only the Control Session aggregator
	// (Wave 3 §8.17) — `repo` (session_repository.go) is retained because
	// it backs the aggregator, not the agent-run routes.
	convexClient := convex.NewClient(cfg.Convex.URL, cfg.Convex.ServiceKey)
	if convexClient != nil {
		log.Info().Str("url", cfg.Convex.URL).Msg("Convex sync enabled")
	}
	orgClient := clients.NewOrgClient(cfg.OrgCore.URL, cfg.OrgCore.ServiceToken)
	if orgClient != nil {
		log.Info().Str("url", cfg.OrgCore.URL).Msg("Org membership validation enabled")
	}
	sessionService := service.NewSessionService(repo, natsLocal, natsShared, cache, cfg.NATS.ModelPlaneV2RolloutPct, convexClient, orgClient)

	// G10: Control Session aggregator clients + service.
	userClient := clients.NewUserClient(cfg.UserCore.URL, cfg.UserCore.ServiceToken)
	if userClient != nil {
		log.Info().Str("url", cfg.UserCore.URL).Msg("user-core client enabled for Control Session aggregator")
	}
	billingClient := clients.NewBillingClient(cfg.BillingCore.URL, cfg.BillingCore.ServiceToken)
	if billingClient != nil {
		log.Info().Str("url", cfg.BillingCore.URL).Msg("billing-core client enabled for Control Session aggregator")
	}
	controlSessionService := service.NewControlSessionService(userClient, orgClient, billingClient, natsShared, cache, convexClient)
	if cache != nil {
		log.Info().Dur("ttl", service.ControlSessionCacheTTL).Msg("Control Session read-through cache enabled (G34)")
	}
	if convexClient != nil {
		log.Info().Msg("Control Session Convex mirror enabled (G35)")
	}
	log.Info().Msg("Control Session aggregator ready (GET /api/v1/sessions/current)")

	// G34-followup: subscribe to upstream user/org/billing events on the
	// shared bus so cache entries get busted reactively (not just on
	// explicit /refresh or TTL expiry). When user-core or billing-core
	// publish on the same NATS connection, the queue subscriber picks the
	// event up and invalidates the affected (user, org) snapshot, then
	// republishes `app.session.entitlements_changed` so notification-core
	// fires its toast.
	var upstreamSub *subscribers.UpstreamInvalidator
	if natsSharedClient != nil && cache != nil {
		upstreamSub = subscribers.NewUpstreamInvalidator(natsSharedClient, cache, natsShared)
		if err := upstreamSub.Start(ctx); err != nil {
			log.Warn().Err(err).Msg("Failed to start upstream invalidator subscribers")
		}
	}

	httpServer := internalhttp.NewServer(sessionService, controlSessionService, natsShared, cache, cfg.Server.HTTPPort)

	go func() {
		if err := httpServer.Start(); err != nil {
			log.Error().Err(err).Msg("HTTP server error")
		}
	}()

	// Prometheus /metrics on a dedicated port (default 9091), scraped by the
	// Control-Plane Prometheus (Phase 6 B13).
	metricsPort := 9091
	if v := strings.TrimSpace(os.Getenv("METRICS_PORT")); v != "" {
		if n, convErr := strconv.Atoi(v); convErr == nil && n > 0 {
			metricsPort = n
		}
	}
	metricsServer := metricsserver.NewServer(metricsPort)
	go func() {
		if err := metricsServer.Start(); err != nil {
			log.Error().Err(err).Msg("metrics server error")
		}
	}()

	if cfg.Server.GRPCPort != "" {
		go func() {
			lis, err := net.Listen("tcp", ":"+cfg.Server.GRPCPort)
			if err != nil {
				log.Error().Err(err).Msg("Failed to listen for gRPC")
				return
			}
			_ = lis
			log.Info().Str("port", cfg.Server.GRPCPort).Msg("gRPC listener ready (no services registered yet)")
		}()
	}

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	sig := <-quit
	log.Info().Str("signal", sig.String()).Msg("Shutting down session-core")

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	if upstreamSub != nil {
		upstreamSub.Stop()
	}
	if err := httpServer.Shutdown(shutdownCtx); err != nil {
		log.Error().Err(err).Msg("HTTP server shutdown failed")
	}
	if err := metricsServer.Shutdown(shutdownCtx); err != nil {
		log.Error().Err(err).Msg("metrics server shutdown failed")
	}
	if natsLocal != nil {
		natsLocal.Close()
	}
	if natsSharedClient != nil {
		natsSharedClient.Close()
	}
	if natsShared != nil {
		natsShared.Close()
	}
	if cache != nil {
		cache.Close()
	}

	log.Info().Msg("Session-core stopped")
}
