// Package main is the entry point for capability-core.
// Policy and capability authority — skill registry, tool metadata, eligibility.
package main

import (
	"context"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/api"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/commands"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
	"github.com/triodelab/model-plane/services/capability-core/internal/roadmap"
	capserver "github.com/triodelab/model-plane/services/capability-core/internal/server"
	"github.com/triodelab/model-plane/services/capability-core/internal/sessionreview"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("capability-core starting")

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	authConfig, err := authConfigFromEnv()
	if err != nil {
		slog.Error("capability-core authentication configuration unavailable", "error", err)
		os.Exit(1)
	}
	verifier, err := authctx.NewVerifier(authConfig)
	if err != nil {
		slog.Error("capability-core authentication unavailable", "error", err)
		os.Exit(1)
	}

	dsn := os.Getenv("DATABASE_URL")
	if dsn == "" {
		slog.Error("DATABASE_URL required")
		os.Exit(1)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		slog.Error("pgx pool", "error", err)
		os.Exit(1)
	}
	defer pool.Close()

	// --- registry sources ---------------------------------------------------
	// Postgres-backed live source merged with static seed.
	pgSource, err := registry.NewCapabilitiesSource(pool, "")
	if err != nil {
		slog.Error("capabilities source unavailable", "error", err)
		os.Exit(1)
	}
	reg, err := registry.NewFromSource(pgSource)
	if err != nil {
		slog.Error("postgres capabilities source load failed; migration 0006 and database health are required", "error", err)
		os.Exit(1)
	}

	modelsReg, err := registry.NewModelsRegistry(pool)
	if err != nil {
		slog.Error("models registry", "error", err)
		os.Exit(1)
	}

	capStore, err := registry.NewCapabilitiesStore(pool)
	if err != nil {
		slog.Error("capabilities store unavailable", "error", err)
		os.Exit(1)
	}

	// The durable scope-grant store is the required tenant-org invocation
	// authority. Construction failure aborts startup above; policy must never
	// fall back to static EnabledForScopes for an org-scoped decision.
	scopeStore, err := registry.NewScopeStore(pool)
	if err != nil {
		slog.Error("scope store unavailable", "error", err)
		os.Exit(1)
	}

	pol := policy.New(reg).WithScopeResolver(scopeStore)

	// Dial session-core + inference-core ONCE (guarded on their addrs; lazy grpc
	// clients). Shared by the /commands delegation (/models → inference
	// ListModels, /compact → session CompactNow) and the G7 learning consumer —
	// one dial site, no duplication. Nil when addrs are unset → both consumers
	// degrade gracefully.
	sessionClient, inferenceClient := dialBackends()

	// --- §4.3 reconcile event publisher -------------------------------------
	// capability-core is the registry system-of-record; on a create/update it
	// emits mp.v1.capability.<kind>.<action> so cache holders (the gateway's
	// runtime registries) stay coherent. The gateway-side consumer is built
	// (model-gateway capability_consumer); this is the emit half. Best-effort
	// and guarded: with NATS_URL unset the publisher is nil and reconcile.Emit
	// no-ops, so the service runs cleanly without a bus. ModeV1Only because
	// capability events are v1-native (no legacy mapping).
	var recPub publisher.EventPublisher
	if natsURL := os.Getenv("NATS_URL"); natsURL != "" {
		nc, nerr := nats.Connect(natsURL, natsAuthOptions()...)
		if nerr != nil {
			slog.Warn("NATS connect failed; capability reconcile events disabled", "error", nerr)
		} else {
			defer nc.Close()
			recPub = publisher.NewNATSPublisher(natsx.NewPublisher(nc, natsx.ModeV1Only))
			slog.Info("capability reconcile events enabled")

			// G7 learning-review trigger: on RUN_COMPLETED, review the session
			// and persist learned skills. Needs session-core + inference-core;
			// guarded on their addrs (absent → consumer not started, service
			// still runs). The decode→review→persist core is unit-tested
			// (sessionreview.HandleRunCompleted); this wiring is e2e-verified
			// only against the running stack.
			startLearningConsumer(ctx, nc, sessionClient, inferenceClient)
		}
	}

	// --- HTTP server on :8085 -----------------------------------------------
	publicMux := http.NewServeMux()
	publicMux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	publicMux.HandleFunc("/readyz", func(w http.ResponseWriter, request *http.Request) {
		readyCtx, cancel := context.WithTimeout(request.Context(), 2*time.Second)
		defer cancel()
		if err := pool.Ping(readyCtx); err != nil {
			http.Error(w, "database unavailable", http.StatusServiceUnavailable)
			return
		}
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	protectedMux := http.NewServeMux()
	protectedMux.Handle("/api/v1/model-plane/implementation-status", roadmap.NewHandler())

	// Capability-core product APIs. The scope store (when available) enables
	// the durable grant/revoke/resolve endpoints and ranked listing.
	if capStore != nil {
		ch := api.NewCapabilitiesHandler(capStore)
		if scopeStore != nil {
			ch = ch.WithScopeStore(scopeStore)
		}
		ch.Register(protectedMux)
	}
	// The four reconcile-emitting registries get the publisher (nil-safe: a nil
	// recPub makes reconcile.Emit a no-op).
	api.NewSkillsHandler(pool).WithPublisher(recPub).Register(protectedMux)
	api.NewPluginsHandler(pool).WithPublisher(recPub).Register(protectedMux)
	api.NewMCPHandler(pool).WithPublisher(recPub).Register(protectedMux)
	api.NewRoutingHandler(pool).WithPublisher(recPub).Register(protectedMux)
	api.NewSafetyHandler(pool).WithPublisher(recPub).Register(protectedMux)
	api.NewMemoryHandler(pool).Register(protectedMux)
	api.NewTasksHandler(pool).Register(protectedMux)
	api.NewCronHandler(pool).Register(protectedMux)

	// Cron sweeper: fires due cron_schedules — creates a task per fire, records
	// cron_fires, and advances next_fire_at, single-flight across replicas via
	// FOR UPDATE SKIP LOCKED. Opt out with CRON_SWEEPER_ENABLED=false.
	if os.Getenv("CRON_SWEEPER_ENABLED") != "false" {
		go cron.NewSweeper(pool).Start(ctx)
		slog.Info("cron sweeper started")
	}
	// /models delegates to inference-core ListModels, /compact to session-core
	// CompactNow (nil-safe: unwired → honest "unavailable").
	commands.NewHandler().WithModels(inferenceClient).WithCompactor(sessionClient).Register(protectedMux)
	publicMux.Handle("/", verifier.HTTPMiddleware(authz.AuthorizeHTTP)(protectedMux))

	healthServer := &http.Server{
		Addr:              ":8085",
		Handler:           publicMux,
		ReadHeaderTimeout: 10 * time.Second,
	}
	go func() {
		slog.Info("health server listening", "addr", ":8085")
		if err := healthServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("health server error", "error", err)
		}
	}()

	// gRPC server on :9097
	lis, err := net.Listen("tcp", ":9097")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	grpcServer := grpc.NewServer(grpc.UnaryInterceptor(verifier.UnaryServerInterceptor(authz.AuthorizeGRPC)))
	// Attach the durable store so ListCapabilities returns score-ranked results
	// (nil-safe: WithStore(nil) keeps the in-memory registry ordering).
	capSrv := capserver.NewServer(reg, modelsReg, pol).WithStore(capStore)
	capserver.Register(grpcServer, capSrv)

	go func() {
		slog.Info("gRPC listening", "addr", ":9097")
		if err := grpcServer.Serve(lis); err != nil {
			slog.Error("gRPC serve error", "error", err)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down")
	grpcServer.GracefulStop()
	_ = healthServer.Shutdown(context.Background())
}

func natsAuthOptions() []nats.Option {
	options := []nats.Option{nats.CustomInboxPrefix("_INBOX.MODEL_RUNTIME")}
	user := strings.TrimSpace(os.Getenv("NATS_USER"))
	password := strings.TrimSpace(os.Getenv("NATS_PASSWORD"))
	if user != "" || password != "" {
		return append(options, nats.UserInfo(user, password))
	}
	token := strings.TrimSpace(os.Getenv("NATS_AUTH_TOKEN"))
	if token == "" || os.Getenv("NATS_ALLOW_TOKEN_FALLBACK") != "1" {
		return options
	}
	return append(options, nats.Token(token))
}

func authConfigFromEnv() (authctx.Config, error) {
	audience := strings.TrimSpace(os.Getenv("CAPABILITY_CORE_AUTH_AUDIENCE"))
	issuer := strings.TrimSpace(os.Getenv("AUTH_CORE_ISSUER"))
	jwksURL := strings.TrimSpace(os.Getenv("AUTH_CORE_JWKS_URL"))
	for name, value := range map[string]string{
		"CAPABILITY_CORE_AUTH_AUDIENCE": audience,
		"AUTH_CORE_ISSUER":              issuer,
		"AUTH_CORE_JWKS_URL":            jwksURL,
	} {
		if value == "" {
			return authctx.Config{}, fmt.Errorf("%s is required", name)
		}
	}
	return authctx.Config{Audiences: []string{audience}, Issuer: issuer, JWKSURL: jwksURL}, nil
}

// startLearningConsumer wires the G7 learning-review trigger on the shared
// session-core + inference-core clients: spawn the RUN_COMPLETED consumer
// (sessionreview). No-op when the clients are nil (backends unset) so the
// service runs without the learning loop. The decode→review→persist core is
// unit-tested; this subscribe wiring is e2e-verified only against the stack.
func startLearningConsumer(ctx context.Context, nc *nats.Conn, sc mpv1.SessionCoreClient, ic mpv1.InferenceCoreClient) {
	if sc == nil || ic == nil {
		slog.Info("learning-review consumer disabled (session-core/inference-core not dialed)")
		return
	}
	model := os.Getenv("LEARNING_REVIEW_MODEL") // empty -> llmreviewer.DefaultModel
	go func() {
		if rerr := sessionreview.RunConsumer(ctx, nc, sc, ic, model); rerr != nil {
			slog.Warn("learning-review consumer stopped", "error", rerr)
		}
	}()
}

// dialBackends dials session-core + inference-core once (guarded on
// SESSION_CORE_ADDR + INFERENCE_CORE_ADDR; lazy grpc clients shared by the
// /commands delegation and the learning consumer — one dial site, no
// duplication). Returns (nil, nil) when the addrs are unset or a dial fails, so
// callers degrade gracefully.
func dialBackends() (mpv1.SessionCoreClient, mpv1.InferenceCoreClient) {
	sessAddr := os.Getenv("SESSION_CORE_ADDR")
	infAddr := os.Getenv("INFERENCE_CORE_ADDR")
	if sessAddr == "" || infAddr == "" {
		slog.Info("backend clients disabled (SESSION_CORE_ADDR/INFERENCE_CORE_ADDR unset)")
		return nil, nil
	}
	creds := grpc.WithTransportCredentials(insecure.NewCredentials())
	sessConn, err := grpc.NewClient(sessAddr, creds)
	if err != nil {
		slog.Warn("dial session-core failed", "error", err)
		return nil, nil
	}
	infConn, err := grpc.NewClient(infAddr, creds)
	if err != nil {
		slog.Warn("dial inference-core failed", "error", err)
		_ = sessConn.Close()
		return nil, nil
	}
	return mpv1.NewSessionCoreClient(sessConn), mpv1.NewInferenceCoreClient(infConn)
}
