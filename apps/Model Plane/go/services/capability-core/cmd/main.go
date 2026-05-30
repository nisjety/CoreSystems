// Package main is the entry point for capability-core.
// Policy and capability authority — skill registry, tool metadata, eligibility.
package main

import (
	"context"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/api"
	"github.com/triodelab/model-plane/services/capability-core/internal/commands"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
	"github.com/triodelab/model-plane/services/capability-core/internal/roadmap"
	capserver "github.com/triodelab/model-plane/services/capability-core/internal/server"
	"google.golang.org/grpc"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("capability-core starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	dsn := os.Getenv("DATABASE_URL")
	if dsn == "" {
		slog.Error("DATABASE_URL required")
		os.Exit(1)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		slog.Error("pgx pool", "error", err)
		os.Exit(1)
	}
	defer pool.Close()

	// --- registry sources ---------------------------------------------------
	// Postgres-backed live source merged with static seed.
	pgSource, err := registry.NewCapabilitiesSource(pool, "")
	if err != nil {
		slog.Warn("capabilities source unavailable, falling back to static seed", "error", err)
	}

	var reg *registry.Registry
	if pgSource != nil {
		reg, err = registry.NewFromSource(pgSource)
		if err != nil {
			slog.Warn("postgres capabilities source load failed, falling back to static seed", "error", err)
			reg = registry.NewRegistry()
		}
	} else {
		reg = registry.NewRegistry()
	}

	modelsReg, err := registry.NewModelsRegistry(pool)
	if err != nil {
		slog.Error("models registry", "error", err)
		os.Exit(1)
	}

	capStore, err := registry.NewCapabilitiesStore(pool)
	if err != nil {
		slog.Warn("capabilities store unavailable", "error", err)
	}

	pol := policy.New(reg)

	// --- §4.3 reconcile event publisher -------------------------------------
	// capability-core is the registry system-of-record; on a create/update it
	// emits mp.v1.capability.<kind>.<action> so cache holders (the gateway's
	// runtime registries) stay coherent. The gateway-side consumer is built
	// (model-gateway capability_consumer); this is the emit half. Best-effort
	// and guarded: with NATS_URL unset the publisher is nil and reconcile.Emit
	// no-ops, so the service runs cleanly without a bus. ModeV1Only because
	// capability events are v1-native (no legacy mapping).
	var recPub publisher.EventPublisher
	if natsURL := os.Getenv("NATS_URL"); natsURL != "" {
		nc, nerr := nats.Connect(natsURL)
		if nerr != nil {
			slog.Warn("NATS connect failed; capability reconcile events disabled", "error", nerr)
		} else {
			defer nc.Close()
			recPub = publisher.NewNATSPublisher(natsx.NewPublisher(nc, natsx.ModeV1Only))
			slog.Info("capability reconcile events enabled", "nats_url", natsURL)
		}
	}

	// --- HTTP server on :8085 -----------------------------------------------
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	mux.HandleFunc("/readyz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	mux.Handle("/api/v1/model-plane/implementation-status", roadmap.NewHandler())

	// Capability-core product APIs
	if capStore != nil {
		api.NewCapabilitiesHandler(capStore).Register(mux)
	}
	// The four reconcile-emitting registries get the publisher (nil-safe: a nil
	// recPub makes reconcile.Emit a no-op).
	api.NewSkillsHandler(pool).WithPublisher(recPub).Register(mux)
	api.NewMCPHandler(pool).WithPublisher(recPub).Register(mux)
	api.NewRoutingHandler(pool).WithPublisher(recPub).Register(mux)
	api.NewSafetyHandler(pool).WithPublisher(recPub).Register(mux)
	api.NewMemoryHandler(pool).Register(mux)
	api.NewTasksHandler(pool).Register(mux)
	api.NewCronHandler(pool).Register(mux)
	commands.NewHandler().Register(mux)

	healthServer := &http.Server{Addr: ":8085", Handler: mux}
	go func() {
		slog.Info("health server listening", "addr", ":8085")
		if err := healthServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("health server error", "error", err)
		}
	}()

	// gRPC server on :9097
	lis, err := net.Listen("tcp", ":9097")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	grpcServer := grpc.NewServer()
	capserver.Register(grpcServer, capserver.NewServer(reg, modelsReg, pol))

	go func() {
		slog.Info("gRPC listening", "addr", ":9097")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = healthServer.Shutdown(context.Background())
}
