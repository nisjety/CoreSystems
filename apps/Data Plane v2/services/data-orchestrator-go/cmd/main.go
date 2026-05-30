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

	nc, err := nats.Connect(cfg.NatsURL)
	if err != nil {
		log.Fatal().Err(err).Str("url", cfg.NatsURL).Msg("nats connect failed")
	}
	defer nc.Close()

	executor := jobs.NewExecutor(pool, nc)
	staleDetector := jobs.NewStaleDetector(pool)
	orchHandler := handler.NewOrchestratorHandler(executor, staleDetector)

	// Cost ledger consumer: subscribes to dataplane.cost.ledger and persists.
	costConsumer := cost.NewConsumer(pool, nc)
	if cleanup, err := costConsumer.Start(ctx); err != nil {
		log.Warn().Err(err).Msg("cost ledger consumer failed to start; continuing without")
	} else {
		defer cleanup()
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

	r.Route("/v1/orchestrator", func(r chi.Router) {
		r.Use(handler.OrgIDMiddleware)
		r.Post("/jobs", orchHandler.CreateJob)
		r.Post("/reindex", orchHandler.Reindex)
		r.Get("/stale-embeddings", orchHandler.StaleEmbeddings)
	})

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
