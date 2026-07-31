// Package main is the entry point for orchestrator-core.
// Temporal-based outer workflow envelope for long-running orchestration.
package main

import (
	"context"
	"encoding/json"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"slices"
	"strings"
	"syscall"

	"github.com/nats-io/nats.go"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/worker"
	"go.temporal.io/sdk/workflow"
	"google.golang.org/grpc"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/workflows"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/compat"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/config"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/feedback"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/natsadapter"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/orchestration"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/servicecred"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("orchestrator-core starting")

	cfg := config.Load()

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	// Outbound credential for the Temporal activity path. An activity's context
	// comes from the worker, not a gRPC handler, so there is no inbound bearer
	// to forward and every sibling answers Unauthenticated without this. A
	// failure here is a WARN, not fatal: the inbound proxy path (run-event
	// streaming, approvals) forwards the caller's own credential and keeps
	// working, so refusing to boot would take down working functionality to
	// punish an unconfigured one.
	minters, merr := servicecred.NewMinters(servicecred.Options{
		AuthCoreURL: cfg.AuthCoreURL,
		ServiceID:   cfg.ServicePrincipalID,
		Credential:  cfg.ServicePrincipalKey,
	}, logger)
	if merr != nil {
		slog.Warn("outbound service tokens unavailable; workflow activities "+
			"that call sibling services will fail Unauthenticated", "error", merr)
	} else {
		slog.Info("outbound service tokens configured",
			"audiences", len(minters), "principal", cfg.ServicePrincipalID)
	}

	// gRPC clients to sibling services (non-blocking; logs warnings on dial failure)
	clients, cerr := grpcclient.Dial(ctx, grpcclient.Options{
		SessionCoreAddr:    cfg.SessionCoreAddr,
		InferenceCoreAddr:  cfg.InferenceCoreAddr,
		ExecutionCoreAddr:  cfg.ExecutionCoreAddr,
		CapabilityCoreAddr: cfg.CapabilityCoreAddr,
		SandboxManagerAddr: cfg.SandboxManagerAddr,
		BrowserBrokerAddr:  cfg.BrowserBrokerAddr,
		LettaBridgeAddr:    cfg.LettaBridgeAddr,
		Minters:            minters,
		Logger:             logger,
	})
	if cerr != nil {
		slog.Warn("grpcclient.Dial returned error; continuing", "error", cerr)
	}

	// Health server on :8082
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})
	mux.HandleFunc("/readyz", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	})

	server := &http.Server{Addr: ":8084", Handler: mux}
	go func() {
		slog.Info("health server listening", "addr", ":8084")
		if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("health server error", "error", err)
		}
	}()

	// Temporal worker
	c, err := client.Dial(client.Options{HostPort: cfg.TemporalAddress})
	if err != nil {
		slog.Error("failed to create temporal client", "error", err)
		os.Exit(1)
	}
	defer c.Close()

	a := activities.NewActivities(logger, clients)
	// Durable operator-rating store backing the feedback → skill-promotion loop.
	// Postgres when DATABASE_URL is configured; otherwise an in-memory store
	// that is lost on restart and says so, rather than silently pretending.
	feedbackStore, closeFeedbackStore := buildFeedbackStore(ctx)
	defer closeFeedbackStore()
	a.SetFeedbackStore(feedbackStore)

	w := worker.New(c, cfg.TaskQueue, worker.Options{})
	for _, reg := range registeredWorkflows() {
		w.RegisterWorkflowWithOptions(reg.Fn, workflow.RegisterOptions{Name: reg.Name})
	}

	w.RegisterActivity(a.StartRunActivity)
	// ExecuteStepLoopActivity stays registered for replay of in-flight runs that
	// recorded the legacy (DefaultVersion) single-activity step loop; new runs
	// use the durable per-turn ExecuteStepActivity instead.
	w.RegisterActivity(a.ExecuteStepLoopActivity)
	w.RegisterActivity(a.ExecuteStepActivity)
	w.RegisterActivity(a.CompleteRunActivity)
	w.RegisterActivity(a.FailRunActivity)
	w.RegisterActivity(a.QueryMemoryEntriesActivity)
	w.RegisterActivity(a.SummarizeMemoryActivity)
	w.RegisterActivity(a.WriteConsolidatedMemoryActivity)
	w.RegisterActivity(a.ValidateSkillBundleActivity)
	w.RegisterActivity(a.RunPromotionGateActivity)
	w.RegisterActivity(a.UpdateRegistryActivity)
	w.RegisterActivity(a.AggregateFeedbackActivity)
	w.RegisterActivity(a.EvaluatorOptimizerActivity)
	w.RegisterActivity(a.InferModelActivity)
	w.RegisterActivity(a.PublishEvalRoundActivity)
	w.RegisterActivity(a.RecordEvalOutcomeActivity)

	go func() {
		if err := w.Run(worker.InterruptCh()); err != nil {
			slog.Error("temporal worker error", "error", err)
		}
	}()

	// NATS publishes current v1 lifecycle events. The legacy compatibility
	// adapter is a distinct opt-in path; production Compose keeps it disabled so
	// this principal never receives legacy wildcard permissions.
	var pub *natsx.Publisher
	if natsURL := os.Getenv("NATS_URL"); natsURL != "" {
		nc, nerr := nats.Connect(natsURL, natsAuthOptions()...)
		if nerr != nil {
			slog.Warn("NATS connect failed; continuing without compat adapter", "error", nerr)
		} else {
			defer nc.Close()
			adapter := natsadapter.New(nc)
			mode := natsx.ReadCompatModeFromEnv()
			pub = natsx.NewPublisher(adapter, mode)
			a.SetPublisher(pub)
			if compatAdapterEnabled() {
				compatSub := compat.NewSubscriber(pub, logger)

				handler := func(subject string, data []byte) {
					if herr := compatSub.HandleLegacyMessage(context.Background(), subject, data); herr != nil {
						slog.Error("compat handler error", "subject", subject, "error", herr)
					}
				}

				for _, subj := range []string{
					natsx.LegacyRunEventsWildcard,
					natsx.LegacySessionCommandWildcard,
					natsx.LegacyAqenciaWildcard,
				} {
					if _, serr := adapter.Subscribe(subj, handler); serr != nil {
						slog.Error("compat subscribe failed", "subject", subj, "error", serr)
					} else {
						slog.Info("compat subscription active", "subject", subj, "mode", mode)
					}
				}
				slog.Info("NATS legacy compatibility adapter online", "mode", mode)
			} else {
				slog.Info("NATS legacy compatibility adapter disabled")
			}

			orchPersister := orchestration.NewLoggingPersister(logger)
			orchSub := orchestration.NewSubscriber(orchPersister, logger)
			if _, oerr := adapter.Subscribe("mp.v1.orchestration.>", func(subj string, d []byte) {
				if herr := orchSub.HandleEvent(context.Background(), subj, d); herr != nil {
					slog.Error("orch subscriber error", "subject", subj, "error", herr)
				}
			}); oerr != nil {
				slog.Error("orchestration subscribe failed", "error", oerr)
			} else {
				slog.Info("orchestration subscription active", "subject", "mp.v1.orchestration.>")
			}

			// Feedback → skill-promotion loop: persist operator ratings that
			// AggregateFeedbackActivity later reads. A rating attaches to the
			// run AND to each skill model-gateway injected into that turn, so a
			// thumbs-down can demote the skill that steered a bad answer.
			if _, ferr := adapter.Subscribe("mp.v1.feedback.rated", func(subj string, d []byte) {
				var env envelope.Envelope
				if derr := json.Unmarshal(d, &env); derr != nil {
					slog.Error("feedback envelope decode failed", "subject", subj, "error", derr)
					return
				}
				var p struct {
					RunID     string   `json:"run_id"`
					SkillID   string   `json:"skill_id"`
					SkillIDs  []string `json:"skill_ids"`
					FromScope string   `json:"from_scope"`
					ToScope   string   `json:"to_scope"`
					Rating    string   `json:"rating"`
					Note      string   `json:"note"`
				}
				if perr := json.Unmarshal(env.Payload, &p); perr != nil {
					slog.Error("feedback payload decode failed", "subject", subj, "error", perr)
					return
				}
				base := feedback.Rating{
					OrgID:     env.OrgID,
					UserID:    env.UserID,
					RunID:     p.RunID,
					FromScope: p.FromScope,
					ToScope:   p.ToScope,
					Rating:    p.Rating,
					Note:      p.Note,
					CreatedAt: env.Ts,
				}
				// Row 1 is always the run-level rating (skill_id ""), so a chat
				// turn with no matched skill is still recorded rather than
				// dropped — the pre-fix `if skillID == "" { return }` bug.
				targets := []string{""}
				for _, id := range feedbackSkillIDs(p.SkillIDs, p.SkillID) {
					targets = append(targets, id)
				}
				for _, skillID := range targets {
					r := base
					r.SkillID = skillID
					if rerr := a.RecordFeedback(context.Background(), r); rerr != nil {
						slog.Error("feedback record failed",
							"subject", subj, "run_id", p.RunID, "skill_id", skillID, "error", rerr)
					}
				}
			}); ferr != nil {
				slog.Error("feedback subscribe failed", "error", ferr)
			} else {
				slog.Info("feedback subscription active", "subject", "mp.v1.feedback.rated")
			}
		}
	}

	// Orchestration gRPC server: proxies OrchestrationCoreService to session-core.
	var sessionClient mpv1.OrchestrationCoreServiceClient
	if clients != nil && clients.SessionCore != nil {
		sessionClient = mpv1.NewOrchestrationCoreServiceClient(clients.SessionCore)
	}
	_, handlers := orchestration.New(pub, logger, sessionClient)

	// StartWorkflow: the production entry point into the durable tier. Until it
	// existed, all seven registered workflows had zero callers — nothing outside
	// tests ever started one. It is attached only when a credential source is
	// configured; otherwise the RPC answers Unavailable rather than accepting
	// unauthenticated starts.
	startAuth := orchestration.NewStartWorkflowAuth(
		buildAuthVerifier(cfg.AuthIssuer, cfg.AuthJWKSURL, cfg.AuthAudiences),
		cfg.InternalServiceToken,
		cfg.InternalServiceOrgs,
	)
	var workflowStarts *orchestration.WorkflowStartService
	if startAuth.Configured() {
		workflowStarts = orchestration.NewWorkflowStartService(
			startAuth, orchestration.NewTemporalStarter(c), cfg.TaskQueue,
		)
		slog.Info("StartWorkflow enabled",
			"task_queue", cfg.TaskQueue,
			"allowed_workflow_types", orchestration.AllowedWorkflowTypes(),
			"internal_service_orgs", len(cfg.InternalServiceOrgs))
	} else {
		slog.Warn("StartWorkflow DISABLED: no credential source configured " +
			"(set AUTH_CORE_ISSUER + AUTH_CORE_JWKS_URL, or " +
			"ORCHESTRATOR_INTERNAL_SERVICE_TOKEN + ORCHESTRATOR_INTERNAL_SERVICE_ORGS)")
	}

	lis, lerr := net.Listen("tcp", cfg.OrchestratorGRPCAddr)
	if lerr != nil {
		slog.Error("orchestrator grpc listen failed", "addr", cfg.OrchestratorGRPCAddr, "error", lerr)
		os.Exit(1)
	}
	gs := grpc.NewServer()
	mpv1.RegisterOrchestrationCoreServiceServer(gs, handlers)
	// Second service on the SAME listener. Unregistered when no credential
	// source exists, so an unauthenticated deployment answers Unimplemented
	// instead of starting durable work for anyone who can reach the port.
	if workflowStarts != nil {
		mpv1.RegisterOrchestratorWorkflowServiceServer(gs, workflowStarts)
	}
	go func() {
		slog.Info("orchestrator gRPC listening", "addr", cfg.OrchestratorGRPCAddr)
		if err := gs.Serve(lis); err != nil {
			slog.Error("orchestrator grpc serve error", "error", err)
		}
	}()
	defer gs.GracefulStop()

	<-ctx.Done()
	slog.Info("shutting down")
	_ = server.Shutdown(context.Background())
}

