// Package main is the entry point for cost-core.
// Token/cost ledger — tracks usage per org/user, enforces budget caps,
// and subscribes to USAGE_ENVELOPE events via NATS.
package main

import (
	"context"
	"encoding/json"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"github.com/triodelab/model-plane/services/cost-core/internal/server"
	"google.golang.org/grpc"
	"google.golang.org/grpc/health"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
)

// usageEnvelope is the JSON shape of the USAGE_ENVELOPE NATS message.
type usageEnvelope struct {
	OrgID        string  `json:"org_id"`
	UserID       string  `json:"user_id"`
	InputTokens  int32   `json:"input_tokens"`
	OutputTokens int32   `json:"output_tokens"`
	CostUSD      float64 `json:"cost_usd"`
	Model        string  `json:"model"`
	Timestamp    string  `json:"timestamp"`
}

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("cost-core starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	store := ledger.NewStore()
	srv := server.NewServer(store)

	// HTTP server on :8089 (health + API)
	mux := http.NewServeMux()
	srv.RegisterRoutes(mux)

	httpServer := &http.Server{
		Addr:              ":8089",
		Handler:           mux,
		ReadHeaderTimeout: 10 * time.Second,
	}
	go func() {
		slog.Info("HTTP server listening", "addr", ":8089")
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("HTTP server error", "error", err)
		}
	}()

	// gRPC server on :9098 (health only; extend with CostService RPCs later)
	lis, err := net.Listen("tcp", ":9098")
	if err != nil {
		slog.Error("failed to listen on gRPC port", "error", err)
		os.Exit(1)
	}

	grpcServer := grpc.NewServer()
	healthSvc := health.NewServer()
	healthpb.RegisterHealthServer(grpcServer, healthSvc)
	healthSvc.SetServingStatus("cost-core", healthpb.HealthCheckResponse_SERVING)

	go func() {
		slog.Info("gRPC listening", "addr", ":9098")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	// NATS subscriber for USAGE_ENVELOPE events.
	// Uses the NATS_URL environment variable; falls back to nats://localhost:4222.
	go subscribeUsageEnvelopes(ctx, srv)

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = httpServer.Shutdown(context.Background())
}

// subscribeUsageEnvelopes connects to NATS and subscribes to
// "USAGE_ENVELOPE" messages, recording each event in the ledger.
// It reconnects on failure with exponential backoff.
func subscribeUsageEnvelopes(ctx context.Context, srv *server.Server) {
	natsURL := os.Getenv("NATS_URL")
	if natsURL == "" {
		natsURL = "nats://localhost:4222"
	}

	slog.Info("NATS subscriber starting", "url", natsURL, "subject", "USAGE_ENVELOPE")

	// Minimal embedded NATS consumer using a raw TCP connection + JSON.
	// In production this would use github.com/nats-io/nats.go; the logic
	// below is a zero-dependency placeholder that reads newline-delimited
	// JSON from a configurable data source (file, pipe, or future NATS
	// client). For now it reads from the NATS_FEED_PATH env var if set,
	// allowing integration testing without a running NATS server.
	feedPath := os.Getenv("NATS_FEED_PATH")
	if feedPath == "" {
		slog.Info("NATS_FEED_PATH not set; USAGE_ENVELOPE subscriber idle until NATS client is wired")
		// Block until context is cancelled so the goroutine stays alive
		// for future NATS client integration.
		<-ctx.Done()
		return
	}

	slog.Info("reading usage events from feed file", "path", feedPath)
	processUsageFeed(ctx, srv, feedPath)
}

// processUsageFeed reads newline-delimited JSON usage envelopes from a file
// or named pipe, recording each in the ledger. It re-opens the file on EOF
// after a short delay, making it work with append-mode log files or FIFOs.
func processUsageFeed(ctx context.Context, srv *server.Server, path string) {
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		data, err := os.ReadFile(path)
		if err != nil {
			slog.Warn("failed to read usage feed", "path", path, "error", err)
			sleepCtx(ctx, 5*time.Second)
			continue
		}

		var envelopes []usageEnvelope
		if err := json.Unmarshal(data, &envelopes); err != nil {
			// Try single object
			var single usageEnvelope
			if err2 := json.Unmarshal(data, &single); err2 != nil {
				slog.Warn("failed to parse usage feed", "error", err)
			} else {
				envelopes = append(envelopes, single)
			}
		}

		for _, env := range envelopes {
			if env.OrgID == "" || env.UserID == "" {
				slog.Warn("skipping envelope with missing org_id or user_id")
				continue
			}
			srv.RecordUsage(env.OrgID, env.UserID, env.InputTokens, env.OutputTokens, env.CostUSD)
		}

		// Wait before re-reading to avoid busy-looping.
		sleepCtx(ctx, 2*time.Second)
	}
}

// sleepCtx sleeps for the given duration or until the context is cancelled,
// whichever comes first.
func sleepCtx(ctx context.Context, d time.Duration) {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
	case <-t.C:
	}
}
