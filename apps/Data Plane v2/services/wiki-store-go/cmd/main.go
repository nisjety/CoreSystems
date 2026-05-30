package main

import (
	"context"
	"fmt"
	"net"
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
	"google.golang.org/grpc"

	wikipb "github.com/triodelab/dataplane/gen/go/wiki/v1"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/config"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/events"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/grpcserver"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/handler"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/metrics"
	apmotel "github.com/triodelab/dataplane/services/wiki-store-go/internal/otel"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/repo"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "wiki-store-go").Logger()

	cfg := config.Load()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	otelShutdown, err := apmotel.Init(ctx, "wiki-store-go")
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

	wikiRepo := repo.NewWikiRepo(pool)

	// §16.3.8 — attach a NATS publisher so CreatePage / CreateVersion emit
	// `dataplane.wiki.version.published`. Empty NatsURL keeps the repo
	// silent (publisher = nil), useful in local dev without NATS.
	if cfg.NatsURL != "" {
		nc, err := nats.Connect(cfg.NatsURL)
		if err != nil {
			log.Warn().Err(err).Str("url", cfg.NatsURL).Msg("NATS connect failed; wiki publish events disabled")
		} else {
			defer nc.Close()
			wikiRepo.SetPublisher(events.NewPublisher(nc))
			log.Info().Str("url", cfg.NatsURL).Msg("wiki publisher attached")
		}
	}

	wikiHandler := handler.NewWikiHandler(wikiRepo)

	r := chi.NewRouter()
	r.Use(chimw.RequestID)
	r.Use(chimw.RealIP)
	r.Use(chimw.Recoverer)
	r.Use(chimw.Timeout(30 * time.Second))
	r.Use(metrics.Middleware)

	r.Get("/health", handler.Health)
	r.Get("/readyz", handler.Readyz)
	r.Method("GET", "/metrics", metrics.Handler())

	r.Route("/v1/wiki", func(r chi.Router) {
		r.Use(handler.OrgIDMiddleware)
		r.Post("/pages", wikiHandler.CreatePage)
		// Wave 3.1 / Wave 11.C-b — paginated list-all-pages for velion sidebar.
		r.Get("/pages", wikiHandler.ListPages)
		r.Get("/pages/by-path", wikiHandler.GetPageByPath)
		r.Get("/pages/{pageID}", wikiHandler.GetPage)
		r.Post("/pages/{pageID}/versions", wikiHandler.UpdateVersion)
		r.Get("/pages/{pageID}/versions", wikiHandler.ListVersions)
		r.Get("/pages/{pageID}/diff", wikiHandler.DiffVersions)
		r.Get("/pages/{pageID}/backlinks", wikiHandler.GetBacklinks)
		r.Post("/pages/{pageID}/proposals", wikiHandler.SubmitProposal)
		r.Post("/proposals/review", wikiHandler.ReviewProposal)
		r.Post("/pages/{pageID}/source-logs", wikiHandler.CreateSourceLog)
		r.Get("/pages/{pageID}/source-logs", wikiHandler.ListSourceLogs)
		r.Post("/pages/{pageID}/maintenance-logs", wikiHandler.CreateMaintenanceLog)
		r.Get("/pages/{pageID}/maintenance-logs", wikiHandler.ListMaintenanceLogs)
		// D4+D5 spec §3.4: batch ingest of lint findings (orphan/stale/weak-citation/contradiction).
		r.Post("/maintenance/sweep", wikiHandler.MaintenanceSweep)
	})

	addr := fmt.Sprintf("0.0.0.0:%d", cfg.HTTPPort)
	srv := &http.Server{Addr: addr, Handler: r}

	go func() {
		log.Info().Str("addr", addr).Msg("wiki-store-go HTTP starting")
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatal().Err(err).Msg("http server error")
		}
	}()

	// gRPC server on a separate port — same WikiRepo backs both wires.
	grpcAddr := fmt.Sprintf("0.0.0.0:%d", cfg.GRPCPort)
	grpcLis, gerr := net.Listen("tcp", grpcAddr)
	if gerr != nil {
		log.Fatal().Err(gerr).Str("addr", grpcAddr).Msg("grpc listen failed")
	}
	grpcSrv := grpc.NewServer()
	wikipb.RegisterWikiServiceServer(grpcSrv, grpcserver.New(wikiRepo))
	go func() {
		log.Info().Str("addr", grpcAddr).Msg("wiki-store-go gRPC starting")
		if err := grpcSrv.Serve(grpcLis); err != nil {
			log.Error().Err(err).Msg("grpc server error")
		}
	}()
	defer grpcSrv.GracefulStop()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	log.Info().Msg("shutting down")
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	srv.Shutdown(shutdownCtx)
}