// workflowRegistration binds a wire-visible workflow type name to the function
// that implements it.
type workflowRegistration struct {
	Name string
	Fn   any
}

// registeredWorkflows is the exact set of workflow types this worker serves.
//
// Names are pinned explicitly rather than inferred from the Go function name so
// the wire contract cannot drift on a rename, and so this list can be compared
// against orchestration.AllowedWorkflowTypes(). workflow_parity_test.go asserts
// the two sets are equal: an allowlisted type the worker does not register would
// accept a start that then sits unhandled on the task queue, and a registered
// type missing from the allowlist would be unreachable.
//
// workflows.AutoresearchWorkflow is intentionally absent from BOTH lists. Its
// budget guard multiplies a hardcoded $0.10 by the step count because
// ExecuteStepResponse carries no usage data, so a `budget_usd` cap would look
// enforced while being fiction. Registering it would make that reachable.
func registeredWorkflows() []workflowRegistration {
	return []workflowRegistration{
		{"InteractiveRunSupervision", workflows.InteractiveRunSupervision},
		{"DeepTaskWorkflow", workflows.DeepTaskWorkflow},
		{"MemoryConsolidationWorkflow", workflows.MemoryConsolidationWorkflow},
		{"SkillPromotionWorkflow", workflows.SkillPromotionWorkflow},
		{"FeedbackPromotionWorkflow", workflows.FeedbackPromotionWorkflow},
		{"WideResearchWorkflow", workflows.WideResearchWorkflow},
		{"EvaluatorOptimizerWorkflow", workflows.EvaluatorOptimizerWorkflow},
	}
}

