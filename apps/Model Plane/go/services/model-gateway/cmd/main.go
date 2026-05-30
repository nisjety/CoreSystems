// Package main is the entry point for model-gateway.
//
// model-gateway owns the public invoke surface and forwards traffic to
// session-core / inference-core. It does not own threads, runs, or memory.
package main

import (
	"context"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"syscall"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/quarry"
	"github.com/triodelab/model-plane/services/model-gateway/internal/langcache"
	"github.com/triodelab/model-plane/services/model-gateway/internal/proxy"
	mgserver "github.com/triodelab/model-plane/services/model-gateway/internal/server"
	"github.com/triodelab/model-plane/services/model-gateway/internal/sse"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("model-gateway starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	// Health server on :8090
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	mux.HandleFunc("/readyz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})

	orchClient := buildOrchClient(ctx)
	mux.Handle("/v1/runs/", sse.New(orchClient, logger))

	healthServer := &http.Server{Addr: ":8090", Handler: mux}
	go func() {
		slog.Info("health server listening", "addr", ":8090")
		if err := healthServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("health server error", "error", err)
		}
	}()

	// gRPC server on :9090
	lis, err := net.Listen("tcp", ":9090")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	grpcServer := grpc.NewServer()
	p := buildProxy(ctx)
	srv := mgserver.NewServer(p)

	// Wave 9 — wire Quarry edge client for Fetch + ExtractStructured.
	// QUARRY_EDGE_URL empty (or unreachable at first call) → those RPCs
	// return Unimplemented; everything else keeps working.
	if qc := buildQuarryClient(); qc != nil {
		srv.SetQuarry(qc)
	}

	// Wave 9 — wire a direct InferenceClient for ExtractStructured. We
	// reuse the same INFERENCE_CORE_ADDR as buildProxy; if buildProxy
	// already dialled inference-core we'd ideally share that conn, but
	// the existing API doesn't expose it. A second dial is cheap (gRPC
	// HTTP/2 multiplexing makes it ~free at the wire) and keeps the
	// extraction path independent of session-core availability.
	if ic := buildInferenceClient(); ic != nil {
		srv.SetInferenceClient(ic)
	}

	mpv1.RegisterModelGatewayServer(grpcServer, srv)

	go func() {
		slog.Info("gRPC listening", "addr", ":9090")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = healthServer.Shutdown(context.Background())
}

// buildProxy dials session-core and inference-core when both addresses are
// provided via SESSION_CORE_ADDR / INFERENCE_CORE_ADDR. If either is missing,
// the gateway runs in stub mode (Invoke / InvokeStream return Unimplemented),
// which keeps the v2 fallback path active during cutover.
func buildProxy(_ context.Context) *proxy.Proxy {
	sessAddr := os.Getenv("SESSION_CORE_ADDR")
	infAddr := os.Getenv("INFERENCE_CORE_ADDR")
	if sessAddr == "" || infAddr == "" {
		slog.Info("downstream proxy disabled (stub mode)",
			"session_core_addr", sessAddr, "inference_core_addr", infAddr)
		return nil
	}

	sessConn, err := grpc.NewClient(sessAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		slog.Error("dial session-core failed; running stub mode", "addr", sessAddr, "error", err)
		return nil
	}
	infConn, err := grpc.NewClient(infAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		slog.Error("dial inference-core failed; running stub mode", "addr", infAddr, "error", err)
		_ = sessConn.Close()
		return nil
	}

	slog.Info("downstream proxy enabled",
		"session_core_addr", sessAddr, "inference_core_addr", infAddr)
	return proxy.New(
		mpv1.NewSessionCoreClient(sessConn),
		mpv1.NewInferenceCoreClient(infConn),
		semanticCacheOpts()...,
	)
}

// semanticCacheOpts wires a Redis LangCache semantic cache when LANGCACHE_URL,
// LANGCACHE_CACHE_ID, and LANGCACHE_API_KEY are all set. When any is missing it
// returns no options, so the gateway runs without a cache (every Invoke hits
// inference-core) — matching the dev-friendly stub pattern used elsewhere.
func semanticCacheOpts() []proxy.Option {
	cli, ok := langcache.New(langcache.Config{
		BaseURL:   os.Getenv("LANGCACHE_URL"),
		CacheID:   os.Getenv("LANGCACHE_CACHE_ID"),
		APIKey:    os.Getenv("LANGCACHE_API_KEY"),
		Threshold: langcacheThreshold(),
	})
	if !ok {
		slog.Info("semantic cache disabled (LANGCACHE_URL/CACHE_ID/API_KEY unset)")
		return nil
	}
	slog.Info("semantic cache enabled (langcache)")
	return []proxy.Option{proxy.WithCache(cli)}
}

// langcacheThreshold reads LANGCACHE_THRESHOLD as a float. Unset or invalid
// values return 0, which lets the client apply its own default.
func langcacheThreshold() float64 {
	v := os.Getenv("LANGCACHE_THRESHOLD")
	if v == "" {
		return 0
	}
	f, err := strconv.ParseFloat(v, 64)
	if err != nil {
		slog.Warn("invalid LANGCACHE_THRESHOLD; using client default", "value", v)
		return 0
	}
	return f
}

// buildQuarryClient constructs the Quarry edge HTTP client used by the
// Fetch + ExtractStructured RPCs. Returns nil when QUARRY_EDGE_URL is
// unset, so the gateway boots cleanly in dev environments without an
// Ingestion Plane running.
//
// Auth: QUARRY_EDGE_TOKEN is sent as a Bearer header. Quarry's dev
// bypass means any non-empty value works in dev; production deployments
// must set a real token.
func buildQuarryClient() *quarry.Client {
	base := os.Getenv("QUARRY_EDGE_URL")
	if base == "" {
		slog.Info("quarry edge disabled (QUARRY_EDGE_URL unset)")
		return nil
	}
	c := quarry.New(quarry.Config{
		BaseURL: base,
		Token:   os.Getenv("QUARRY_EDGE_TOKEN"),
		// 30s default is generous because Quarry may JS-render. Override
		// with QUARRY_EDGE_TIMEOUT_SECS if the operator wants snappier
		// failures on a CDN-fronted target set.
		Timeout: 30 * time.Second,
	})
	slog.Info("quarry edge enabled", "base_url", base)
	return c
}

// buildInferenceClient dials inference-core for the ExtractStructured
// LLM-coercion path. Same env var as buildProxy (INFERENCE_CORE_ADDR);
// returning nil here just means ExtractStructured replies Unimplemented.
func buildInferenceClient() mpv1.InferenceCoreClient {
	addr := os.Getenv("INFERENCE_CORE_ADDR")
	if addr == "" {
		slog.Info("inference client for ExtractStructured disabled (INFERENCE_CORE_ADDR unset)")
		return nil
	}
	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		slog.Error("dial inference-core for ExtractStructured failed", "addr", addr, "error", err)
		return nil
	}
	slog.Info("inference client for ExtractStructured enabled", "addr", addr)
	return mpv1.NewInferenceCoreClient(conn)
}

// buildOrchClient dials orchestrator-core for SSE streaming. Returns nil if
// the dial fails; the SSE handler treats a nil client as Unimplemented.
func buildOrchClient(_ context.Context) mpv1.OrchestrationCoreServiceClient {
	addr := os.Getenv("ORCHESTRATOR_CORE_ADDR")
	if addr == "" {
		addr = "localhost:9091"
	}
	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		slog.Error("dial orchestrator-core failed", "addr", addr, "error", err)
		return nil
	}
	slog.Info("orchestrator-core client ready", "addr", addr)
	return mpv1.NewOrchestrationCoreServiceClient(conn)
}
