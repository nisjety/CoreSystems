// quarry-orchestrator-go — Temporal workflows + schedules + webhooks + durable state.
//
// Outer envelope for durable runs. Per-page hot path lives in quarry-runtime-rs.
package main

import (
	"context"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/worker"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/pkg/quarryotel"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/jobs"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/schedules"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/workflows"
)

const TaskQueue = "quarry-orchestrator"

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnixMs
	log.Logger = log.Output(zerolog.ConsoleWriter{Out: os.Stderr, TimeFormat: time.RFC3339})

	otelCtx, otelCancel := context.WithCancel(context.Background())
	defer otelCancel()
	otelShutdown, err := quarryotel.Init(otelCtx, "quarry-orchestrator", "0.1.0")
	if err != nil {
		log.Warn().Err(err).Msg("OTEL init failed; continuing without tracing")
	}
	defer func() {
		if err := otelShutdown(context.Background()); err != nil {
			log.Warn().Err(err).Msg("OTEL shutdown failed")
		}
	}()

	hostPort := envOr("TEMPORAL_ADDR", "localhost:7233")
	namespace := envOr("TEMPORAL_NAMESPACE", "default")
	runtimeURL := envOr("RUNTIME_BASE_URL", "http://quarry-runtime:8082")
	controlURL := envOr("CONTROL_BASE_URL", "http://quarry-control:8081")
	// The baseline/diff store is served by quarry-edge alongside run_page, so
	// the change-record endpoint defaults to the same base URL as the runtime
	// (in the monorepo deployment the edge IS the runtime, on :8082).
	edgeURL := envOr("EDGE_BASE_URL", runtimeURL)
	runtimeToken := os.Getenv("RUNTIME_AUTH_TOKEN")
	controlToken := os.Getenv("CONTROL_AUTH_TOKEN")
	// quarry-edge's internal change-record endpoint shares the runtime
	// service token by default (both are internal-auth peers); override
	// with EDGE_AUTH_TOKEN if the edge is keyed separately.
	edgeToken := os.Getenv("EDGE_AUTH_TOKEN")
	if edgeToken == "" {
		edgeToken = runtimeToken
	}

	c, err := client.Dial(client.Options{HostPort: hostPort, Namespace: namespace})
	if err != nil {
		log.Fatal().Err(err).Msg("temporal dial")
	}
	defer c.Close()

	acts := activities.New(activities.Config{
		RuntimeBaseURL:   runtimeURL,
		ControlBaseURL:   controlURL,
		RuntimeAuthToken: runtimeToken,
		ControlAuthToken: controlToken,
		EdgeBaseURL:      edgeURL,
		EdgeAuthToken:    edgeToken,
	})

	w := worker.New(c, TaskQueue, worker.Options{})
	// Workflow functions take `*activities.Activities` as a third
	// parameter so they can call activity methods (`a.RunPage`, etc.)
	// directly. The Temporal Go SDK can't serialize that pointer over
	// the wire, so when a client calls ExecuteWorkflow they pass only
	// the data args. We bridge by registering thin wrappers that
	// capture `acts` in closure scope and inject it on the worker
	// side. The wrapper's registered name must match the underlying
	// workflow's exported name so client-side ExecuteWorkflow(...,
	// workflows.CrawlJobWF, input) resolves to the right workflow
	// type.
	w.RegisterWorkflowWithOptions(
		func(ctx workflow.Context, in workflows.ScrapeJobInput) error {
			return workflows.ScrapeJobWF(ctx, in, acts)
		},
		workflow.RegisterOptions{Name: "ScrapeJobWF"},
	)
	w.RegisterWorkflowWithOptions(
		func(ctx workflow.Context, in workflows.CrawlJobInput) error {
			return workflows.CrawlJobWF(ctx, in, acts)
		},
		workflow.RegisterOptions{Name: "CrawlJobWF"},
	)
	w.RegisterWorkflowWithOptions(
		func(ctx workflow.Context, in workflows.BatchJobInput) error {
			return workflows.BatchJobWF(ctx, in, acts)
		},
		workflow.RegisterOptions{Name: "BatchJobWF"},
	)
	w.RegisterWorkflowWithOptions(
		func(ctx workflow.Context, in workflows.ChangeMonitorInput) error {
			return workflows.ChangeMonitorWF(ctx, in, acts)
		},
		workflow.RegisterOptions{Name: "ChangeMonitorWF"},
	)
	w.RegisterActivity(acts)

	// Root context cancelled on SIGINT / SIGTERM. All long-running
	// goroutines (schedules manager, jobs dispatcher) accept it so a
	// graceful shutdown actually stops them — previously they ran on
	// `context.Background()` and kept polling control after the
	// process started its shutdown sequence, dirtying state on the
	// way out.
	rootCtx, rootCancel := signal.NotifyContext(
		context.Background(),
		syscall.SIGINT, syscall.SIGTERM,
	)
	defer rootCancel()

	schedMgr := schedules.New(c, schedules.Config{
		ControlBaseURL:   controlURL,
		ControlAuthToken: controlToken,
		TaskQueue:        TaskQueue,
		Interval:         30 * time.Second,
	})
	go schedMgr.Run(rootCtx)

	// Ad-hoc jobs dispatcher: polls control for newly POSTed jobs (the
	// REST entrypoint velion's onboarding crawl-preview uses) and
	// starts the matching Temporal workflow. Without this, ad-hoc jobs
	// never execute — only scheduled cron jobs would.
	jobsMgr := jobs.New(c, jobs.Config{
		ControlBaseURL:   controlURL,
		ControlAuthToken: controlToken,
		TaskQueue:        TaskQueue,
		Interval:         2 * time.Second,
	})
	go jobsMgr.Run(rootCtx)

	// Use rootCtx.Done() instead of worker.InterruptCh() so signal
	// handling has a single owner. Previously worker.InterruptCh() and
	// the manual signal.Notify both listened for SIGTERM and raced;
	// whichever ran first decided the shutdown path.
	interruptCh := make(chan interface{}, 1)
	go func() {
		<-rootCtx.Done()
		interruptCh <- struct{}{}
	}()
	workerErr := make(chan error, 1)
	go func() {
		workerErr <- w.Run(interruptCh)
	}()

	log.Info().Str("queue", TaskQueue).Msg("quarry-orchestrator-go running")

	// Block until either the root ctx is cancelled OR the worker
	// returns an error on its own. Either path triggers shutdown.
	select {
	case <-rootCtx.Done():
	case err := <-workerErr:
		if err != nil {
			log.Error().Err(err).Msg("worker run failed")
		}
	}
	log.Info().Msg("shutdown")
}

func envOr(k, d string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return d
}