// buildAuthVerifier loads the Auth Core trust material for StartWorkflow.
//
// A nil return means the JWT path is unavailable — StartWorkflow then accepts
// only the Model-Plane-local internal credential, and if that is unset too it
// refuses every call. It is deliberately non-fatal: the Temporal worker and the
// existing session-core read proxies must keep serving even when Auth Core trust
// material is missing, and a missing verifier closes the door rather than
// opening it.
func buildAuthVerifier(issuer, jwksURL string, audiences []string) *authctx.Verifier {
	if issuer == "" || jwksURL == "" || len(audiences) == 0 {
		slog.Warn("StartWorkflow JWT authentication unavailable: incomplete Auth Core configuration",
			"issuer_set", issuer != "", "jwks_set", jwksURL != "", "audiences", len(audiences))
		return nil
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences: audiences,
		Issuer:    issuer,
		JWKSURL:   jwksURL,
	})
	if err != nil {
		slog.Error("StartWorkflow JWT authentication unavailable: verifier init failed", "error", err)
		return nil
	}
	slog.Info("StartWorkflow JWT authentication ready", "audiences", audiences)
	return verifier
}

// buildFeedbackStore returns the durable rating store plus its shutdown hook.
//
// Postgres is the intended sink: ratings must survive a restart, which the old
// in-memory map did not (AGENT_QUALITY_PLAN_2026-07-29 §1.2). When DATABASE_URL
// is absent or unreachable we fall back to the in-memory store so the worker
// still starts — but log at WARN that ratings are ephemeral, because a rating
// store that quietly forgets is worse than one that is obviously missing.
func buildFeedbackStore(ctx context.Context) (feedback.Store, func()) {
	dsn := strings.TrimSpace(os.Getenv("DATABASE_URL"))
	if dsn == "" {
		slog.Warn("DATABASE_URL not set; operator ratings are IN-MEMORY and lost on restart")
		return feedback.NewMemoryStore(), func() {}
	}
	pool, err := feedback.Connect(ctx, dsn)
	if err != nil {
		slog.Error("feedback store: postgres connect failed; ratings are IN-MEMORY and lost on restart",
			"error", err)
		return feedback.NewMemoryStore(), func() {}
	}
	store, serr := feedback.NewPostgresStore(pool)
	if serr != nil {
		slog.Error("feedback store: postgres init failed; ratings are IN-MEMORY and lost on restart",
			"error", serr)
		pool.Close()
		return feedback.NewMemoryStore(), func() {}
	}
	slog.Info("feedback store: postgres-backed (ratings survive restart)")
	return store, pool.Close
}

// feedbackSkillIDs picks the skill ids a rating attaches to: the `skill_ids`
// array when present, else the back-compat single `skill_id`. Blanks and
// duplicates are dropped so one skill cannot be counted twice for one rating.
func feedbackSkillIDs(ids []string, single string) []string {
	if len(ids) == 0 && strings.TrimSpace(single) != "" {
		ids = []string{single}
	}
	out := make([]string, 0, len(ids))
	for _, id := range ids {
		id = strings.TrimSpace(id)
		if id == "" {
			continue
		}
		if slices.Contains(out, id) {
			continue
		}
		out = append(out, id)
	}
	return out
}

func natsAuthOptions() []nats.Option {
	options := []nats.Option{nats.CustomInboxPrefix("_INBOX.ORCHESTRATOR_CORE_RUNTIME")}
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

func compatAdapterEnabled() bool {
	return strings.EqualFold(strings.TrimSpace(os.Getenv("ENABLE_COMPAT_ADAPTER")), "true")
}
