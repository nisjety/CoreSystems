// audit-core entry point.
//
// Subscribes to NATS audit + usage subjects, persists to Postgres, and
// serves a read API at /v1/audit, /v1/usage, /v1/usage/summary. All
// reads are org-scoped (`?org_id=...` required) — the multi-tenant
// trust contract assumes the caller (velion) attaches the verified
// org_id from the auth-core JWT.
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
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/api"
	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/subscriber"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "audit-core").Logger()

	cfg, err := loadConfig()
	if err != nil {
		log.Fatal().Err(err).Msg("config load failed")
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	pool, err := pgxpool.New(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatal().Err(err).Msg("postgres connect failed")
	}
	defer pool.Close()
	if err := pool.Ping(ctx); err != nil {
		log.Fatal().Err(err).Msg("postgres ping failed")
	}

	if err := store.Migrate(ctx, pool); err != nil {
		log.Fatal().Err(err).Msg("schema migration failed")
	}
	log.Info().Msg("schema migrated (idempotent CREATE IF NOT EXISTS)")

	st := store.New(pool)

	natsOpts := []nats.Option{nats.Name("audit-core")}
	if cfg.NATSToken != "" {
		natsOpts = append(natsOpts, nats.Token(cfg.NATSToken))
	}
	nc, err := nats.Connect(cfg.NATSURL, natsOpts...)
	if err != nil {
		log.Fatal().Err(err).Str("nats_url", cfg.NATSURL).Msg("nats connect failed")
	}
	defer func() { _ = nc.Drain() }()

	sub := subscriber.New(nc, st)
	if err := sub.Start(ctx); err != nil {
		log.Fatal().Err(err).Msg("subscriber start failed")
	}

	r := chi.NewRouter()
	api.New(st, cfg.InternalAPIKey).Mount(r)

	addr := fmt.Sprintf("0.0.0.0:%d", cfg.HTTPPort)
	srv := &http.Server{Addr: addr, Handler: r, ReadHeaderTimeout: 5 * time.Second}
	go func() {
		log.Info().Str("addr", addr).Msg("audit-core http listening")
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
	_ = srv.Shutdown(shutdownCtx)
}

type config struct {
	DatabaseURL    string
	NATSURL        string
	NATSToken      string
	HTTPPort       int
	InternalAPIKey string
}

func loadConfig() (*config, error) {
	dsn := os.Getenv("DATABASE_URL")
	if dsn == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	natsURL := os.Getenv("NATS_URL")
	if natsURL == "" {
		natsURL = "nats://velion-nats:4222"
	}
	port := 8087
	if v := os.Getenv("HTTP_PORT"); v != "" {
		var p int
		if _, err := fmt.Sscanf(v, "%d", &p); err == nil && p > 0 {
			port = p
		}
	}
	return &config{
		DatabaseURL:    dsn,
		NATSURL:        natsURL,
		NATSToken:      os.Getenv("NATS_TOKEN"),
		HTTPPort:       port,
		InternalAPIKey: os.Getenv("INTERNAL_API_KEY"),
	}, nil
}
