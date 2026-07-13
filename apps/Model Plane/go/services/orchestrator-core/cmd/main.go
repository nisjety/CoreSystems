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
	"strings"
	"syscall"

	"github.com/nats-io/nats.go"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/worker"
	"google.golang.org/grpc"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/workflows"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/compat"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/config"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/natsadapter"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/orchestration"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	slog.Info("orchestrator-core starting")

	cfg := config.Load()

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer cancel()

	// gRPC clients to sibling services (non-blocking; logs warnings on dial failure)
	clients, cerr := grpcclient.Dial(ctx, grpcclient.Options{
		SessionCoreAddr:    cfg.SessionCoreAddr,
		InferenceCoreAddr:  cfg.InferenceCoreAddr,
		ExecutionCoreAddr:  cfg.ExecutionCoreAddr,
		CapabilityCoreAddr: cfg.CapabilityCoreAddr,
		SandboxManagerAddr: cfg.SandboxManagerAddr,
		BrowserBrokerAddr:  cfg.BrowserBrokerAddr,
		LettaBridgeAddr:    cfg.LettaBridgeAddr,
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
	// Operator-rating accumulator backing the feedback → skill-promotion loop.
	feedbackStore := activities.NewFeedbackStore()
	a.SetFeedbackStore(feedbackStore)

	w := worker.New(c, cfg.TaskQueue, worker.Options{})
	w.RegisterWorkflow(workflows.InteractiveRunSupervision)
	w.RegisterWorkflow(workflows.DeepTaskWorkflow)
	w.RegisterWorkflow(workflows.MemoryConsolidationWorkflow)
	w.RegisterWorkflow(workflows.SkillPromotionWorkflow)
	w.RegisterWorkflow(workflows.FeedbackPromotionWorkflow)
	w.RegisterWorkflow(workflows.WideResearchWorkflow)

	w.RegisterActivity(a.StartRunActivity)
	w.RegisterActivity(a.ExecuteStepLoopActivity)
	w.RegisterActivity(a.CompleteRunActivity)
	w.RegisterActivity(a.FailRunActivity)
	w.RegisterActivity(a.QueryMemoryEntriesActivity)
	w.RegisterActivity(a.SummarizeMemoryActivity)
	w.RegisterActivity(a.WriteConsolidatedMemoryActivity)
	w.RegisterActivity(a.ValidateSkillBundleActivity)
	w.RegisterActivity(a.RunPromotionGateActivity)
	w.RegisterActivity(a.UpdateRegistryActivity)
	w.RegisterActivity(a.AggregateFeedbackActivity)

	go func() {
		if err := w.Run(worker.InterruptCh()); err != nil {
			slog.Error("temporal worker error", "error", err)
		}
	}()

	// Optional NATS compat adapter (skip cleanly if NATS_URL unset or dial fails)
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
			slog.Info("NATS compat adapter online", "mode", mode)

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

			// Feedback → skill-promotion loop: fold operator ratings into the
			// in-memory accumulator that AggregateFeedbackActivity reads.
			if _, ferr := adapter.Subscribe("mp.v1.feedback.rated", func(subj string, d []byte) {
				var env envelope.Envelope
				if derr := json.Unmarshal(d, &env); derr != nil {
					slog.Error("feedback envelope decode failed", "subject", subj, "error", derr)
					return
				}
				var p struct {
					SkillID   string `json:"skill_id"`
					FromScope string `json:"from_scope"`
					ToScope   string `json:"to_scope"`
					Rating    string `json:"rating"`
				}
				if perr := json.Unmarshal(env.Payload, &p); perr != nil {
					slog.Error("feedback payload decode failed", "subject", subj, "error", perr)
					return
				}
				feedbackStore.Record(p.SkillID, p.FromScope, p.ToScope, p.Rating)
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

	lis, lerr := net.Listen("tcp", cfg.OrchestratorGRPCAddr)
	if lerr != nil {
		slog.Error("orchestrator grpc listen failed", "addr", cfg.OrchestratorGRPCAddr, "error", lerr)
		os.Exit(1)
	}
	gs := grpc.NewServer()
	mpv1.RegisterOrchestrationCoreServiceServer(gs, handlers)
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

func natsAuthOptions() []nats.Option {
	token := strings.TrimSpace(os.Getenv("NATS_AUTH_TOKEN"))
	if token == "" {
		return nil
	}
	return []nats.Option{nats.Token(token)}
}
