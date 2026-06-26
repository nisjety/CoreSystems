// Package main is the entry point for cost-core.
//
// cost-core is the Model Plane token/cost ledger. It records per-run /
// per-org / per-user token and dollar costs into a durable (Postgres-backed)
// store, exposes query + aggregation HTTP endpoints, enforces budget caps for
// the model-gateway budget guard, and subscribes to USAGE_ENVELOPE events on
// the NATS bus to record costs as runs execute.
//
// Durability: when DATABASE_URL is set, the durable Postgres ledger is used;
// otherwise cost-core falls back to an in-memory ledger (local dev / tests).
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

	"github.com/nats-io/nats.go"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"github.com/triodelab/model-plane/services/cost-core/internal/postgres"
	"github.com/triodelab/model-plane/services/cost-core/internal/pricing"
	"github.com/triodelab/model-plane/services/cost-core/internal/server"
	"google.golang.org/grpc"
	"google.golang.org/grpc/health"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
)

// usageSubjectWildcard matches every per-org usage subject the model-gateway
// publishes to: mp.v1.usage.{org_id}. See mp-events subjects::usage_subject.
const usageSubjectWildcard = "mp.v1.usage.*"

// envelope is a minimal local view of the canonical Model Plane event envelope
// (pkg/envelope.Envelope). cost-core decodes only the fields it needs so it
// stays standalone-buildable (no workspace replace directives).
type envelope struct {
	EventType      string          `json:"event_type"`
	CorrelationID  string          `json:"correlation_id"`
	IdempotencyKey string          `json:"idempotency_key"`
	OrgID          string          `json:"org_id"`
	UserID         string          `json:"user_id"`
	Payload        json.RawMessage `json:"payload"`
}

// usagePayload is the inner payload of a USAGE_ENVELOPE event. cost_usd and
// run_id are optional: the gateway does not always emit them, in which case
// they default to zero/empty and cost is derived elsewhere.
type usagePayload struct {
	RequestID    string  `json:"request_id"`
	OrgID        string  `json:"org_id"`
	UserID       string  `json:"user_id"`
	RunID        string  `json:"run_id"`
	Model        string  `json:"model"`
	InputTokens  int64   `json:"input_tokens"`
	OutputTokens int64   `json:"output_tokens"`
	CostUSD      float64 `json:"cost_usd"`
}

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("cost-core starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	store := buildLedger(ctx)
	srv := server.NewServer(store)
	srv.SetPricing(buildPricing(ctx))

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

	// gRPC server on :9098. The Model Plane proto defines no CostService, so
	// the gRPC surface is health-only by design; all cost RPCs are served over
	// the HTTP API above (the model-gateway budget guard is an HTTP client).
	// Adding gRPC cost RPCs requires a coordinated proto change.
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

	// USAGE_ENVELOPE subscriber: record costs as runs execute.
	go subscribeUsageEnvelopes(ctx, srv)

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = httpServer.Shutdown(context.Background())
}

// buildLedger selects the durable Postgres ledger when DATABASE_URL is set,
// falling back to the in-memory ledger otherwise so the service runs cleanly
// in local dev and tests without a database.
func buildLedger(ctx context.Context) ledger.Ledger {
	dsn := os.Getenv("DATABASE_URL")
	if dsn == "" {
		slog.Warn("DATABASE_URL not set; using in-memory ledger (non-durable)")
		return ledger.NewStore()
	}
	pool, err := postgres.Connect(ctx, dsn)
	if err != nil {
		slog.Error("postgres connect failed; falling back to in-memory ledger", "error", err)
		return ledger.NewStore()
	}
	pgStore, err := postgres.New(pool)
	if err != nil {
		slog.Error("postgres store init failed; falling back to in-memory ledger", "error", err)
		pool.Close()
		return ledger.NewStore()
	}
	slog.Info("using durable Postgres ledger")
	return pgStore
}

// buildPricing loads the model price catalogue from Postgres when DATABASE_URL
// is set, falling back to the built-in default catalogue otherwise (or on any
// load error) so cost is always priceable. Returns a non-nil resolver.
func buildPricing(ctx context.Context) *pricing.Resolver {
	dsn := os.Getenv("DATABASE_URL")
	if dsn == "" {
		slog.Info("DATABASE_URL not set; using built-in default price catalogue")
		return pricing.Default()
	}
	pool, err := postgres.Connect(ctx, dsn)
	if err != nil {
		slog.Warn("pricing: postgres connect failed; using default price catalogue", "error", err)
		return pricing.Default()
	}
	defer pool.Close()
	resolver, err := pricing.LoadFromPool(ctx, pool)
	if err != nil {
		slog.Warn("pricing: catalogue load failed; using default price catalogue", "error", err)
		return pricing.Default()
	}
	slog.Info("loaded model price catalogue from Postgres")
	return resolver
}

