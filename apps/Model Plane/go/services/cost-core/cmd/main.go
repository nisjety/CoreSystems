// Package main is the entry point for cost-core.
//
// cost-core is the Model Plane token/cost ledger. It records per-run /
// per-org / per-user token and dollar costs into a durable (Postgres-backed)
// store, exposes query + aggregation HTTP endpoints, enforces budget caps for
// the model-gateway budget guard, and subscribes to USAGE_ENVELOPE events on
// the NATS bus to record costs as runs execute.
//
// Durability: the service requires Postgres. Local tests may opt into the
// in-memory ledger explicitly with COST_CORE_ALLOW_EPHEMERAL=true.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/cost-core/internal/consumers"
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

const natsInboxPrefix = "_INBOX.COST_CORE_RUNTIME"

// gdprInboxPrefix is the inbox prefix for the SECOND, narrowly-scoped NATS
// connection used only by the GDPR org-erasure consumer (see
// runOrgErasureConsumer). Deliberately distinct from natsInboxPrefix so the
// cross-plane GDPR credential/connection never shares an inbox namespace with
// the Model-Plane-local USAGE_ENVELOPE connection above.
const gdprInboxPrefix = "_INBOX.COST_CORE_GDPR"

const (
	usageEnvelopeProducer = "model-gateway"
	maxUsageMessageBytes  = 64 << 10
)

// envelope is a minimal local view of the canonical Model Plane event envelope
// (pkg/envelope.Envelope). cost-core decodes only the fields it needs so it
// stays standalone-buildable (no workspace replace directives).
type envelope struct {
	EventType      string          `json:"event_type"`
	Producer       string          `json:"producer"`
	CorrelationID  string          `json:"correlation_id"`
	IdempotencyKey string          `json:"idempotency_key"`
	OrgID          string          `json:"org_id"`
	UserID         string          `json:"user_id"`
	Payload        json.RawMessage `json:"payload"`
}

// usagePayload is the inner payload of a USAGE_ENVELOPE event. cost_usd and
// run_id are optional: the gateway does not always emit them, in which case
// they default to zero/empty and cost is derived elsewhere.
//
// CacheReadInputTokens / CacheCreationInputTokens are cache-token telemetry
// (a native-compaction migration prerequisite); older gateway builds that
// predate this field simply omit the JSON keys, which decodes to the correct
// zero value here -- no version gate needed.
type usagePayload struct {
	RequestID                string  `json:"request_id"`
	OrgID                    string  `json:"org_id"`
	UserID                   string  `json:"user_id"`
	RunID                    string  `json:"run_id"`
	Model                    string  `json:"model"`
	InputTokens              int64   `json:"input_tokens"`
	OutputTokens             int64   `json:"output_tokens"`
	CostUSD                  float64 `json:"cost_usd"`
	CacheReadInputTokens     int64   `json:"cache_read_input_tokens"`
	CacheCreationInputTokens int64   `json:"cache_creation_input_tokens"`
}

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("cost-core starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	store, err := buildLedgerFromConfig(
		ctx,
		strings.TrimSpace(os.Getenv("DATABASE_URL")),
		strings.EqualFold(strings.TrimSpace(os.Getenv("COST_CORE_ALLOW_EPHEMERAL")), "true"),
	)
	if err != nil {
		slog.Error("durable cost ledger unavailable", "error", err)
		os.Exit(1)
	}
	srv := server.NewServer(store)
	srv.SetPricing(buildPricing(ctx))
	// COST_CORE_AUTH_AUDIENCE is a comma-separated list (e.g.
	// "cost-core,inference-core"). Budget checks are made by inference-core's
	// intent layer forwarding the caller's own delegated `aud=inference-core`
	// token, so that audience must verify here too. This does NOT widen
	// writes: CostAuthorizer limits user principals to reads + POST
	// /api/v1/budget/check, and handleBudgetCheck pins org/user to the
	// token's claims — an accepted second audience still only ever acts as
	// the token's own org and user.
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences: splitAudiences(requiredEnv("COST_CORE_AUTH_AUDIENCE")),
		Issuer:    requiredEnv("AUTH_CORE_ISSUER"),
		JWKSURL:   requiredEnv("AUTH_CORE_JWKS_URL"),
	})
	if err != nil {
		slog.Error("cost-core authentication unavailable", "error", err)
		os.Exit(1)
	}

	// HTTP server on :8089 (health + API)
	httpServer := &http.Server{
		Addr:              ":8089",
		Handler:           srv.Handler(verifier.HTTPMiddleware(server.CostAuthorizer)),
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

	// Cross-plane GDPR erasure fan-out: org-core publishes
	// verevon.gdpr.erasure.requested (explicit hard-delete AND its 30-day
	// auto-purge cron) on the shared Control-Plane bus; this permanently
	// deletes every cost_entries row cost-core holds for that org. Runs on its
	// own dedicated shared-broker connection (identity "cost-core-gdpr",
	// configured via COST_CORE_GDPR_NATS_URL/_USER/_PASSWORD below) —
	// independent of the Model-Plane-local NATS_URL connection used by the
	// USAGE_ENVELOPE subscriber above, since this consumer never touches that
	// plane-local broker.
	go runOrgErasureConsumer(ctx, store)

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = httpServer.Shutdown(context.Background())
}

