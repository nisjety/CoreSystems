// Package main is the entry point for sandbox-manager.
// Thread-scoped and agent-scoped sandbox lifecycle management.
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

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/process"
	sbxserver "github.com/triodelab/model-plane/services/sandbox-manager/internal/server"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/workspace"
	"google.golang.org/grpc"
	"google.golang.org/grpc/health"
	grpc_health_v1 "google.golang.org/grpc/health/grpc_health_v1"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("sandbox-manager starting")
	// Lease/snapshot durability: a real Postgres-backed store (migrations
	// 0001-0002, internal/lease + internal/snapshot) is required before this
	// binary may run in a normal deployment — starting it with the in-memory
	// fallback as though it were a durable Space computer would make a
	// restart silently discard a lease/snapshot a caller may rely on for an
	// effect. The explicit development switch keeps unit/manual experiments
	// possible (in-memory when DATABASE_URL is also absent, mirroring
	// cost-core's own established "runs against its in-memory ledger"
	// precedent for exactly this case) without letting an accidental
	// default become production behaviour.
	ephemeralDev := ephemeralDevelopmentEnabled(os.Getenv("SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT"))
	databaseURL := strings.TrimSpace(os.Getenv("DATABASE_URL"))
	if databaseURL == "" && !ephemeralDev {
		slog.Error("refusing to start sandbox-manager without a durable database", "required", "DATABASE_URL", "development_override", "SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT=true")
		os.Exit(1)
	}
	verifier, err := authz.NewVerifierFromEnv()
	if err != nil {
		slog.Error("sandbox-manager authentication configuration is invalid", "error", err)
		os.Exit(1)
	}

	// Space capability verification pins a lease request to the exact
	// sandbox-manager instance its capability decision named. A production
	// deployment must always be able to do this check — any request could
	// carry a space_id — so a missing or malformed Control public key /
	// backend id here is fatal unless ephemeral development is explicitly
	// opted in (the same escape hatch as the in-memory-store gate above).
	backendID := strings.TrimSpace(os.Getenv("SANDBOX_MANAGER_BACKEND_ID"))
	capabilityVerifier, capabilityVerifierErr := authz.LoadSpaceCapabilityVerifierFromEnv(os.Getenv)
	if capabilityVerifierErr != nil || backendID == "" {
		if !ephemeralDevelopmentEnabled(os.Getenv("SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT")) {
			slog.Error("refusing to start sandbox-manager without Space capability verification configured", "verifier_error", capabilityVerifierErr, "backend_id_configured", backendID != "")
			os.Exit(1)
		}
		slog.Warn("starting sandbox-manager without Space capability verification (ephemeral development only); any AcquireLease naming a space_id will be refused", "verifier_error", capabilityVerifierErr, "backend_id_configured", backendID != "")
		capabilityVerifier = nil
	}

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	// Health server on :8086
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	mux.HandleFunc("/readyz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})

	healthServer := &http.Server{Addr: ":8086", Handler: mux}
	go func() {
		slog.Info("health server listening", "addr", ":8086")
		if err := healthServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("health server error", "error", err)
		}
	}()

	// Authenticated gRPC application service on :9094.
	lis, err := net.Listen("tcp", ":9094")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	var leaseStore sbxserver.LeaseStore
	var snapStore sbxserver.SnapshotStore
	var workspaceStore sbxserver.WorkspaceStore
	// Nil unless a durable database is configured; a Server without it
	// refuses every process RPC.
	var processStore sbxserver.ProcessStore
	if databaseURL != "" {
		pool, err := pgxpool.New(ctx, databaseURL)
		if err != nil {
			slog.Error("sandbox-manager database pool unavailable", "error", err)
			os.Exit(1)
		}
		defer pool.Close()
		pgLeaseStore, err := lease.NewStore(pool)
		if err != nil {
			slog.Error("sandbox-manager lease store unavailable", "error", err)
			os.Exit(1)
		}
		pgSnapStore, err := snapshot.NewStore(pool)
		if err != nil {
			slog.Error("sandbox-manager snapshot store unavailable", "error", err)
			os.Exit(1)
		}
		pgWorkspaceStore, err := workspace.NewStore(pool)
		if err != nil {
			slog.Error("sandbox-manager workspace store unavailable", "error", err)
			os.Exit(1)
		}
		leaseStore, snapStore, workspaceStore = pgLeaseStore, pgSnapStore, pgWorkspaceStore
		slog.Info("sandbox-manager using durable Postgres-backed lease/snapshot/workspace stores")

		// S4.2: the background-process registry and its staleness sweeper.
		// Postgres-only by construction — there is no in-memory process
		// store, because a registry whose whole purpose is surviving a
		// restart has nothing to offer in a mode that discards it. In
		// ephemeral-development mode the Server therefore gets no registry
		// at all and refuses every process RPC, which is the honest answer
		// rather than serving state that vanishes on the next boot.
		pgProcessStore, err := process.NewStore(pool)
		if err != nil {
			slog.Error("sandbox-manager process store unavailable", "error", err)
			os.Exit(1)
		}
		processStore = pgProcessStore
		if process.EnvSweeperEnabled() {
			interval, staleAfter := process.TimingFromEnv(os.Getenv)
			// The concrete store, not the Server's narrowed interface: the
			// sweeper needs SweepStale, which is deliberately not part of
			// what the Server calls.
			go process.NewSweeper(pgProcessStore).WithTiming(interval, staleAfter).Start(ctx)
			slog.Info("process staleness sweeper started",
				"interval", interval.String(), "stale_after", staleAfter.String())
		} else {
			slog.Warn("process staleness sweeper disabled; a lost host's processes will keep reading as running")
		}
	} else {
		leaseStore, snapStore, workspaceStore = sbxserver.NewMemoryLeaseStore(), sbxserver.NewMemorySnapshotStore(), sbxserver.NewMemoryWorkspaceStore()
		slog.Warn("starting sandbox-manager with in-memory lease/snapshot/workspace stores (ephemeral development only); a restart discards all leases, snapshots, and workspace overlays")
	}

	server := sbxserver.NewServer(leaseStore, snapStore, workspaceStore).WithProcessStore(processStore)
	if capabilityVerifier != nil {
		server = server.WithCapabilityVerifier(capabilityVerifier.Verify, backendID)
	}
	grpcServer := grpc.NewServer(grpc.UnaryInterceptor(authz.UnaryInterceptor(verifier)))
	sbxserver.Register(grpcServer, server)
	healthServerGRPC := health.NewServer()
	grpc_health_v1.RegisterHealthServer(grpcServer, healthServerGRPC)
	healthServerGRPC.SetServingStatus("", grpc_health_v1.HealthCheckResponse_SERVING)

	go func() {
		slog.Info("gRPC listening", "addr", ":9094")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = healthServer.Shutdown(context.Background())
}

func ephemeralDevelopmentEnabled(value string) bool {
	return value == "true"
}
