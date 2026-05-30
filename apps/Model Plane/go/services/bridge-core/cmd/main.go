// Package main is the entry point for bridge-core.
// CLI/IDE/channel ingress — voice, canvas, and remote session lifecycle.
package main

import (
	"context"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"

	"github.com/triodelab/model-plane/services/bridge-core/internal/channel"
	bcserver "github.com/triodelab/model-plane/services/bridge-core/internal/server"
	"github.com/triodelab/model-plane/services/bridge-core/internal/session"
	"google.golang.org/grpc"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("bridge-core starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	// Domain setup
	registry := session.NewRegistry()
	adapters := channel.NewAdapterRegistry()
	srv := bcserver.NewServer(registry, adapters)

	// HTTP server on :8091
	httpServer := &http.Server{Addr: ":8091", Handler: srv.Handler()}
	go func() {
		slog.Info("HTTP server listening", "addr", ":8091")
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("HTTP server error", "error", err)
		}
	}()

	// gRPC server on :9100
	lis, err := net.Listen("tcp", ":9100")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	grpcServer := grpc.NewServer()

	go func() {
		slog.Info("gRPC listening", "addr", ":9100")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = httpServer.Shutdown(context.Background())
}
