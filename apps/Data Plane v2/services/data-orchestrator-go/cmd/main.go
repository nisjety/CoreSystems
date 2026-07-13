package main

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"
	chimw "github.com/go-chi/chi/v5/middleware"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/config"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/cost"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/handler"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/jobs"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/metrics"
	apmotel "github.com/triodelab/dataplane/services/data-orchestrator-go/internal/otel"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "data-orchestrator-go").Logger()

	cfg := config.Load()
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audience:      cfg.JWTAudience,
		Issuer:        cfg.JWTIssuer,
		PublicKeyFile: cfg.JWTPublicKeyFile,
		JWKSURL:       cfg.JWKSURL,
	})
	if err != nil {
		log.Fatal().Err(err).Msg("JWT verification configuration invalid")
	}
	authMiddleware := authctx.Middleware(verifier)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	otelShutdown, err := apmotel.Init(ctx, "data-orchestrator-go")
	if err != nil {
		log.Warn().Err(err).Msg("otel init failed, continuing without tracing")
	} else {
		defer otelShutdown(ctx)
	}

	pool, err := pgxpool.New(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatal().Err(err).Msg("postgres connect failed")
	}
	defer pool.Close()

	if err := pool.Ping(ctx); err != nil {
		log.Fatal().Err(err).Msg("postgres ping failed")
	}

	legacyEventsEnabled := unverifiedLegacyEventsEnabled()
	var nc *nats.Conn
	if legacyEventsEnabled {
		natsOptions := []nats.Option{nats.Name("data-orchestrator")}
		if cfg.NatsToken != "" {
			natsOptions = append(natsOptions, nats.Token(cfg.NatsToken))
		}
		nc, err = nats.Connect(cfg.NatsURL, natsOptions...)
		if err != nil {
			log.Fatal().Err(err).Msg("nats connect failed")
		}
		defer nc.Close()
	}

	executor := jobs.NewExecutor(pool, nc, legacyEventsEnabled)
	staleDetector := jobs.NewStaleDetector(pool)
	orchHandler := handler.NewOrchestratorHandler(executor, staleDetector)

	// Unsigned legacy cost events can select arbitrary tenants and values. Keep
	// the mutation consumer fail-closed until producer-scoped signed envelopes
	// and NATS subject ACLs are deployed.
	if legacyEventsEnabled {
		log.Warn().Msg("unsigned cost ledger consumer enabled for insecure development")
		costConsumer := cost.NewConsumer(pool, nc)
		if cleanup, err := costConsumer.Start(ctx); err != nil {
			log.Warn().Err(err).Msg("cost ledger consumer failed to start; continuing without")
		} else {
			defer cleanup()
		}
	} else {
		log.Warn().Msg("cost ledger consumer disabled until signed producer-scoped envelopes are available")
	}

	r := chi.NewRouter()
	r.Use(chimw.RequestID)
	r.Use(chimw.RealIP)
	r.Use(chimw.Recoverer)
	r.Use(chimw.Timeout(60 * time.Second))
	r.Use(metrics.Middleware)

	r.Get("/health", handler.Health)
	r.Get("/readyz", handler.Readyz)
	r.Method("GET", "/metrics", metrics.Handler())

	handler.MountProtectedRoutes(r, authMiddleware, orchHandler)

	addr := fmt.Sprintf("0.0.0.0:%d", cfg.HTTPPort)
	srv := &http.Server{Addr: addr, Handler: r}

	go func() {
		log.Info().Str("addr", addr).Msg("data-orchestrator-go starting")
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatal().Err(err).Msg("http server error")
		}
	}()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	log.Info().Msg("shutting down")
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	srv.Shutdown(shutdownCtx)
}

func unverifiedLegacyEventsEnabled() bool {
	return os.Getenv("ALLOW_UNVERIFIED_LEGACY_EVENTS") == "1" &&
		os.Getenv("ALLOW_INSECURE_DEV_DEFAULTS") == "1" &&
		os.Getenv("ISOLATED_E2E") == "1"
}
