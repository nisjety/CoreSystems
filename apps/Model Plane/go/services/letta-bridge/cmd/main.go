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

	"github.com/triodelab/model-plane/services/letta-bridge/internal/agentmemory"
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

	grpcServer := grpc.NewServer()
	lbserver.Register(grpcServer, buildMemoryServer())

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

// buildMemoryServer selects the memory backend. When AGENT_MEMORY_URL is set,
// letta-bridge uses Redis Agent Memory (semantic vector long-term recall);
// otherwise it falls back to the in-memory substring store so the service
// still boots with no external dependency.
func buildMemoryServer() *lbserver.Server {
	if cli, ok := agentmemory.New(agentmemory.Config{
		BaseURL: os.Getenv("AGENT_MEMORY_URL"),
		APIKey:  os.Getenv("AGENT_MEMORY_TOKEN"),
	}); ok {
		slog.Info("agent memory backend enabled (redis agent-memory-server)")
		return lbserver.NewServerWithStore(cli)
	}
	slog.Info("agent memory backend disabled (AGENT_MEMORY_URL unset); using in-memory store")
	return lbserver.NewServer()
}