func requiredEnv(name string) string {
	value := strings.TrimSpace(os.Getenv(name))
	if value == "" {
		slog.Error("required environment variable is missing", "name", name)
		os.Exit(1)
	}
	return value
}

// splitAudiences parses a comma-separated audience list, trimming whitespace
// and dropping empty entries, so "cost-core, inference-core" and "cost-core"
// both configure the verifier correctly.
func splitAudiences(raw string) []string {
	parts := strings.Split(raw, ",")
	audiences := make([]string, 0, len(parts))
	for _, part := range parts {
		if trimmed := strings.TrimSpace(part); trimmed != "" {
			audiences = append(audiences, trimmed)
		}
	}
	return audiences
}

// buildLedgerFromConfig selects the durable Postgres ledger. Ephemeral storage
// is available only through an explicit local-test opt-in; connection or
// migration failure otherwise prevents startup so accounting is never lost
// silently.
func buildLedgerFromConfig(ctx context.Context, dsn string, allowEphemeral bool) (ledger.Ledger, error) {
	if dsn == "" {
		if allowEphemeral {
			slog.Warn("using explicitly enabled in-memory cost ledger")
			return ledger.NewStore(), nil
		}
		return nil, fmt.Errorf("DATABASE_URL is required unless COST_CORE_ALLOW_EPHEMERAL=true")
	}
	pool, err := postgres.Connect(ctx, dsn)
	if err != nil {
		return nil, fmt.Errorf("connect postgres: %w", err)
	}
	pgStore, err := postgres.New(pool)
	if err != nil {
		pool.Close()
		return nil, fmt.Errorf("initialize postgres ledger: %w", err)
	}
	slog.Info("using durable Postgres ledger")
	return pgStore, nil
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

	nc, err := nats.Connect(natsURL, natsAuthOptions()...)
	if err != nil {
		slog.Error("NATS connect failed; USAGE_ENVELOPE subscriber disabled", "error", err)
		runFeedFallback(ctx, srv)
		return
	}
	defer nc.Close()

	sub, err := nc.Subscribe(usageSubjectWildcard, func(msg *nats.Msg) {
		handleUsageMessage(ctx, srv, msg.Subject, msg.Data)
	})
	if err != nil {
		slog.Error("NATS subscribe failed; USAGE_ENVELOPE subscriber disabled", "error", err, "subject", usageSubjectWildcard)
		runFeedFallback(ctx, srv)
		return
	}
	defer func() { _ = sub.Unsubscribe() }()

	slog.Info("USAGE_ENVELOPE subscriber active", "subject", usageSubjectWildcard)
	<-ctx.Done()
}

