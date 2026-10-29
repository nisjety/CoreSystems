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

	"github.com/triodelab/quarry-v2/pkg/quarryotel"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
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
	runtimeToken := os.Getenv("RUNTIME_AUTH_TOKEN")
	controlToken := os.Getenv("CONTROL_AUTH_TOKEN")

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
	})

	w := worker.New(c, TaskQueue, worker.Options{})
	w.RegisterWorkflow(workflows.ScrapeJobWF)
	w.RegisterWorkflow(workflows.CrawlJobWF)
	w.RegisterWorkflow(workflows.BatchJobWF)
	w.RegisterActivity(acts)

	schedMgr := schedules.New(c, schedules.Config{
		ControlBaseURL:   controlURL,
		ControlAuthToken: controlToken,
		TaskQueue:        TaskQueue,
		Interval:         30 * time.Second,
	})
	go schedMgr.Run(context.Background())

	go func() {
		if err := w.Run(worker.InterruptCh()); err != nil {
			log.Fatal().Err(err).Msg("worker run")
		}
	}()

	log.Info().Str("queue", TaskQueue).Msg("quarry-orchestrator-go running")

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	<-sig
	log.Info().Msg("shutdown")
}

func envOr(k, d string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return d
}
