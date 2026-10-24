// Package main is the entry point for browser-broker.
// Trusted browser grant lifecycle — local/cloud mode, per-session revocation.
package main

import (
	"context"
	"errors"
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
	databaseURL := strings.TrimSpace(os.Getenv("DATABASE_URL"))
	if err := startupGate(databaseURL, os.Getenv("BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT")); err != nil {
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
		// WS8.C: the effective human-browse settings, never a host or an org.
		"human_browse_enabled", cfg.HumanBrowseEnabled,
		"human_absolute_ttl_seconds", int(cfg.HumanAbsoluteTTL.Seconds()),
		"max_attach_domains", cfg.MaxAttachDomains,
		"deny_suffix_count", denySuffixCount(cfg),
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
		pool, err := newPool(ctx, databaseURL)
		if err != nil {
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
	mux.HandleFunc("/readyz", readyzHandler(ready))

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

// denySuffixCount is the size of the effective human-browse deny-list: the
// configured one, or the built-in default.
func denySuffixCount(cfg bbserver.Config) int {
	if len(cfg.DenySuffixes) > 0 {
		return len(cfg.DenySuffixes)
	}
	return len(grant.DefaultDeniedSuffixes)
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
			purgeOnce(ctx, purger, time.Now())
		}
	}
}

// purgeOnce deletes the grants that expired more than purgeRetention before now.
func purgeOnce(ctx context.Context, purger grant.Purger, now time.Time) {
	n, err := purger.PurgeExpired(ctx, now.Add(-purgeRetention))
	if err != nil {
		// The store already counted and classified the failure.
		slog.Warn("browser grant purge failed", "purged", n)
		return
	}
	if n > 0 {
		slog.Info("browser grants purged", "purged", n)
	}
}

// errStartupRefused means no durable database is configured and the
// ephemeral-development opt-in is not exactly "true".
var errStartupRefused = errors.New("refusing to start browser-broker without a durable database")

// startupGate refuses to start without DATABASE_URL unless the explicit
// development opt-in is set, so a production restart cannot silently void
// every live session grant by running on in-memory grants.
func startupGate(databaseURL, ephemeralFlag string) error {
	if databaseURL == "" && !ephemeralDevelopmentEnabled(ephemeralFlag) {
		return errStartupRefused
	}
	return nil
}

// errDatabaseURLInvalid carries no part of the URL: a parse error can quote
// the host or a password.
var errDatabaseURLInvalid = errors.New("DATABASE_URL is not a valid connection string")

// newPool builds the pool; it does not connect until first use.
func newPool(ctx context.Context, databaseURL string) (*pgxpool.Pool, error) {
	pool, err := pgxpool.New(ctx, databaseURL)
	if err != nil {
		return nil, errDatabaseURLInvalid
	}
	return pool, nil
}

// readyzHandler answers 503 while the grant store does not answer within 2 s.
// A nil ready (ephemeral development) is always ready.
func readyzHandler(ready func(context.Context) error) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
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
	}
}

// ephemeralDevelopmentEnabled matches sandbox-manager: only the exact value
// "true" opts in.
func ephemeralDevelopmentEnabled(value string) bool {
	return value == "true"
}