func natsAuthOptions() []nats.Option {
	options := []nats.Option{
		nats.RetryOnFailedConnect(true),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(2 * time.Second),
		nats.CustomInboxPrefix(natsInboxPrefix),
	}
	user := strings.TrimSpace(os.Getenv("NATS_USER"))
	password := strings.TrimSpace(os.Getenv("NATS_PASSWORD"))
	if user != "" || password != "" {
		return append(options, nats.UserInfo(user, password))
	}
	if token := strings.TrimSpace(os.Getenv("NATS_AUTH_TOKEN")); token != "" && os.Getenv("NATS_ALLOW_TOKEN_FALLBACK") == "1" {
		return append(options, nats.Token(token))
	}
	return options
}

// runOrgErasureConsumer connects to the shared cross-plane control-shared-nats
// broker over a SECOND, narrowly-scoped connection (identity "cost-core-gdpr",
// configured by COST_CORE_GDPR_NATS_URL/_USER/_PASSWORD — deliberately
// distinct env var names from NATS_URL/NATS_USER/NATS_PASSWORD above, which
// configure the Model-Plane-local broker the USAGE_ENVELOPE subscriber uses)
// and binds the pre-provisioned GDPR org-erasure consumer. An unset
// COST_CORE_GDPR_NATS_URL disables only this consumer — every other cost-core
// function (recording, budget checks, USAGE_ENVELOPE ingestion) is unaffected.
func runOrgErasureConsumer(ctx context.Context, store ledger.Ledger) {
	natsURL := strings.TrimSpace(os.Getenv("COST_CORE_GDPR_NATS_URL"))
	if natsURL == "" {
		slog.Info("COST_CORE_GDPR_NATS_URL not set; GDPR org-erasure consumer disabled")
		return
	}

	nc, err := nats.Connect(natsURL, gdprNatsAuthOptions()...)
	if err != nil {
		slog.Error("shared-broker NATS connect failed; GDPR org-erasure consumer disabled", "error", err)
		return
	}
	defer nc.Close()

	js, err := nc.JetStream()
	if err != nil {
		slog.Error("shared-broker JetStream context failed; GDPR org-erasure consumer disabled", "error", err)
		return
	}

	orgErasureConsumer := consumers.NewOrgErasureConsumer(js, store)
	if err := orgErasureConsumer.Start(ctx); err != nil {
		slog.Error("GDPR org-erasure consumer bind failed (is it pre-provisioned on control-shared-nats?)", "error", err)
		return
	}
	defer orgErasureConsumer.Stop()

	slog.Info("GDPR org-erasure consumer active")
	<-ctx.Done()
}

func gdprNatsAuthOptions() []nats.Option {
	options := []nats.Option{
		nats.RetryOnFailedConnect(true),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(2 * time.Second),
		nats.CustomInboxPrefix(gdprInboxPrefix),
	}
	user := strings.TrimSpace(os.Getenv("COST_CORE_GDPR_NATS_USER"))
	password := strings.TrimSpace(os.Getenv("COST_CORE_GDPR_NATS_PASSWORD"))
	if user != "" || password != "" {
		return append(options, nats.UserInfo(user, password))
	}
	return options
}

// handleUsageMessage decodes a usage envelope and records it in the ledger.
func handleUsageMessage(ctx context.Context, srv *server.Server, subject string, data []byte) {
	entry, err := decodeUsageMessage(subject, data, time.Now().UTC())
	if err != nil {
		slog.Warn("rejected usage envelope", "subject", subject, "error", err)
		return
	}
	if err := srv.RecordUsage(ctx, entry); err != nil {
		slog.Warn("failed to record usage envelope", "org_id", entry.OrgID, "error", err)
	}
}

