// Package main is the entry point for letta-bridge.
// Block synchronization and retrieval tooling only — never thread/workflow owner.
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

	"github.com/triodelab/model-plane/services/letta-bridge/internal/agentmemory"
	"github.com/triodelab/model-plane/services/letta-bridge/internal/pgstore"
	lbserver "github.com/triodelab/model-plane/services/letta-bridge/internal/server"
	"google.golang.org/grpc"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("letta-bridge starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	// Health server on :8088
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	mux.HandleFunc("/readyz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})

	healthServer := &http.Server{Addr: ":8088", Handler: mux}
	go func() {
		slog.Info("health server listening", "addr", ":8088")
		if err := healthServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("health server error", "error", err)
		}
	}()

	// gRPC server on :9096
	lis, err := net.Listen("tcp", ":9096")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	memServer, closeStore := buildMemoryServer(ctx)
	defer closeStore()

	grpcServer := grpc.NewServer()
	lbserver.Register(grpcServer, memServer)

	go func() {
		slog.Info("gRPC listening", "addr", ":9096")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = healthServer.Shutdown(context.Background())
}

// buildMemoryServer selects the memory backend in priority order and returns
// the server plus a cleanup func that releases any backend resources.
//
//  1. AGENT_MEMORY_URL set → Redis Agent Memory (semantic vector long-term
//     recall, the preferred tier).
//  2. DATABASE_URL set → Postgres-backed durable store (survives restarts;
//     used so orchestrator-core's memory-consolidation writes persist).
//  3. neither set → in-memory substring store, so the service still boots with
//     no external dependency.
func buildMemoryServer(ctx context.Context) (*lbserver.Server, func()) {
	noop := func() {}

	if cli, ok := agentmemory.New(agentmemory.Config{
		BaseURL: os.Getenv("AGENT_MEMORY_URL"),
		APIKey:  os.Getenv("AGENT_MEMORY_TOKEN"),
	}); ok {
		slog.Info("agent memory backend enabled (redis agent-memory-server)")
		return lbserver.NewServerWithStore(cli), noop
	}

	if dsn := os.Getenv("DATABASE_URL"); dsn != "" {
		pool, err := pgxpool.New(ctx, dsn)
		if err != nil {
			slog.Error("postgres backend requested but pool init failed; falling back to in-memory store", "error", err)
			return lbserver.NewServer(), noop
		}
		store, err := pgstore.New(ctx, pool)
		if err != nil {
			slog.Error("postgres backend requested but schema ensure failed; falling back to in-memory store", "error", err)
			pool.Close()
			return lbserver.NewServer(), noop
		}
		slog.Info("postgres durable memory backend enabled (DATABASE_URL set)")
		return lbserver.NewServerWithStore(store), pool.Close
	}

	slog.Info("no durable memory backend configured (AGENT_MEMORY_URL and DATABASE_URL unset); using in-memory store")
	return lbserver.NewServer(), noop
}
