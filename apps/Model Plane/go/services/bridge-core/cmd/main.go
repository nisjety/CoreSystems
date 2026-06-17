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
	"github.com/triodelab/model-plane/services/bridge-core/internal/config"
	"github.com/triodelab/model-plane/services/bridge-core/internal/delivery"
	bcserver "github.com/triodelab/model-plane/services/bridge-core/internal/server"
	"github.com/triodelab/model-plane/services/bridge-core/internal/session"
	"google.golang.org/grpc"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	cfg := config.Load()
	slog.Info("bridge-core starting", "http_addr", cfg.HTTPAddr, "grpc_addr", cfg.GRPCAddr)

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	// Domain setup
	registry := session.NewRegistry()
	adapters := channel.NewAdapterRegistry()

	// Durable delivery outbox + worker. The worker drains queued deliveries and
	// performs the real HTTP webhook POSTs with retry/backoff.
	outbox := delivery.NewMemoryStore()
	worker := delivery.NewWorker(outbox, channel.NewHTTPSender(nil), delivery.DefaultConfig())
	go worker.Run(ctx)

	// Wire a real WebhookAdapter for the configured channels when a webhook URL
	// is present; otherwise channels keep their noop fallback. This keeps the
	// behaviour honest: real delivery only when configured.
	if cfg.WebhookEnabled() {
		for _, ch := range cfg.WebhookChannels {
			adapter, err := channel.NewWebhookAdapter(channel.WebhookConfig{
				ChannelName: ch,
				Destination: cfg.WebhookURL,
				MaxAttempts: cfg.WebhookMaxAttempts,
			}, outbox)
			if err != nil {
				slog.Error("failed to build webhook adapter", "channel", ch, "error", err)
				continue
			}
			adapters.Register(ch, adapter)
			slog.Info("webhook channel adapter registered", "channel", ch, "destination", cfg.WebhookURL)
		}
	} else {
		slog.Info("no webhook URL configured; all channels using noop adapter (set BRIDGE_WEBHOOK_URL to enable real delivery)")
	}

	srv := bcserver.NewServer(registry, adapters)

	// HTTP server (sessions + health) on the configured address.
	httpServer := &http.Server{Addr: cfg.HTTPAddr, Handler: srv.Handler()}
	go func() {
		slog.Info("HTTP server listening", "addr", cfg.HTTPAddr)
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("HTTP server error", "error", err)
		}
	}()

	// gRPC server on the configured address.
	lis, err := net.Listen("tcp", cfg.GRPCAddr)
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	grpcServer := grpc.NewServer()

	go func() {
		slog.Info("gRPC listening", "addr", cfg.GRPCAddr)
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = httpServer.Shutdown(context.Background())
}
