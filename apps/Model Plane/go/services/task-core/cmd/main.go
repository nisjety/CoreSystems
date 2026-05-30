// Package main is the entry point for task-core.
// Handles tasks, cron scheduling, and manual triggers via gRPC + HTTP API.
package main

import (
	"context"
	"log/slog"
	"net"
	"os"
	"os/signal"
	"syscall"

	cronpkg "github.com/triodelab/model-plane/services/task-core/internal/cron"
	"github.com/triodelab/model-plane/services/task-core/internal/server"
	"github.com/triodelab/model-plane/services/task-core/internal/store"
	"google.golang.org/grpc"
	"google.golang.org/grpc/health"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("task-core starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	taskStore := store.NewStore()

	// Cron scheduler — ticks every 30 s looking for due tasks.
	scheduler := cronpkg.NewScheduler(taskStore, logger)
	go scheduler.Run(ctx)

	// gRPC server on :9099.
	lis, err := net.Listen("tcp", ":9099")
	if err != nil {
		slog.Error("failed to listen on gRPC port", "error", err)
		os.Exit(1)
	}

	grpcServer := grpc.NewServer()
	healthSrv := health.NewServer()
	healthpb.RegisterHealthServer(grpcServer, healthSrv)
	healthSrv.SetServingStatus("task-core", healthpb.HealthCheckResponse_SERVING)

	go func() {
		slog.Info("gRPC listening", "addr", ":9099")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	// HTTP server on :8090 (health + REST API).
	srv := server.NewServer(taskStore, logger)
	go func() {
		if err := srv.ListenAndServe(ctx, ":8090"); err != nil {
			slog.Error("HTTP server error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	slog.Info("task-core stopped")
}