// subscribeUsageEnvelopes connects to NATS and subscribes to per-org usage
// subjects (mp.v1.usage.*), recording each USAGE_ENVELOPE event in the ledger.
// When NATS_URL is unset it falls back to the NATS_FEED_PATH file source if
// provided, otherwise stays idle until shutdown.
func subscribeUsageEnvelopes(ctx context.Context, srv *server.Server) {
	natsURL := os.Getenv("NATS_URL")
	if natsURL == "" {
		slog.Info("NATS_URL not set; USAGE_ENVELOPE NATS subscriber disabled")
		runFeedFallback(ctx, srv)
		return
	}

	nc, err := nats.Connect(natsURL,
		nats.RetryOnFailedConnect(true),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(2*time.Second),
	)
	if err != nil {
		slog.Error("NATS connect failed; USAGE_ENVELOPE subscriber disabled", "error", err, "url", natsURL)
		runFeedFallback(ctx, srv)
		return
	}
	defer nc.Close()

	sub, err := nc.Subscribe(usageSubjectWildcard, func(msg *nats.Msg) {
		handleUsageMessage(ctx, srv, msg.Data)
	})
	if err != nil {
		slog.Error("NATS subscribe failed; USAGE_ENVELOPE subscriber disabled", "error", err, "subject", usageSubjectWildcard)
		runFeedFallback(ctx, srv)
		return
	}
	defer func() { _ = sub.Unsubscribe() }()

	slog.Info("USAGE_ENVELOPE subscriber active", "url", natsURL, "subject", usageSubjectWildcard)
	<-ctx.Done()
}

// handleUsageMessage decodes a usage envelope and records it in the ledger.
func handleUsageMessage(ctx context.Context, srv *server.Server, data []byte) {
	var env envelope
	if err := json.Unmarshal(data, &env); err != nil {
		slog.Warn("failed to decode usage envelope", "error", err)
		return
	}
	if env.EventType != "" && env.EventType != "USAGE_ENVELOPE" {
		return // not a usage event
	}

	var p usagePayload
	if len(env.Payload) > 0 {
		if err := json.Unmarshal(env.Payload, &p); err != nil {
			slog.Warn("failed to decode usage payload", "error", err)
			return
		}
	}

	orgID := firstNonEmpty(p.OrgID, env.OrgID)
	userID := firstNonEmpty(p.UserID, env.UserID)
	if orgID == "" {
		slog.Warn("skipping usage envelope with empty org_id")
		return
	}

	entry := ledger.Entry{
		OrgID:          orgID,
		UserID:         userID,
		RunID:          p.RunID,
		RequestID:      firstNonEmpty(p.RequestID, env.CorrelationID),
		Model:          p.Model,
		InputTokens:    p.InputTokens,
		OutputTokens:   p.OutputTokens,
		CostUSD:        p.CostUSD,
		IdempotencyKey: env.IdempotencyKey,
		CreatedAt:      time.Now().UTC(),
	}
	if err := srv.RecordUsage(ctx, entry); err != nil {
		slog.Warn("failed to record usage envelope", "org_id", orgID, "error", err)
	}
}

// runFeedFallback reads newline/array JSON usage payloads from NATS_FEED_PATH
// for local integration testing without a NATS server, or blocks until
// shutdown when the path is unset.
func runFeedFallback(ctx context.Context, srv *server.Server) {
	feedPath := os.Getenv("NATS_FEED_PATH")
	if feedPath == "" {
		<-ctx.Done()
		return
	}
	slog.Info("reading usage events from feed file", "path", feedPath)
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		data, err := os.ReadFile(feedPath)
		if err != nil {
			slog.Warn("failed to read usage feed", "path", feedPath, "error", err)
			sleepCtx(ctx, 5*time.Second)
			continue
		}

		var payloads []usagePayload
		if err := json.Unmarshal(data, &payloads); err != nil {
			var single usagePayload
			if err2 := json.Unmarshal(data, &single); err2 == nil {
				payloads = []usagePayload{single}
			} else {
				slog.Warn("failed to parse usage feed", "error", err)
			}
		}
		for _, p := range payloads {
			if p.OrgID == "" {
				continue
			}
			_ = srv.RecordUsage(ctx, ledger.Entry{
				OrgID:        p.OrgID,
				UserID:       p.UserID,
				RunID:        p.RunID,
				RequestID:    p.RequestID,
				Model:        p.Model,
				InputTokens:  p.InputTokens,
				OutputTokens: p.OutputTokens,
				CostUSD:      p.CostUSD,
			})
		}
		sleepCtx(ctx, 2*time.Second)
	}
}

func firstNonEmpty(vals ...string) string {
	for _, v := range vals {
		if v != "" {
			return v
		}
	}
	return ""
}

// sleepCtx sleeps for d or until ctx is cancelled, whichever comes first.
func sleepCtx(ctx context.Context, d time.Duration) {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
	case <-t.C:
	}
}