// decodeUsageMessage binds the tenant carried by the NATS subject to both the
// canonical envelope and its payload before the event can affect accounting.
// NATS credentials still need per-producer permissions: the producer field is
// an allowlisted attribution value, not cryptographic workload identity. These
// checks prevent malformed/confused-deputy events from changing tenant scope.
func decodeUsageMessage(subject string, data []byte, createdAt time.Time) (ledger.Entry, error) {
	if len(data) == 0 || len(data) > maxUsageMessageBytes {
		return ledger.Entry{}, fmt.Errorf("usage envelope size is outside the supported range")
	}
	parts := strings.Split(subject, ".")
	if len(parts) != 4 || parts[0] != "mp" || parts[1] != "v1" || parts[2] != "usage" || strings.TrimSpace(parts[3]) == "" {
		return ledger.Entry{}, fmt.Errorf("invalid usage subject")
	}
	subjectOrgID := parts[3]

	var env envelope
	if err := json.Unmarshal(data, &env); err != nil {
		return ledger.Entry{}, fmt.Errorf("decode envelope: %w", err)
	}
	if env.EventType != "USAGE_ENVELOPE" {
		return ledger.Entry{}, fmt.Errorf("unexpected event type")
	}
	if env.Producer != usageEnvelopeProducer {
		return ledger.Entry{}, fmt.Errorf("usage envelope producer is not allowed")
	}
	if strings.TrimSpace(env.IdempotencyKey) == "" {
		return ledger.Entry{}, fmt.Errorf("idempotency_key is required")
	}
	if env.OrgID != subjectOrgID {
		return ledger.Entry{}, fmt.Errorf("subject and envelope organization mismatch")
	}
	if strings.TrimSpace(env.UserID) == "" {
		return ledger.Entry{}, fmt.Errorf("user attribution is required")
	}

	var p usagePayload
	if len(env.Payload) > 0 {
		if err := json.Unmarshal(env.Payload, &p); err != nil {
			return ledger.Entry{}, fmt.Errorf("decode payload: %w", err)
		}
	}
	if p.OrgID != "" && p.OrgID != subjectOrgID {
		return ledger.Entry{}, fmt.Errorf("payload organization mismatch")
	}
	if p.UserID != "" && env.UserID != "" && p.UserID != env.UserID {
		return ledger.Entry{}, fmt.Errorf("payload user mismatch")
	}
	if p.RequestID != "" && env.CorrelationID != "" && p.RequestID != env.CorrelationID {
		return ledger.Entry{}, fmt.Errorf("payload request mismatch")
	}

	entry := ledger.Entry{
		OrgID:                    subjectOrgID,
		UserID:                   firstNonEmpty(p.UserID, env.UserID),
		ProducerID:               env.Producer,
		RunID:                    p.RunID,
		RequestID:                firstNonEmpty(p.RequestID, env.CorrelationID),
		Model:                    p.Model,
		InputTokens:              p.InputTokens,
		OutputTokens:             p.OutputTokens,
		CostUSD:                  p.CostUSD,
		IdempotencyKey:           env.IdempotencyKey,
		CreatedAt:                createdAt,
		CacheReadInputTokens:     p.CacheReadInputTokens,
		CacheCreationInputTokens: p.CacheCreationInputTokens,
	}
	if err := ledger.ValidateEntry(entry); err != nil {
		return ledger.Entry{}, err
	}
	return entry, nil
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
			if err := srv.RecordUsage(ctx, ledger.Entry{
				OrgID:        p.OrgID,
				UserID:       p.UserID,
				ProducerID:   "feed:file",
				RunID:        p.RunID,
				RequestID:    p.RequestID,
				Model:        p.Model,
				InputTokens:  p.InputTokens,
				OutputTokens: p.OutputTokens,
				CostUSD:      p.CostUSD,
			}); err != nil {
				slog.Warn("rejected usage feed entry", "error", err)
			}
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
