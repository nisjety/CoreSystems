// Package main is the entry point for browser-broker.
// Trusted browser grant lifecycle — local/cloud mode, per-session revocation.
package main

import (
	"context"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	bbserver "github.com/triodelab/model-plane/services/browser-broker/internal/server"
	"google.golang.org/grpc"
	"google.golang.org/grpc/health"
	grpc_health_v1 "google.golang.org/grpc/health/grpc_health_v1"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("browser-broker starting")
	// Grant durability (WS1.C): a normal deployment requires the Postgres
	// grant store, so a broker restart does not silently void every live
	// session grant. The in-memory store alone is a development opt-in.
	// Copied from sandbox-manager's gate (cmd/main.go).
	ephemeralDev := ephemeralDevelopmentEnabled(os.Getenv("BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT"))
	databaseURL := strings.TrimSpace(os.Getenv("DATABASE_URL"))
	if databaseURL == "" && !ephemeralDev {
		slog.Error("refusing to start browser-broker without a durable database", "required", "DATABASE_URL", "development_override", "BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT=true")
		os.Exit(1)
	}
	verifier, err := authz.NewVerifierFromEnv()
	if err != nil {
		slog.Error("browser-broker authentication configuration is invalid", "error", err)
		os.Exit(1)
	}

	cfg, err := bbserver.ConfigFromEnv(os.Getenv)
	if err != nil {
		slog.Error("browser-broker grant configuration is invalid", "error", err)
		os.Exit(1)
	}
	// The effective limits only: never a host list or an org.
	slog.Info("browser-broker grant limits",
		"grant_ttl_seconds", int(cfg.TTL.Seconds()),
		"max_session_domains", cfg.MaxSessionDomains,
		"max_active_grants_per_owner", cfg.MaxActivePerOwner,
		"max_run_domains", cfg.MaxRunDomains,
	)

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	// Zero Data Retention grants always stay in memory (X-2): Routed sends
	// them, and every "z_" id, to this store and never to Postgres. That
	// is why the broker runs as one replica while ZDR grants exist.
	ephemeral := grant.NewMemoryRepository()
	var grants grant.Repository = ephemeral
	var purger grant.Purger = ephemeral
	var ready func(context.Context) error
	if databaseURL != "" {
		pool, err := pgxpool.New(ctx, databaseURL)
		if err != nil {
			// The parse error can quote the URL; it is not logged.
			slog.Error("browser-broker database configuration is invalid", "required", "DATABASE_URL")
			os.Exit(1)
		}
		defer pool.Close()
		pg := grant.NewPgStore(pool)
		routed := grant.NewRouted(pg, ephemeral)
		grants, purger, ready = routed, routed, pg.Ping
		pingCtx, pingCancel := context.WithTimeout(ctx, 10*time.Second)
		if err := pg.Ping(pingCtx); err != nil {
			// Not fatal: /readyz reports it until the database answers.
			slog.Warn("browser-broker database is not answering yet")
		}
		pingCancel()
		slog.Info("browser-broker using the durable Postgres grant store; ZDR grants stay in memory")
	} else {
		slog.Warn("browser-broker running with in-memory grants only (ephemeral development); every grant is lost on restart")
	}
	go purgeLoop(ctx, purger)

	// Health server on :8087
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	mux.HandleFunc("/readyz", func(w http.ResponseWriter, r *http.Request) {
		if ready != nil {
			pingCtx, pingCancel := context.WithTimeout(r.Context(), 2*time.Second)
			defer pingCancel()
			if err := ready(pingCtx); err != nil {
				w.WriteHeader(http.StatusServiceUnavailable)
				_, _ = w.Write([]byte("grant store unavailable"))
				return
			}
		}
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})

	healthServer := &http.Server{Addr: ":8087", Handler: mux}
	go func() {
		slog.Info("health server listening", "addr", ":8087")
		if err := healthServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("health server error", "error", err)
		}
	}()

	// gRPC server on :9095
	lis, err := net.Listen("tcp", ":9095")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	grpcServer := grpc.NewServer(grpc.UnaryInterceptor(authz.UnaryInterceptor(verifier)))
	bbserver.Register(grpcServer, bbserver.NewServerWithConfig(grants, cfg))
	healthServerGRPC := health.NewServer()
	grpc_health_v1.RegisterHealthServer(grpcServer, healthServerGRPC)
	healthServerGRPC.SetServingStatus("", grpc_health_v1.HealthCheckResponse_SERVING)

	go func() {
		slog.Info("gRPC listening", "addr", ":9095")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = healthServer.Shutdown(context.Background())
}

// Purge cadence and retention (WS1.C): every 10 minutes, grants that expired
// more than 24 hours ago are deleted (children cascade).
const (
	purgeInterval  = 10 * time.Minute
	purgeRetention = 24 * time.Hour
)

func purgeLoop(ctx context.Context, purger grant.Purger) {
	ticker := time.NewTicker(purgeInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			n, err := purger.PurgeExpired(ctx, time.Now().Add(-purgeRetention))
			if err != nil {
				// The store already counted and classified the failure.
				slog.Warn("browser grant purge failed", "purged", n)
				continue
			}
			if n > 0 {
				slog.Info("browser grants purged", "purged", n)
			}
		}
	}
}

// ephemeralDevelopmentEnabled matches sandbox-manager: only the exact value
// "true" opts in.
func ephemeralDevelopmentEnabled(value string) bool {
	return value == "true"
}
