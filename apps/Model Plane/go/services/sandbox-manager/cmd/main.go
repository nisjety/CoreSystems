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
	"syscall"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	sbxserver "github.com/triodelab/model-plane/services/sandbox-manager/internal/server"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"
	"google.golang.org/grpc"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("sandbox-manager starting")

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

	// gRPC server on :9094 — sandbox manager stub returning Unimplemented.
	lis, err := net.Listen("tcp", ":9094")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	leaseStore := lease.NewStore()
	snapStore := snapshot.NewStore()

	grpcServer := grpc.NewServer()
	sbxserver.Register(grpcServer, sbxserver.NewServer(leaseStore, snapStore))

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
