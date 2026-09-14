// Package main is the entry point for capability-core.
// Policy and capability authority — skill registry, tool metadata, eligibility.
package main

import (
	"context"
	"encoding/base64"
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
	"github.com/triodelab/model-plane/pkg/servicetoken"
	"github.com/triodelab/model-plane/services/capability-core/internal/api"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/commands"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
	"github.com/triodelab/model-plane/services/capability-core/internal/crypto"
	"github.com/triodelab/model-plane/services/capability-core/internal/lettatools"
	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/processwatch"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
	"github.com/triodelab/model-plane/services/capability-core/internal/roadmap"
	"github.com/triodelab/model-plane/services/capability-core/internal/runwatch"
	capserver "github.com/triodelab/model-plane/services/capability-core/internal/server"
	"github.com/triodelab/model-plane/services/capability-core/internal/sessionreview"
	"github.com/triodelab/model-plane/services/capability-core/internal/taskexec"
	"github.com/triodelab/model-plane/services/capability-core/internal/watch"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

const natsInboxPrefix = "_INBOX.CAPABILITY_CORE_RUNTIME"

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

	// MCP OAuth token encryption (mcp_oauth_tokens.access_token/refresh_token
	// at rest). Optional: a missing/invalid key leaves the oauth-token(s)
	// endpoints 503ing rather than falling back to plaintext storage.
	var mcpVault *crypto.Vault
	if rawKey := os.Getenv("MCP_TOKEN_ENCRYPTION_KEY"); rawKey != "" {
		key, decodeErr := base64.StdEncoding.DecodeString(rawKey)
		if decodeErr != nil || len(key) != 32 {
			slog.Error("MCP_TOKEN_ENCRYPTION_KEY must be base64-encoded 32 bytes; MCP OAuth token storage disabled", "error", decodeErr)
		} else if vault, vaultErr := crypto.NewVault(key); vaultErr != nil {
			slog.Error("mcp token vault init failed; MCP OAuth token storage disabled", "error", vaultErr)
		} else {
			mcpVault = vault
			slog.Info("mcp token vault ready")
		}
	} else {
		slog.Warn("MCP_TOKEN_ENCRYPTION_KEY unset; MCP OAuth token storage disabled")
	}

	// Model-Plane-local service secret for model-gateway's execution-core-
	// triggered oauth-token resolve and its refresh-writeback (no live per-user
	// bearer exists on that path). Optional: unset simply leaves those internal
	// paths disabled — the per-user JWT paths are unaffected either way.
	mcpServiceToken := os.Getenv("MCP_OAUTH_SERVICE_TOKEN")
	if mcpServiceToken == "" {
		slog.Warn("MCP_OAUTH_SERVICE_TOKEN unset; model-gateway cannot resolve OAuth-connected MCP tokens at tool-call time")
	}

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

	// Startup-loaded cache over the models table (PROVIDER_AND_PRIVACY_STRATEGY.md
	// §4.5): ListCapabilities/GetCapability must not perform a synchronous
	// Postgres round-trip per request. modelsCache is what the server actually
	// reads from; modelsReg remains available for future admin-side writes.
	modelsCache, err := registry.NewModelsCache(ctx, modelsReg)
	if err != nil {
		slog.Error("models cache initial load failed", "error", err)
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

	// Letta's /v1/tools/search searches tool definitions, not memories. When
	// explicitly configured it is only a discovery ranker; the server
	// intersects results with the tenant-scoped durable catalog and execution
	// still performs its own authoritative capability-policy check.
	lettaConfig, err := lettatools.ConfigFromLookup(os.Getenv)
	if err != nil {
		slog.Error("Letta tool search configuration invalid", "error", err)
		os.Exit(1)
	}
	var lettaToolSearcher *lettatools.Client
	if lettaConfig.Enabled {
		lettaToolSearcher, err = lettatools.New(lettaConfig)
		if err != nil {
			slog.Error("Letta tool search configuration invalid", "error", err)
			os.Exit(1)
		}
		slog.Info("Letta tool-definition ranking enabled")
	}

	// Dial session-core + inference-core ONCE (guarded on their addrs; lazy grpc
	// clients). Shared by the /commands delegation (/models → inference
	// ListModels, /compact → session CompactNow) and the G7 learning consumer —
	// one dial site, no duplication. Nil when addrs are unset → both consumers
	// degrade gracefully.
	sessionClient, inferenceClient, runClient := dialBackends()

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

			// Task lifecycle closer: a task the executor started stays in
			// `running` until its run reaches a terminal state. Without this the
			// workflow dispatcher would strand every cron-fired task exactly the
			// way the publish-only dispatcher did.
			startTaskCompletionConsumer(ctx, nc, pool)

			// AUTO-2 run-watch notify consumer: on a terminal run event, notify
			// every user who registered a watch on that run (see
			// api.RunWatchersHandler for how a watch gets registered). Needs
			// notification-core's URL + this service's delegated service token;
			// guarded independently of the two consumers above.
			startRunWatchConsumer(ctx, nc, pool)
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
		if modelActionViewVerifier, viewVerifierErr := modelActionViewVerifierFromEnv(os.Getenv); viewVerifierErr != nil {
			slog.Warn("run-bound Model owner-action view unavailable; Control public key is required", "error", viewVerifierErr)
		} else {
			ch = ch.WithModelActionViewVerifier(modelActionViewVerifier)
		}
		ch.Register(protectedMux)
	}
	// The four reconcile-emitting registries get the publisher (nil-safe: a nil
	// recPub makes reconcile.Emit a no-op).
	api.NewSkillsHandler(pool).WithPublisher(recPub).Register(protectedMux)
	api.NewPluginsHandler(pool).WithPublisher(recPub).Register(protectedMux)
	mcpHandler := api.NewMCPHandler(pool).WithPublisher(recPub).WithVault(mcpVault).WithMCPServiceToken(mcpServiceToken)
	mcpHandler.Register(protectedMux)
	// Deliberately on publicMux, not protectedMux: see RegisterInternal's doc
	// comment — these routes authenticate themselves (X-Mcp-Service-Token) and
	// must not sit behind the per-user JWT middleware wrapping protectedMux
	// below, which a service-to-service caller has no bearer to satisfy.
	mcpHandler.RegisterInternal(publicMux)

	// MCP DNS revalidator: normalizeMCPRegistration's DNS-address check only
	// ever runs once, at registration/update time — DNS answers can legitimately
	// drift afterward even without an attacker. This periodically re-resolves
	// every enabled MCP server's hostname and quarantines it if DNS now lands
	// inside a forbidden range. Opt out with MCP_DNS_REVALIDATOR_ENABLED=false;
	// override the interval with MCP_DNS_REVALIDATION_INTERVAL (Go duration,
	// e.g. "5m").
	if os.Getenv("MCP_DNS_REVALIDATOR_ENABLED") != "false" {
		interval := api.DefaultMCPDNSRevalidationInterval
		if raw := os.Getenv("MCP_DNS_REVALIDATION_INTERVAL"); raw != "" {
			if parsed, perr := time.ParseDuration(raw); perr == nil && parsed > 0 {
				interval = parsed
			} else {
				slog.Warn("invalid MCP_DNS_REVALIDATION_INTERVAL; using default",
					"value", raw, "default", interval)
			}
		}
		go api.NewMCPDNSRevalidator(pool, interval).WithPublisher(recPub).Start(ctx)
		slog.Info("mcp dns revalidator started", "interval", interval)
	}
	api.NewRoutingHandler(pool).WithPublisher(recPub).Register(protectedMux)
	api.NewSafetyHandler(pool).WithPublisher(recPub).Register(protectedMux)
	api.NewMemoryHandler(pool, runClient).Register(protectedMux)
	api.NewTasksHandler(pool).Register(protectedMux)
	// AUTO-2: lets a user register/inspect/cancel a watch on one run. The
	// consumer that actually fires the notification is wired below,
	// independently, once NATS is available (see startRunWatchConsumer).
	api.NewRunWatchersHandler(pool).Register(protectedMux)
	cronHandler := api.NewCronHandler(pool)
	if cronVerifier, cronVerifierErr := cronDecisionVerifierFromEnv(os.Getenv); cronVerifierErr != nil {
		slog.Warn("Space-scoped cron creation unavailable; Control decision verifier is required", "error", cronVerifierErr)
	} else {
		cronHandler.WithSpaceDecisionVerifier(cronVerifier)
	}
	cronHandler.Register(protectedMux)
	cronHandler.RegisterDeletionAdapter(protectedMux)

	// Cron sweeper: fires due cron_schedules — creates a task per fire, records
	// cron_fires, and advances next_fire_at, single-flight across replicas via
	// FOR UPDATE SKIP LOCKED. Opt out with CRON_SWEEPER_ENABLED=false.
	cronAuthorizer, cronAuthErr := cronFireAuthorizerFromEnv(os.Getenv)
	if os.Getenv("CRON_SWEEPER_ENABLED") != "false" {
		if cronAuthErr != nil {
			slog.Warn("cron sweeper not started; fresh Control authorization is required", "error", cronAuthErr)
		} else {
			go cron.NewSweeper(pool, cronAuthorizer).Start(ctx)
			slog.Info("cron sweeper started with fresh Control authorization")
		}
	}

	// Watch sweeper (S4.3): claims due space_watches, polls each one's source
	// adapter, and commits what it saw. Single-flight across replicas via the
	// same FOR UPDATE SKIP LOCKED claim the cron sweeper uses — except the claim
	// commits BEFORE the poll, so no transaction is held open across an RPC.
	//
	// Started only when the process-output adapter can be built, because that is
	// the only source this binary knows: a sweeper with no adapters claims rows
	// and backs every one of them off, which is cost without work.
	//
	// It cannot emit anything yet regardless. Step 3 supplies the Reauthorizer,
	// and until it does the sweeper REFUSES to observe — the record is not the
	// authority, and a poll without a fresh authority check is the one thing
	// this loop must never do. There is also no create path yet, so there are no
	// rows to claim in the first place.
	if os.Getenv("WATCH_SWEEPER_ENABLED") != "false" {
		if adapters, adapterErr := processWatchAdaptersFromEnv(os.Getenv); adapterErr != nil {
			slog.Warn("watch sweeper not started; the process-output source is unavailable", "error", adapterErr)
		} else if watchStore, storeErr := watch.NewStore(pool); storeErr != nil {
			slog.Warn("watch sweeper not started", "error", storeErr)
		} else if sweeper, sweepErr := watch.NewSweeper(watchStore, adapters, nil); sweepErr != nil {
			slog.Warn("watch sweeper not started", "error", sweepErr)
		} else {
			go sweeper.Run(ctx)
			slog.Info("watch sweeper started",
				"sources", len(adapters),
				"observing", false,
				"reason", "a reauthorizer is required before any watch may observe (S4.3 step 3)")
		}
	}

	// Task executor: claims `created` tasks (single-flight), marks them running,
	// and turns each into a durable Temporal run via orchestrator-core's
	// StartWorkflow RPC — the hand-off that closes the cron → run loop.
	//
	// This shipped disabled because the only dispatcher published an event
	// nothing consumed, so every claimed task stranded in `running`. With the
	// workflow dispatcher the hand-off is synchronous: a failure to start is
	// recorded as a task failure with a reason instead of a silent stall, so the
	// executor now defaults ON whenever that dispatcher is available.
	var taskFireAuthorizer taskexec.ScheduleFireAuthorizer
	if cronAuthErr == nil {
		taskFireAuthorizer = cronAuthorizer
	}
	dispatcher, workflowBacked := buildTaskDispatcher(pool, recPub, taskFireAuthorizer, sessionClient)
	if taskExecutorEnabled(workflowBacked) {
		go taskexec.NewExecutor(pool, dispatcher).Start(ctx)
		slog.Info("task executor started", "workflow_backed", workflowBacked)
	} else {
		slog.Info("task executor not started",
			"workflow_backed", workflowBacked,
			"reason", "TASK_EXECUTOR_ENABLED=false, or no orchestrator-core workflow dispatcher configured")
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
	capSrv := capserver.NewServer(reg, modelsCache, pol).WithStore(capStore)
	if lettaToolSearcher != nil {
		capSrv.WithLettaToolSearcher(lettaToolSearcher)
	}
	if signer, signerErr := capserver.NewDecisionProofSignerFromEnv(); signerErr != nil {
		slog.Warn("capability decision evidence signer unavailable; execution allow responses will lack per-call proof", "error", signerErr)
	} else {
		capSrv.WithDecisionProofSigner(signer)
		slog.Info("capability decision evidence signer enabled")
	}
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
	options := []nats.Option{nats.CustomInboxPrefix(natsInboxPrefix)}
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

// cronFireAuthorizerFromEnv keeps the scheduler's service identity and
// Control verification key deployment-managed. Missing configuration disables
// the sweeper rather than allowing an older org-wide schedule to execute.
func cronFireAuthorizerFromEnv(getenv func(string) string) (*cron.ControlFireAuthorizer, error) {
	if getenv == nil {
		return nil, fmt.Errorf("cron environment reader is required")
	}
	return cron.NewControlFireAuthorizer(
		getenv("CONTROL_USER_CORE_URL"),
		getenv("CAPABILITY_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN"),
		getenv("CONTROL_SPACE_DECISION_KEY_ID"),
		getenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64"),
		nil,
	)
}

func cronDecisionVerifierFromEnv(getenv func(string) string) (*cron.ControlDecisionVerifier, error) {
	if getenv == nil {
		return nil, fmt.Errorf("cron environment reader is required")
	}
	return cron.NewControlDecisionVerifier(
		getenv("CONTROL_SPACE_DECISION_KEY_ID"),
		getenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64"),
	)
}

// modelActionViewVerifierFromEnv pins the same deployment-distributed Control
// public key used by other Space decision recipients. It has no fallback: an
// unsigned view could make an owner-plane effect look model-eligible.
func modelActionViewVerifierFromEnv(getenv func(string) string) (*api.ControlModelActionViewVerifier, error) {
	if getenv == nil {
		return nil, fmt.Errorf("model action view environment reader is required")
	}
	return api.NewControlModelActionViewVerifier(
		getenv("CONTROL_SPACE_DECISION_KEY_ID"),
		getenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64"),
	)
}

// startTaskCompletionConsumer closes a task when the run it started finishes.
//
// It is the other half of the executor's safety story: buildTaskDispatcher makes
// the hand-off synchronous so a failure to START is recorded, and this makes the
// hand-off finite so a started run's task cannot sit in `running` forever.
func startTaskCompletionConsumer(ctx context.Context, nc *nats.Conn, pool *pgxpool.Pool) {
	consumer, err := taskexec.NewRunCompletionConsumer(pool)
	if err != nil {
		slog.Error("task completion consumer unavailable; started tasks will stay in `running`", "error", err)
		return
	}
	go func() {
		if rerr := consumer.Run(ctx, nc); rerr != nil {
			slog.Warn("task completion consumer stopped", "error", rerr)
		}
	}()
}

// startRunWatchConsumer wires AUTO-2's durable JetStream consumer: on a
// terminal run event, notify every user who registered a watch on that run
// (api.RunWatchersHandler is where a watch gets registered).
//
// Needs BOTH NOTIFICATION_CORE_URL and
// NOTIFICATION_CAPABILITY_CORE_SERVICE_TOKEN — notifyclient.New reports
// (nil, false) when either is blank, and that alone disables this consumer
// (logged), independently of the task-completion and learning-review
// consumers above. Also needs a JetStream context on the same connection:
// unlike those two (plain core-NATS nc.Subscribe/QueueSubscribe), this
// consumer binds a pre-provisioned DURABLE consumer with manual ack (see
// runwatch's package doc for why), which requires nats-provisioner to have
// already registered RunWatchDurable on RunEventsStream.
func startRunWatchConsumer(ctx context.Context, nc *nats.Conn, pool *pgxpool.Pool) {
	notifyClient, enabled := notifyclient.New(
		os.Getenv("NOTIFICATION_CORE_URL"),
		os.Getenv("NOTIFICATION_CAPABILITY_CORE_SERVICE_TOKEN"),
		nil,
	)
	if !enabled {
		slog.Info("run-watch notify consumer disabled (NOTIFICATION_CORE_URL/NOTIFICATION_CAPABILITY_CORE_SERVICE_TOKEN not set)")
		return
	}
	notifier, err := runwatch.NewNotifier(runwatch.NewPostgresSubscriptionStore(pool), notifyClient)
	if err != nil {
		slog.Error("run-watch notifier unavailable", "error", err)
		return
	}
	js, err := nc.JetStream()
	if err != nil {
		slog.Error("JetStream context unavailable; run-watch notify consumer disabled", "error", err)
		return
	}
	go func() {
		if rerr := notifier.Run(ctx, js); rerr != nil {
			slog.Warn("run-watch notify consumer stopped (is it pre-provisioned on "+runwatch.RunEventsStream+"?)", "error", rerr)
		}
	}()
	slog.Info("run-watch notify consumer started", "stream", runwatch.RunEventsStream, "durable", runwatch.RunWatchDurable)
}

// buildTaskDispatcher returns the task dispatcher plus whether it can actually
// complete a hand-off.
//
// The workflow-backed dispatcher needs ORCHESTRATOR_WORKFLOW_ADDR (where
// orchestrator-core's OrchestratorWorkflowService listens) and
// ORCHESTRATOR_INTERNAL_SERVICE_TOKEN (the Model-Plane-local credential that
// side accepts, org-bound there by ORCHESTRATOR_INTERNAL_SERVICE_ORGS). Missing
// either, we fall back to the publish-only NatsDispatcher and report false, and
// the caller leaves the executor OFF by default — the honest outcome, since a
// publish that nobody consumes would leave every task in `running`. An operator
// can still force the executor on regardless (TASK_EXECUTOR_ENABLED=true); in
// that case the fallback dispatcher fails each task with a clear reason instead
// of stranding it, unless TASK_EXECUTOR_ACCEPT_PUBLISH_ONLY_DISPATCH=true
// acknowledges that this deployment runs its own consumer of the published
// subject (see NatsDispatcher's doc comment).
func buildTaskDispatcher(pool *pgxpool.Pool, pub publisher.EventPublisher, fireAuthorizer taskexec.ScheduleFireAuthorizer, sessionClient mpv1.SessionCoreClient) (taskexec.Dispatcher, bool) {
	fallback := taskexec.NewNatsDispatcher(pub, acceptsPublishOnlyDispatch())

	addr := strings.TrimSpace(os.Getenv("ORCHESTRATOR_WORKFLOW_ADDR"))
	token := strings.TrimSpace(os.Getenv("ORCHESTRATOR_INTERNAL_SERVICE_TOKEN"))

	// Prefer a minted service JWT over the shared secret. Only a JWT carries a
	// SIGNED retention posture, and orchestrator-core refuses the three workflows
	// that persist derived content (MemoryConsolidation, SkillPromotion,
	// FeedbackPromotion) to any caller without one — by identity, so no amount of
	// configuration on the shared-secret path can reach them.
	//
	// Assigned through an explicitly nil-checked interface variable: a nil
	// *servicetoken.Provider stored in a non-nil interface would make the
	// dispatcher believe it can mint and lose the shared-secret fallback.
	var workflowMinter taskexec.WorkflowMinter
	orchestratorCredential := newBackendCredential(
		"orchestrator-core",
		"ORCHESTRATOR_CORE_SERVICE_TOKEN",
		taskexec.WorkflowStartScopes,
		orchestratorCoreTokenReason,
	)
	if orchestratorCredential.minter != nil {
		workflowMinter = orchestratorCredential.minter
	}

	if addr == "" || (workflowMinter == nil && token == "") {
		slog.Warn("task workflow dispatch disabled; cron-fired tasks cannot become runs",
			"orchestrator_workflow_addr_set", addr != "",
			"service_jwt_available", workflowMinter != nil,
			"internal_service_token_set", token != "")
		return fallback, false
	}

	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		slog.Error("dial orchestrator-core workflow service failed", "addr", addr, "error", err)
		return fallback, false
	}
	dispatcher, err := taskexec.NewWorkflowDispatcher(
		pool,
		mpv1.NewOrchestratorWorkflowServiceClient(conn),
		workflowMinter,
		token,
		pub,
		os.Getenv("TASK_DEFAULT_WORKFLOW_TYPE"),
		fireAuthorizer,
	)
	if err != nil {
		slog.Error("task workflow dispatcher unavailable", "error", err)
		_ = conn.Close()
		return fallback, false
	}
	// Scoped cron fires prepare their deterministic service-owned Session Core
	// thread before Temporal starts. This is deliberately configured only after
	// the dispatcher exists; an absent Session Core client leaves those fires
	// fail-closed in the dispatcher instead of falling back to a generic run.
	dispatcher.SetScheduledRunSession(sessionClient)
	slog.Info("task workflow dispatch enabled",
		"orchestrator_workflow_addr", addr,
		"credential", map[bool]string{true: "minted service JWT (signed retention posture)",
			false: "shared secret (workflows that persist derived content will be refused)"}[workflowMinter != nil])
	return dispatcher, true
}

// acceptsPublishOnlyDispatch reports whether this deployment has explicitly
// acknowledged that it runs its own consumer of
// mp.v1.capability.task.dispatched, so NatsDispatcher's publish-then-fail
// safety net (see its doc comment) should stand down and let a published
// task remain `running` for that consumer to complete. Defaults to false:
// this repository ships no such consumer, so the safe default is to fail a
// task loudly rather than assume one exists.
func acceptsPublishOnlyDispatch() bool {
	return strings.EqualFold(strings.TrimSpace(os.Getenv("TASK_EXECUTOR_ACCEPT_PUBLISH_ONLY_DISPATCH")), "true")
}

// taskExecutorEnabled resolves the executor's on/off decision.
//
// Unset defaults to the safe answer: on only when the dispatcher can complete a
// hand-off. "false" always wins. An explicit "true" is honoured even without the
// workflow dispatcher, because a deployment may run its own consumer of
// mp.v1.capability.task.dispatched — but that combination is logged loudly
// because, absent TASK_EXECUTOR_ACCEPT_PUBLISH_ONLY_DISPATCH=true, every claimed
// task will now fail immediately (with a clear reason) rather than execute or
// silently strand.
func taskExecutorEnabled(workflowBacked bool) bool {
	switch strings.ToLower(strings.TrimSpace(os.Getenv("TASK_EXECUTOR_ENABLED"))) {
	case "false":
		return false
	case "true":
		if !workflowBacked && !acceptsPublishOnlyDispatch() {
			slog.Error("TASK_EXECUTOR_ENABLED=true without a workflow dispatcher and without " +
				"TASK_EXECUTOR_ACCEPT_PUBLISH_ONLY_DISPATCH=true: the executor will start, but every " +
				"claimed task will fail immediately with an explicit reason instead of running — this " +
				"repository ships no consumer of mp.v1.capability.task.dispatched")
		}
		return true
	default:
		return workflowBacked
	}
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
// duplication). Returns (nil, nil, nil) when the addrs are unset or a dial
// fails, so callers degrade gracefully.
//
// The returned RunServiceClient (MEM-2) is built on the same sessConn as the
// SessionCoreClient — RunService is served on session-core's same tonic
// multiplexed server (see grpc.rs's single Server::builder()...add_service
// chain), so it needs no separate dial, env var, or credential.
func dialBackends() (mpv1.SessionCoreClient, mpv1.InferenceCoreClient, mpv1.RunServiceClient) {
	sessAddr := os.Getenv("SESSION_CORE_ADDR")
	infAddr := os.Getenv("INFERENCE_CORE_ADDR")
	if sessAddr == "" || infAddr == "" {
		slog.Info("backend clients disabled (SESSION_CORE_ADDR/INFERENCE_CORE_ADDR unset)")
		return nil, nil, nil
	}
	creds := grpc.WithTransportCredentials(insecure.NewCredentials())

	// Both cores authenticate every RPC. Without a credential the learning
	// consumer reaches session-core and gets
	// "Unauthenticated: verified caller credential required" on its very first
	// transcript read, so the review never runs no matter how many RUN_COMPLETED
	// events arrive. These credentials are the only way this service can present
	// one — see newBackendCredential for how each one is obtained.
	sessConn, err := grpc.NewClient(sessAddr, creds,
		newBackendCredential(
			"session-core",
			"SESSION_CORE_SERVICE_TOKEN",
			sessionCoreScopes,
			sessionCoreTokenReason,
		).dialOption())
	if err != nil {
		slog.Warn("dial session-core failed", "error", err)
		return nil, nil, nil
	}
	infConn, err := grpc.NewClient(infAddr, creds,
		newBackendCredential(
			"inference-core",
			"INFERENCE_CORE_SERVICE_TOKEN",
			inferenceCoreScopes,
			inferenceCoreTokenReason,
		).dialOption())
	if err != nil {
		slog.Warn("dial inference-core failed", "error", err)
		_ = sessConn.Close()
		return nil, nil, nil
	}
	return mpv1.NewSessionCoreClient(sessConn), mpv1.NewInferenceCoreClient(infConn), mpv1.NewRunServiceClient(sessConn)
}

// Auth Core service-principal configuration for capability-core's OWN identity.
// This is the credential that is exchanged for per-audience, per-tenant backend
// tokens; it is never sent to session-core or inference-core themselves.
const (
	// authCoreURLEnv is Auth Core's base URL. capability-core already knows
	// AUTH_CORE_JWKS_URL / AUTH_CORE_ISSUER for *verifying* inbound tokens; this
	// is the separate base URL used to *mint* outbound ones.
	authCoreURLEnv = "AUTH_CORE_URL"
	// serviceIDEnv / serviceCredentialEnv follow the naming every other minting
	// service in this plane already uses (EXECUTION_CORE_SERVICE_ID /
	// EXECUTION_CORE_SERVICE_API_KEY, SESSION_CORE_SERVICE_ID / ...).
	serviceIDEnv         = "CAPABILITY_CORE_SERVICE_ID"
	serviceCredentialEnv = "CAPABILITY_CORE_SERVICE_API_KEY"

	defaultAuthCoreURL = "http://auth-core:3011"
	defaultServiceID   = "capability-core"
)

// Per-audience scopes and audit reasons. Deliberately NOT a shared union: each
// audience is minted with exactly the scopes it needs, so a session-core token
// can never invoke inference and an inference token can never write skills.
var (
	// sessionCoreScopes covers the learning review's transcript read
	// (ListConversation, ListAgentSkills → session:read) and its skill upsert
	// (UpsertAgentSkill → session:skills:write).
	sessionCoreScopes = []string{"session:read", "session:skills:write", "session:schedule-prepare"}
	// inferenceCoreScopes covers the review's model call (Infer).
	inferenceCoreScopes = []string{"inference:invoke"}
)

const (
	sessionCoreTokenReason      = "capability-core learning review: read a completed run's transcript and upsert learned skills"
	inferenceCoreTokenReason    = "capability-core learning review: extract skill candidates from a completed run"
	orchestratorCoreTokenReason = "capability-core task executor: start the durable workflow for a fired cron task"
)

// backendCredential is capability-core's credential source for ONE downstream
// audience, and the interceptor that presents it.
//
// Exactly one of three modes is chosen at startup and logged once:
//
//   - static: the legacy `*_SERVICE_TOKEN` env var is set, so it is used
//     verbatim and nothing is minted. Kept as a break-glass and test path.
//   - minted: a service-principal credential is available, so a short-lived
//     token is minted per tenant and refreshed ahead of expiry.
//   - none: neither is available. RPCs go out unauthenticated and the backend
//     rejects them — today's behaviour, preserved so a dev-bypass backend keeps
//     working — but the startup WARN now names exactly what is missing.
//
// The mode is logged because a silent choice is how the previous gap stayed
// invisible: a pasted token that expired minutes after boot produced logs
// identical to a healthy consumer.
type backendCredential struct {
	backend  string
	tokenEnv string
	static   bool
	minter   *servicetoken.Provider
}

func newBackendCredential(backend, tokenEnv string, scopes []string, reason string) *backendCredential {
	credential := &backendCredential{backend: backend, tokenEnv: tokenEnv}

	if strings.TrimSpace(os.Getenv(tokenEnv)) != "" {
		credential.static = true
		slog.Info("backend credential mode: static env override (no minting)",
			"backend", backend, "env", tokenEnv,
			"note", "a minted Auth Core plane token is short-lived (300s by default), "+
				"so a pasted one will start returning Unauthenticated once it expires; "+
				"clear "+tokenEnv+" to mint on demand instead")
		return credential
	}

	minter, err := servicetoken.New(servicetoken.Config{
		AuthCoreURL: envOrDefault(authCoreURLEnv, defaultAuthCoreURL),
		ServiceID:   envOrDefault(serviceIDEnv, defaultServiceID),
		Credential:  os.Getenv(serviceCredentialEnv),
		Audience:    backend,
		Scopes:      scopes,
		Reason:      reason,
	})
	if err != nil {
		// Non-fatal by design: a missing credential must not stop capability-core
		// from serving its registry, policy and task surfaces. The learning review
		// is the part that goes dark, and this WARN is what says so.
		slog.Warn("backend credential unavailable; this backend's RPCs will be rejected as unauthenticated "+
			"and the learning review will not run",
			"backend", backend, "error", err,
			"remedy", "set "+serviceCredentialEnv+" (and "+authCoreURLEnv+" if not "+defaultAuthCoreURL+
				") to mint on demand, or "+tokenEnv+" for a static break-glass token")
		return credential
	}

	credential.minter = minter
	slog.Info("backend credential mode: minted on demand from auth-core",
		"backend", backend,
		"audience", minter.Audience(),
		"scopes", minter.Scopes(),
		"service_id", envOrDefault(serviceIDEnv, defaultServiceID),
		"auth_core_url", envOrDefault(authCoreURLEnv, defaultAuthCoreURL))
	return credential
}

// dialOption returns the unary interceptor that authenticates every outbound RPC
// on the connection.
// processWatchAdaptersFromEnv builds the S4.3 source adapters this binary can
// serve.
//
// Today that is exactly one: background process output, read from
// sandbox-manager's registry. It needs a gRPC target and capability-core's own
// `aud=sandbox-manager` credential — which Auth Core will only issue if
// capability-core's entry in plane-service-principals.json grants that audience
// with `sandbox:read`. That grant is part of S4.3 step 2; without it every poll
// is refused at sandbox-manager's interceptor, and the error says so.
func processWatchAdaptersFromEnv(getenv func(string) string) (watch.AdapterSet, error) {
	target := strings.TrimSpace(getenv("SANDBOX_MANAGER_GRPC_ADDR"))
	if target == "" {
		return nil, fmt.Errorf("SANDBOX_MANAGER_GRPC_ADDR is required to watch process output")
	}
	serviceID := strings.TrimSpace(getenv("CAPABILITY_CORE_SERVICE_ID"))
	if serviceID == "" {
		serviceID = "capability-core"
	}
	tokens, err := processwatch.NewAuthCoreTokens(
		getenv("AUTH_CORE_URL"), serviceID, getenv("CAPABILITY_CORE_SERVICE_API_KEY"), nil)
	if err != nil {
		return nil, err
	}
	conn, err := grpc.NewClient(target, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil, fmt.Errorf("dial sandbox-manager: %w", err)
	}
	adapter, err := processwatch.NewAdapter(mpv1.NewSandboxManagerClient(conn), tokens)
	if err != nil {
		return nil, err
	}
	return watch.NewAdapterSet(adapter)
}

func (b *backendCredential) dialOption() grpc.DialOption {
	return grpc.WithChainUnaryInterceptor(b.intercept)
}

func (b *backendCredential) intercept(
	ctx context.Context,
	method string,
	req, reply any,
	cc *grpc.ClientConn,
	invoker grpc.UnaryInvoker,
	opts ...grpc.CallOption,
) error {
	switch {
	case b.static:
		// Re-read per call rather than captured at dial time, so a rotated token
		// takes effect on the next RPC instead of at the next restart.
		if token := strings.TrimSpace(os.Getenv(b.tokenEnv)); token != "" {
			ctx = withBearer(ctx, token)
		}
		return invoker(ctx, method, req, reply, cc, opts...)
	case b.minter == nil:
		// No credential at all. Pass through so the backend answers with its own
		// explicit Unauthenticated (or succeeds, if it runs an auth dev-bypass)
		// rather than this process inventing a different failure.
		return invoker(ctx, method, req, reply, cc, opts...)
	}

	org := outboundOrg(ctx, req)
	if org == "" {
		// Minting without a tenant is not possible: the mint request carries orgId
		// and session-core requires the request's org_id to equal the token's
		// exactly. Guessing an org would produce a token that authenticates and is
		// then refused on every call, which is strictly worse than saying so.
		return status.Errorf(codes.FailedPrecondition,
			"capability-core cannot mint a %s credential for %s: the request carries no org_id "+
				"and the calling context has no verified principal",
			b.backend, method)
	}

	token, err := b.minter.Token(ctx, org)
	if err != nil {
		slog.Warn("minting a backend service token failed",
			"backend", b.backend, "method", method, "org_id", org, "error", err)
		return status.Errorf(codes.Unavailable,
			"capability-core could not obtain a %s credential", b.backend)
	}

	err = invoker(withBearer(ctx, token), method, req, reply, cc, opts...)
	if status.Code(err) != codes.Unauthenticated {
		return err
	}

	// 401 backstop — one forced re-mint and one retry, never a loop.
	//
	// Expiry is normally handled by the refresh margin; reaching here means the
	// credential was rejected while we believed it live: a rotated principal, a
	// re-keyed issuer, or clock skew. Retrying is safe even for the skill upsert
	// because Unauthenticated is returned before the RPC does any work, so
	// nothing was written on the first attempt.
	//
	// Note both invocations derive from the ORIGINAL ctx: metadata is appended,
	// not replaced, so reusing the first attempt's context would send two
	// authorization values.
	b.minter.Invalidate(org)
	fresh, mintErr := b.minter.Token(ctx, org)
	if mintErr != nil {
		slog.Warn("re-minting a backend service token after Unauthenticated failed",
			"backend", b.backend, "method", method, "org_id", org, "error", mintErr)
		return err
	}
	slog.Info("re-minted a backend service token after Unauthenticated; retrying once",
		"backend", b.backend, "method", method, "org_id", org)
	return invoker(withBearer(ctx, fresh), method, req, reply, cc, opts...)
}

// outboundOrg resolves the tenant an outbound RPC acts for, so the minted token
// carries exactly that org.
//
// This is what makes the learning loop work for every tenant rather than one:
// session-core's authorize_org requires an exact org_id match, so a single
// startup-minted token would serve exactly one organisation and silently skip
// every run belonging to any other.
//
// Preference order:
//  1. The request message's own org_id. Every learning-review RPC carries one —
//     ListConversation, ListAgentSkills, UpsertAgentSkill, Infer — which is why
//     lazy per-run minting is possible at all.
//  2. The verified principal on the calling context. The /commands delegation
//     (ListModels, CompactNow) has no org_id field on the wire, and those
//     handlers run behind the HTTP auth middleware, so acting as the caller's own
//     org is both available and correct.
//
// Only a verified principal is consulted — never a header or a request field
// other than the tenant the message itself declares.
//
// Deployment note: minting for an arbitrary tenant requires this service's
// Auth Core principal to be registered with `allowAnyOrg: true` (as in the dev
// deployment). Without it, only orgs listed in the principal's `orgIds` can be
// minted for and every other tenant's review fails with Unavailable — visible
// in the "minting a backend service token failed" WARN rather than silent.
func outboundOrg(ctx context.Context, req any) string {
	if org := requestOrg(req); org != "" {
		return org
	}
	if principal, ok := authctx.PrincipalFromContext(ctx); ok {
		return strings.TrimSpace(principal.OrganizationID)
	}
	return ""
}

// requestOrg reads org_id off a generated protobuf request when it has one.
// Every mpv1 message with an org_id field exposes GetOrgId(), so this needs no
// per-message wiring and cannot drift as RPCs are added.
func requestOrg(req any) string {
	scoped, ok := req.(interface{ GetOrgId() string })
	if !ok {
		return ""
	}
	return strings.TrimSpace(scoped.GetOrgId())
}

// withBearer attaches a bearer credential to an outgoing gRPC context.
func withBearer(ctx context.Context, token string) context.Context {
	return metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+token)
}

// envOrDefault returns the trimmed env value or fallback when unset/blank.
func envOrDefault(name, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(name)); value != "" {
		return value
	}
	return fallback
}
